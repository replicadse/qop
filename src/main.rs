include!("check_features.rs");

pub mod args;
pub mod reference;

use std::{
    collections::HashMap, io::Read, path::{
        Path,
        PathBuf,
    }
};

use anyhow::Result;
use args::ManualFormat;
use chrono::{
    DateTime,
    Utc,
};
use sha2::Digest;

const STORE_PATH: &'static str = "./.qop/store";

#[tokio::main]
async fn main() -> Result<()> {
    let cmd = crate::args::ClapArgumentLoader::load()?;

    match cmd.command {
        | crate::args::Command::Manual { path, format } => {
            let out_path = PathBuf::from(path);
            std::fs::create_dir_all(&out_path)?;
            match format {
                | ManualFormat::Manpages => {
                    reference::build_manpages(&out_path)?;
                },
                | ManualFormat::Markdown => {
                    reference::build_markdown(&out_path)?;
                },
            }
            Ok(())
        },
        | crate::args::Command::Autocomplete { path, shell } => {
            let out_path = PathBuf::from(path);
            std::fs::create_dir_all(&out_path)?;
            reference::build_shell_completion(&out_path, &shell)?;
            Ok(())
        },
        | crate::args::Command::Init => {
            write_index().await?;
            Ok(())
        },
        | crate::args::Command::Checkpoint => {
            write_index().await?;
            Ok(())
        },
        | crate::args::Command::Apply { file } => {
            apply(file).await?;
            Ok(())
        },
        | crate::args::Command::Diff { reverse } => {
            diff(reverse).await?;
            Ok(())
        },
        | crate::args::Command::Reverse { file } => {
            reverse(file).await?;
            Ok(())
        }
    }
}

async fn write_index() -> Result<()> {
    fn process_files(path: PathBuf, ignore_stack: &mut Vec<Vec<String>>) -> Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        let dir = std::fs::read_dir(&path)?.filter_map(|entry| entry.ok()).collect::<Vec<_>>();

        let ignore_patterns = match std::fs::read_to_string(Path::join(&path, ".qopfile")) {
            | Ok(s) => {
                let f = toml::from_str::<QopFile>(&s)?;
                f.ignore.iter().map(|x| Path::join(&path, x).to_str().unwrap().to_owned()).collect::<Vec<_>>()
            },
            | Err(_) => Vec::<String>::new(),
        };
        ignore_stack.push(ignore_patterns);

        'entries: for d in dir {
            for ignore_list in ignore_stack.iter() {
                for ignore_pattern in ignore_list {
                    if d.path().starts_with(ignore_pattern) {
                        continue 'entries;
                    }
                }
            }

            let rel_path = d.path();
            let new_path = Path::new(STORE_PATH).join(&rel_path);
            if d.file_type()?.is_dir() {
                std::fs::create_dir(&new_path)?;
                files.extend(process_files(d.path(), ignore_stack)?);
            } else {
                let hash = hex::encode(sha2::Sha256::digest(std::fs::read(&rel_path)?));
                files.insert(d.path().to_string_lossy().to_string(), hash);
                std::fs::copy(&rel_path, new_path)?;
            }
        }
        ignore_stack.pop();

        Ok(files)
    }

    let _ = std::fs::remove_dir_all(STORE_PATH);
    std::fs::create_dir_all(STORE_PATH)?;

    let index = Index {
        latest: None,
        entries: HashMap::new(),
        files: process_files(PathBuf::from("."), &mut Vec::new())?,
    };

    std::fs::write("./.qop/index.toml", toml::to_string(&index)?)?;
    Ok(())
}

