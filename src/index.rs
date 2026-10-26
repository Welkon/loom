//! Cargo-style static registry: one index file per unit and one tarball per version.

use super::registry::UnitFile;
use super::semver;
use super::spec::Spec;
use flate2::read::GzDecoder;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Component, PathBuf};
use std::process::Command;
use tar::Archive;

type Failure = (String, u8);

pub(super) struct Index {
    base: String,
    local: Option<PathBuf>,
    cache: PathBuf,
    config: Value,
    token: Option<String>,
}

impl Index {
    pub fn open(base: &str, token: Option<&str>) -> Result<Self, Failure> {
        let base = base.trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(("missing_index".to_string(), 2));
        }
        let local = base
            .strip_prefix("file://")
            .map(PathBuf::from)
            .or_else(|| (!base.contains("://")).then(|| PathBuf::from(&base)));
        let cache = cache_root();
        let token = token
            .map(str::to_string)
            .or_else(|| std::env::var("LOOM_TOKEN").ok())
            .filter(|value| !value.is_empty());
        if token
            .as_ref()
            .is_some_and(|value| value.contains(['\n', '\r']))
        {
            return Err(("invalid_loom_token".to_string(), 2));
        }
        let mut index = Self {
            base,
            local,
            cache,
            config: Value::Null,
            token,
        };
        index.config = serde_json::from_slice(&index.read("config.json")?).unwrap_or(Value::Null);
        if index.config["auth-required"] == true && index.token.is_none() {
            return Err(("loom_auth_required".to_string(), 2));
        }
        Ok(index)
    }

    pub fn spec(&self, id: &str) -> Result<Spec, Failure> {
        let entry = self.select(id, None)?;
        self.build(id, &entry)
    }

    pub fn spec_at(&self, id: &str, version: &str) -> Result<Spec, Failure> {
        let entry = self.select(id, Some(version))?;
        self.build(id, &entry)
    }

    pub fn resolve_dependency(&self, requirement: &str) -> Result<Spec, Failure> {
        let (id, range) = requirement
            .split_once('@')
            .map_or((requirement, "*"), |(id, range)| (id, range));
        let entries = self.entries(id)?;
        let mut best: Option<&Value> = None;
        for entry in entries.iter().filter(|entry| entry["yanked"] != true) {
            let Some(version) = entry["version"].as_str() else {
                continue;
            };
            if !semver::satisfies(version, range) {
                continue;
            }
            if best.is_none_or(|current| {
                semver::is_newer(version, current["version"].as_str().unwrap_or(""))
            }) {
                best = Some(entry);
            }
        }
        let entry = best.ok_or_else(|| ("loom_dependency_unsatisfied".to_string(), 2))?;
        self.build(id, entry)
    }

    fn select(&self, id: &str, requested: Option<&str>) -> Result<Value, Failure> {
        let entries = self.entries(id)?;
        if let Some(version) = requested {
            let entry = entries
                .iter()
                .find(|entry| entry["version"].as_str() == Some(version))
                .ok_or_else(|| ("loom_version_not_available".to_string(), 2))?;
            if entry["yanked"] == true {
                return Err(("loom_version_yanked".to_string(), 2));
            }
            return Ok(entry.clone());
        }
        let mut best: Option<&Value> = None;
        for entry in entries.iter().filter(|entry| entry["yanked"] != true) {
            let Some(version) = entry["version"].as_str() else {
                continue;
            };
            if best.is_none_or(|current| {
                semver::is_newer(version, current["version"].as_str().unwrap_or(""))
            }) {
                best = Some(entry);
            }
        }
        best.cloned()
            .ok_or_else(|| ("unknown_loom_unit".to_string(), 2))
    }

    fn entries(&self, id: &str) -> Result<Vec<Value>, Failure> {
        if !safe_component(id) {
            return Err(("invalid_loom_unit".to_string(), 2));
        }
        let text = self.read(&index_path(id))?;
        let mut entries = Vec::new();
        for line in String::from_utf8_lossy(&text).lines() {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value =
                serde_json::from_str(line).map_err(|_| ("invalid_loom_index".to_string(), 1))?;
            if value["id"].as_str() == Some(id) {
                entries.push(value);
            }
        }
        if entries.is_empty() {
            return Err(("unknown_loom_unit".to_string(), 2));
        }
        Ok(entries)
    }

    fn build(&self, id: &str, entry: &Value) -> Result<Spec, Failure> {
        let version = entry["version"]
            .as_str()
            .ok_or_else(|| ("invalid_loom_index".to_string(), 1))?;
        if !safe_component(version) {
            return Err(("invalid_loom_index".to_string(), 1));
        }
        let sha256 = entry["sha256"]
            .as_str()
            .ok_or_else(|| ("invalid_loom_index".to_string(), 1))?;
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(("invalid_loom_index".to_string(), 1));
        }
        let url = self.tarball_url(id, version);
        let tarball = self.tarball(&url, sha256)?;
        let (files, license) = unpack(id, version, &tarball)?;
        let requires = entry["requires"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Spec {
            id: id.to_string(),
            version: version.to_string(),
            kind: entry["kind"].as_str().unwrap_or("operator").to_string(),
            sha256: hash_files(&files),
            source: self.base.clone(),
            commit: entry["commit"]
                .as_str()
                .or_else(|| self.config["commit"].as_str())
                .unwrap_or("")
                .to_string(),
            files,
            license,
            requires,
        })
    }

    fn tarball_url(&self, id: &str, version: &str) -> String {
        let template = self.config["dl"]
            .as_str()
            .unwrap_or("tarballs/{id}-{version}.tar.gz");
        let path = template
            .replace("{id}", id)
            .replace("{crate}", id)
            .replace("{version}", version);
        if path.contains("://") {
            return path;
        }
        if let Some(local) = &self.local {
            return local.join(path).to_string_lossy().into_owned();
        }
        if path.starts_with('/') {
            let origin = self
                .base
                .find("://")
                .and_then(|at| self.base[at + 3..].find('/').map(|slash| at + 3 + slash))
                .map_or(self.base.as_str(), |end| &self.base[..end]);
            return format!("{origin}{path}");
        }
        format!("{}/{path}", self.base)
    }

    fn tarball(&self, url: &str, expected: &str) -> Result<Vec<u8>, Failure> {
        let directory = self.cache.join("tarballs");
        fs::create_dir_all(&directory).map_err(io_error)?;
        let path = directory.join(format!("{expected}.tar.gz"));
        if let Ok(bytes) = fs::read(&path) {
            if digest(&bytes) == expected {
                return Ok(bytes);
            }
            let _ = fs::remove_file(&path);
        }
        let bytes = self.fetch(url, "loom_tarball_unavailable")?;
        if digest(&bytes) != expected {
            return Err(("loom_tarball_checksum".to_string(), 1));
        }
        fs::write(&path, &bytes).map_err(io_error)?;
        Ok(bytes)
    }

    fn read(&self, relative: &str) -> Result<Vec<u8>, Failure> {
        if let Some(local) = &self.local {
            return fs::read(local.join(relative)).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ("loom_index_not_found".to_string(), 2)
                } else {
                    io_error(error)
                }
            });
        }
        self.fetch(
            &format!("{}/{}", self.base, relative.trim_start_matches('/')),
            "loom_index_unavailable",
        )
    }

    fn fetch(&self, url: &str, code: &str) -> Result<Vec<u8>, Failure> {
        if !url.contains("://") {
            return fs::read(url).map_err(|_| (code.to_string(), 1));
        }
        let mut command = Command::new("curl");
        command.args(["-fsSL", "--max-time", "60", "--retry", "2"]);
        if let Some(token) = &self.token {
            command
                .arg("-H")
                .arg(format!("Authorization: Bearer {token}"));
        }
        let output = command
            .arg(url)
            .output()
            .map_err(|_| ("loom_http_client_missing".to_string(), 1))?;
        if !output.status.success() {
            return Err((code.to_string(), 1));
        }
        Ok(output.stdout)
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\'])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn index_path(id: &str) -> String {
    match id.len() {
        0 | 1 => format!("1/{id}"),
        2 => format!("2/{id}"),
        3 => format!("3/{}/{}", &id[..1], id),
        _ => format!("{}/{}/{}", &id[..2], &id[2..4], id),
    }
}

