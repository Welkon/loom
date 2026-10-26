//! Runtime registry loader. The C units stay on disk, not in the binary.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
            candidates.push(registry_path(directory)?);
        }
        if let Some(directory) = crate::registry_override() {
            candidates.insert(0, directory);
        }
        if let Ok(directory) = std::env::var("EMB_LOOM_REGISTRY")
            && !directory.is_empty()
        {
            candidates.push(registry_path(&directory)?);
        }
        if let Ok(executable) = std::env::current_exe()
            && let Some(bin) = executable.parent()
            && let Some(package) = bin.parent()
        {
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

fn registry_path(value: &str) -> Result<PathBuf, (String, u8)> {
    if !is_remote(value) {
        return Ok(PathBuf::from(value));
    }
    let cache = cache_root().join(hex(Sha256::digest(value.as_bytes())));
    if cache.join("loom.json").is_file()
        && std::env::var("LOOM_REGISTRY_REFRESH").as_deref() == Ok("1")
    {
        let _ = fs::remove_dir_all(&cache);
    }
    if cache.join("loom.json").is_file() {
        return Ok(cache);
    }
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let output = Command::new("git")
        .args(["clone", "--depth", "1", "--quiet", value])
        .arg(&cache)
        .output()
        .map_err(|_| ("loom_registry_fetch_failed".to_string(), 1))?;
    if !output.status.success() || !cache.join("loom.json").is_file() {
        return Err(("loom_registry_fetch_failed".to_string(), 1));
    }
    Ok(cache)
}

fn is_remote(value: &str) -> bool {
    value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("ssh://")
        || value.starts_with("git@")
        || value.starts_with("file://")
}

fn cache_root() -> PathBuf {
    if let Ok(value) = std::env::var("XDG_CACHE_HOME")
        && !value.is_empty()
    {
        return PathBuf::from(value).join("loom/registries");
    }
    if let Ok(value) = std::env::var("HOME")
        && !value.is_empty()
    {
        return PathBuf::from(value).join(".cache/loom/registries");
    }
    std::env::temp_dir().join("loom/registries")
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn io_error(error: std::io::Error) -> (String, u8) {
    (format!("loom_io_failed:{error}"), 1)
}