async fn diff(reverse: bool) -> Result<()> {
    let index = toml::from_str::<Index>(&std::fs::read_to_string("./.qop/index.toml")?)?;
    let mut patch = Patch { files: HashMap::new() };
    for (path, store_hash) in index.files {
        let store_path = Path::new(STORE_PATH).join(&path);
        let wc_path = Path::new(&path);

        let wc_file_content = std::fs::read_to_string(&wc_path)?;
        let wc_hash = hex::encode(sha2::Sha256::digest(&wc_file_content));

        if wc_hash == store_hash {
            continue;
        }
        let store_file_content = std::fs::read_to_string(&store_path)?;

        let diff = if !reverse {
            similar::TextDiff::from_lines(&store_file_content, &wc_file_content)
        } else {
            similar::TextDiff::from_lines(&wc_file_content, &store_file_content)
        };

        let mut diff_hunks = Vec::<PatchFileHunk>::new();
        for hunk in diff.unified_diff().context_radius(0).iter_hunks() {
            let ops = hunk.ops();
            let first_op = ops[0];
            let last_op = ops[ops.len() - 1];
            
            let mut diff = Vec::<String>::new();
            for c in hunk.iter_changes() {
                match c.tag() {
                    | similar::ChangeTag::Equal => {
                        diff.push(format!(" {}", c.value()));
                    },
                    | similar::ChangeTag::Insert => {
                        diff.push(format!("+{}", c.value()));
                    },
                    | similar::ChangeTag::Delete => {
                        diff.push(format!("-{}", c.value()));
                    },   
                }
            }

            diff_hunks.push(PatchFileHunk {
                old_range: (first_op.old_range().start, last_op.old_range().end),
                new_range: (first_op.new_range().start, last_op.new_range().end),
                diff: diff.concat(),
            });
        }

        patch.files.insert(path, PatchFile {
            pre_hash: store_hash,
            post_hash: wc_hash,
            hunks: diff_hunks,
        });
    }

    println!("{}", toml::to_string(&patch)?);
    Ok(())
}

async fn apply(file: String) -> Result<()> {
    let patch = if file == "-" {
        toml::from_str::<Patch>(&{
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        })?
    } else {
        toml::from_str::<Patch>(&std::fs::read_to_string(file)?)?
    };

    for mut patch_file in patch.files {
        patch_file.1.hunks.sort_by(|a, b| a.old_range.0.cmp(&b.old_range.0));

        let mut line_idx = 0_usize;
        let mut file_new = Vec::<String>::new();
        let file_old = std::fs::read_to_string(&patch_file.0)?;
        let mut file_old_iter = file_old.lines();
        'eof: for hunk in patch_file.1.hunks {
            while line_idx < hunk.new_range.0 {
                if let Some(v) = file_old_iter.next() {
                    file_new.push(v.to_owned());
                    line_idx += 1;
                } else {
                    break 'eof;
                }
            }
            // skip remove lines
            for _ in 0..(hunk.old_range.1 - hunk.old_range.0) {
                let _ = file_old_iter.next();
            }
            // insert new lines
            for add_line in hunk.diff.lines().filter(|x| x.starts_with('+')) {
                file_new.push(add_line[1..].to_owned());
            }
            line_idx = hunk.new_range.1;
        }
        while let Some(line) = file_old_iter.next() {
            file_new.push(line.to_owned());
        }

        std::fs::write(&patch_file.0, file_new.join("\n"))?;
    }

    Ok(())
}

async fn reverse(file: String) -> Result<()> {
    let mut patch = if file == "-" {
        toml::from_str::<Patch>(&{
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        })?
    } else {
        toml::from_str::<Patch>(&std::fs::read_to_string(file)?)?
    };

    for patch_file in &mut patch.files {
        for hunk in patch_file.1.hunks.iter_mut() {
            let mut diff = Vec::<String>::new();
            for c in hunk.diff.lines() {
                match c.chars().next() {
                    | Some('+') => {
                        diff.push(format!("-{}", &c[1..]));
                    },
                    | Some('-') => {
                        diff.push(format!("+{}", &c[1..]));
                    },
                    | _ => {
                        diff.push(c.to_owned());
                    },
                }
            }
            hunk.diff = diff.join("\n");
            std::mem::swap(&mut hunk.new_range, &mut hunk.old_range);
        }
    }

    println!("{}", toml::to_string(&patch)?);
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QopFile {
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Index {
    pub latest: Option<String>,
    pub entries: HashMap<String, IndexEntry>,
    pub files: HashMap<String, String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexEntry {
    pub instant: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Patch {
    pub files: HashMap<String, PatchFile>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatchFile {
    pub pre_hash: String,
    pub post_hash: String,
    pub hunks: Vec<PatchFileHunk>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatchFileHunk {
    pub old_range: (usize, usize),
    pub new_range: (usize, usize),
    pub diff: String,
}