fn unpack(id: &str, version: &str, bytes: &[u8]) -> Result<(Vec<UnitFile>, String), Failure> {
    let root = format!("{id}-{version}");
    let mut archive = Archive::new(GzDecoder::new(bytes));
    let mut files = Vec::new();
    let mut license = String::new();
    for entry in archive.entries().map_err(io_error)? {
        let mut entry = entry.map_err(io_error)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err(("loom_tarball_unsafe".to_string(), 1));
        }
        let path = entry.path().map_err(io_error)?.into_owned();
        let mut components = path.components();
        if components.next() != Some(Component::Normal(root.as_ref())) {
            return Err(("loom_tarball_unsafe".to_string(), 1));
        }
        let rest: PathBuf = components.collect();
        if rest.as_os_str().is_empty() {
            continue;
        }
        if rest.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(("loom_tarball_unsafe".to_string(), 1));
        }
        let relative = rest.to_string_lossy().replace('\\', "/");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(io_error)?;
        if relative == "LICENSE" {
            license = String::from_utf8_lossy(&data).into_owned();
        } else {
            files.push(UnitFile {
                relative,
                bytes: data,
            });
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    if files.is_empty() {
        return Err(("loom_tarball_empty".to_string(), 1));
    }
    Ok((files, license))
}

fn hash_files(files: &[UnitFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&file.bytes);
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn cache_root() -> PathBuf {
    if let Ok(path) = std::env::var("LOOM_CACHE_DIR") {
        return PathBuf::from(path);
    }
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|root| root.join("loom"))
        .unwrap_or_else(|_| PathBuf::from(".loom-cache"))
}

fn io_error(error: std::io::Error) -> Failure {
    (format!("loom_io_failed:{error}"), 1)
}

#[cfg(test)]
mod tests {
    use super::index_path;

    #[test]
    fn uses_cargo_style_prefixes() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("median"), "me/di/median");
    }
}
