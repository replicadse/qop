include!("check_features.rs");

pub mod args;
pub mod reference;

use std::{
    collections::{
        BTreeMap,
        HashMap,
    }, io::Read, path::{
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
            init().await?;
            Ok(())
        },
        | crate::args::Command::Apply { file } => {
            apply(file).await?;
            Ok(())
        },
        | crate::args::Command::Reverse { file } => {
            reverse(file).await?;
            Ok(())
        },
        | crate::args::Command::Diff { reverse } => {
            diff(reverse).await?;
            Ok(())
        },
    }
}

async fn init() -> Result<()> {
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
            let new_path = Path::new("./.qop/store").join(&rel_path);
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

    let _ = std::fs::remove_dir_all("./.qop/store");
    std::fs::create_dir_all("./.qop/store")?;

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
        let store_path = Path::new("./.qop/store").join(&path);
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
        for diffop in diff.ops() {
            let mut content = Vec::<String>::new();
            for change in diff.iter_changes(diffop) {
                match change.tag() {
                    | similar::ChangeTag::Delete => {
                        content.push(format!("-{}", change.to_string()));
                    },
                    | similar::ChangeTag::Insert => {
                        content.push(format!("+{}", change.to_string()));
                    },
                    | _ => {},
                }
            }

            match diffop.tag() {
                | similar::DiffTag::Equal => {},
                | similar::DiffTag::Delete => {
                    patch.files.entry(path.clone()).or_insert_with(BTreeMap::new).insert(
                        diffop.old_range().start.to_string(),
                        PatchSection {
                            content: content.concat(),
                        },
                    );
                },
                | similar::DiffTag::Insert => {
                    patch.files.entry(path.clone()).or_insert_with(BTreeMap::new).insert(
                        diffop.new_range().start.to_string(),
                        PatchSection {
                            content: content.concat(),
                        },
                    );
                },
                | similar::DiffTag::Replace => {
                    patch.files.entry(path.clone()).or_insert_with(BTreeMap::new).insert(
                        diffop.old_range().start.to_string(),
                        PatchSection {
                            content: content.concat(),
                        },
                    );
                },
            }
        }
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

    for patch_file in patch.files {
        let mut actions = Vec::<Action>::new();
        for section in patch_file.1 {
            let line = section.0.parse::<usize>()?;
            actions.push(section.1.to_action_at(line));
        }

        let file_content = std::fs::read_to_string(&patch_file.0)?;
        let mut file_content_iter = file_content.lines();
        let mut file_new = Vec::<String>::new();
        let mut line_index = 0_i32;
        let mut actions_iter = actions.iter();
        while let Some(act) = actions_iter.next() {
            while line_index < act.at as i32 {
                let x = file_content_iter.next().unwrap();
                file_new.push(x.to_owned());
                line_index += 1;
            }

            file_new.append(&mut act.insert.clone());
            for _ in 0..act.remove.len() {
                file_content_iter.next();
            }
            line_index += act.remove.len() as i32;
        }
        while let Some(v) = file_content_iter.next() {
            file_new.push(v.to_owned());
            line_index += 1;
        }
        if file_content.ends_with("\n") {
            file_new.push("".to_owned());
        }

        // println!("{}", file_new.join("\n"));
        std::fs::write(&patch_file.0, file_new.join("\n"))?;
    }

    Ok(())
}

async fn reverse(file: String) -> Result<()> {
    let patch = toml::from_str::<Patch>(&std::fs::read_to_string(file)?)?;

    let mut new_files = HashMap::<String, BTreeMap<String, PatchSection>>::new();
    for file in patch.files {
        let mut new_sections = BTreeMap::<String, PatchSection>::new();
        for section in file.1 {
            let mut s = section.1.clone();
            let mut content_reverse = Vec::<String>::new();
            let act = s.to_action_at(section.0.parse::<usize>()?);
            for l in &act.insert {
                content_reverse.push(format!("-{}", l));
            }
            for l in &act.remove {
                content_reverse.push(format!("+{}", l));
            }

            s.content = content_reverse.join("\n");
            new_sections.insert(act.at.to_string(), s);
        }
        new_files.insert(file.0, new_sections);
    }

    let new_patch = Patch { files: new_files };
    println!("{}", toml::to_string(&new_patch)?);

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
    pub files: HashMap<String, BTreeMap<String, PatchSection>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatchSection {
    // pub path: String,
    // pub line: usize,
    pub content: String,
}

impl PatchSection {
    pub fn to_action_at(&self, line: usize) -> Action {
        let mut remove_lines = Vec::<String>::new();
        let mut insert_lines = Vec::<String>::new();
        for l in self.content.lines() {
            if l.starts_with("-") {
                remove_lines.push(l[1..].to_string());
            } else if l.starts_with("+") {
                insert_lines.push(l[1..].to_string());
            }
        }

        Action {
            at: line,
            remove: remove_lines,
            insert: insert_lines,
        }
    }
}

pub struct Action {
    at: usize,
    remove: Vec<String>,
    insert: Vec<String>,
}
