//! Runtime registry loader. The C units stay on disk, not in the binary.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct UnitFile {
    pub relative: String,
    pub bytes: Vec<u8>,
}

pub(super) struct Registry {
    pub root: PathBuf,
    pub manifest: Value,
    pub lock: Value,
}

impl Registry {
    pub fn discover(cwd: &Path, explicit: Option<&str>) -> Result<Self, (String, u8)> {
        let mut candidates = Vec::new();
        if let Some(directory) = explicit {
            candidates.push(PathBuf::from(directory));
        }
        if let Ok(directory) = std::env::var("EMB_LOOM_REGISTRY")
            && !directory.is_empty()
        {
            candidates.push(PathBuf::from(directory));
        }
        if let Ok(executable) = std::env::current_exe()
            && let Some(bin) = executable.parent()
            && let Some(package) = bin.parent()
        {
            candidates.push(package.join("crates/emb-loom/registry"));
            candidates.push(package.join("runtime/loom"));
        }
        candidates.push(cwd.join(".emb-agent/runtime/loom"));
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("registry"));
        for root in candidates {
            if root.join("loom.json").is_file() {
                return Self::load(root);
            }
        }
        Err(("loom_registry_missing".to_string(), 1))
    }

    fn load(root: PathBuf) -> Result<Self, (String, u8)> {
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(root.join("loom.json")).map_err(io_error)?)
                .map_err(|_| ("invalid_loom_manifest".to_string(), 1))?;
        let lock: Value =
            serde_json::from_str(&fs::read_to_string(root.join("loom.lock")).map_err(io_error)?)
                .map_err(|_| ("invalid_loom_lock".to_string(), 1))?;
        if !manifest["units"].is_array() {
            return Err(("invalid_loom_manifest".to_string(), 1));
        }
        Ok(Self {
            root,
            manifest,
            lock,
        })
    }

    pub fn unit(&self, id: &str) -> Option<&Value> {
        self.manifest["units"]
            .as_array()
            .and_then(|units| units.iter().find(|unit| unit["id"].as_str() == Some(id)))
    }

    pub fn unit_files(&self, unit: &Value) -> Result<Vec<UnitFile>, (String, u8)> {
        let path = unit["path"]
            .as_str()
            .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
        let directory = self.root.join(path);
        let mut files = Vec::new();
        collect(&directory, &directory, &mut files)?;
        if files.is_empty() {
            return Err(("loom_unit_files_missing".to_string(), 1));
        }
        Ok(files)
    }

    pub fn readme(&self, id: &str) -> Option<String> {
        let unit = self.unit(id)?;
        let path = unit["path"].as_str()?;
        fs::read_to_string(self.root.join(path).join("README.md")).ok()
    }

    pub fn license(&self) -> String {
        fs::read_to_string(self.root.join("LICENSE")).unwrap_or_default()
    }
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<UnitFile>) -> Result<(), (String, u8)> {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map_err(io_error)?
        .map(|entry| entry.map_err(io_error))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".git" || name == "build" {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type().map_err(io_error)?;
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ("invalid_loom_manifest".to_string(), 1))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(UnitFile {
                relative,
                bytes: fs::read(&path).map_err(io_error)?,
            });
        }
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> (String, u8) {
    (format!("loom_io_failed:{error}"), 1)
}
