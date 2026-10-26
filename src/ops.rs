//! Project install, remove, install, update, outdated, verify, and search.

use super::index::Index;
use super::registry::{Registry, UnitFile};
use super::semver;
use super::spec::Spec;
use super::store::{self, LockEntry};
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type Failure = (String, u8);

enum Source<'a> {
    Git(&'a Registry),
    Index(&'a Index),
}

impl Source<'_> {
    fn spec(&self, id: &str) -> Result<Spec, Failure> {
        match self {
            Self::Git(registry) => spec(registry, id),
            Self::Index(index) => index.spec(id),
        }
    }

    fn spec_at(&self, id: &str, version: &str) -> Result<Spec, Failure> {
        match self {
            Self::Git(registry) => spec_at(registry, id, version),
            Self::Index(index) => index.spec_at(id, version),
        }
    }

    fn resolve_dependency(&self, requirement: &str) -> Result<Spec, Failure> {
        match self {
            Self::Git(registry) => resolve_dependency(registry, requirement),
            Self::Index(index) => index.resolve_dependency(requirement),
        }
    }
}

pub(super) fn add(raw: &[String]) -> Result<Value, Failure> {
    let token = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ("missing_loom_unit".to_string(), 2))?;
    let (id, requested) = token
        .split_once('@')
        .map_or((token.as_str(), None), |(id, version)| (id, Some(version)));
    if id.is_empty() {
        return Err(("missing_loom_unit".to_string(), 2));
    }
    let options = Options::parse(raw, 1)?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = if options.index.is_none() {
        Some(Registry::discover(&root, options.registry.as_deref())?)
    } else {
        None
    };
    let index = options
        .index
        .as_deref()
        .map(|base| Index::open(base, options.token.as_deref()))
        .transpose()?;
    let source = match (&registry, &index) {
        (_, Some(index)) => Source::Index(index),
        (Some(registry), None) => Source::Git(registry),
        _ => return Err(("missing_loom_source".to_string(), 2)),
    };
    let spec = match requested {
        Some(version) => source.spec_at(id, version)?,
        None => source.spec(id)?,
    };
    let destination = destination(&root, &spec, options.dest.as_deref())?;
    let mut visited = HashSet::new();
    let mut installed = Vec::new();
    install_tree(
        &root,
        &source,
        &spec,
        &destination,
        options.force,
        options.dry_run,
        &mut visited,
        &mut installed,
    )?;
    if !options.dry_run {
        write_integration(&root)?;
    }
    let requested = installed.iter().find(|entry| entry["id"] == spec.id);
    let already = requested
        .and_then(|entry| entry["already_installed"].as_bool())
        .unwrap_or(false);
    Ok(json!({
        "status": "ok",
        "operation": "loom_add",
        "unit": spec.id,
        "version": spec.version,
        "destination": relative(&root, &destination)?,
        "manifest": "loom.toml",
        "lock": "loom.lock",
        "already_installed": already,
        "installed": installed,
        "dry_run": options.dry_run
    }))
}

pub(super) fn remove(raw: &[String]) -> Result<Value, Failure> {
    let id = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ("missing_loom_unit".to_string(), 2))?;
    let options = Options::parse(raw, 1)?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let manifest_path = root.join("loom.toml");
    let mut units = store::read_manifest(&manifest_path)?;
    if !units.contains_key(id) {
        return Err(("loom_not_installed".to_string(), 2));
    }
    let lock_path = root.join("loom.lock");
    let mut entries = store::read_lock(&lock_path)?;
    let entry = entries.iter().find(|entry| entry.id == *id).cloned();
    let mut pruned = false;
    if options.prune {
        let Some(entry) = &entry else {
            return Err(("loom_lock_entry_missing".to_string(), 1));
        };
        let spec = spec(&registry, id)?;
        let destination = root.join(&entry.path);
        if !options.force {
            let paths: Vec<String> = spec
                .files
                .iter()
                .map(|file| file.relative.clone())
                .collect();
            let hash = store::hash_installed(&destination, &paths);
            if hash.as_deref() != Some(spec.sha256.as_str()) {
                return Err(("loom_target_modified".to_string(), 2));
            }
        }
        if !options.dry_run {
            for file in &spec.files {
                let _ = fs::remove_file(destination.join(&file.relative));
            }
            let _ = fs::remove_file(destination.join("LICENSE"));
            let _ = fs::remove_file(destination.join("PROVENANCE.md"));
            remove_empty_dirs(&destination);
        }
        pruned = true;
    }
    if !options.dry_run {
        units.remove(id);
        store::write_manifest(&manifest_path, &units)?;
        entries.retain(|entry| entry.id != *id);
        store::write_lock(&lock_path, &entries)?;
        write_integration(&root)?;
    }
    Ok(json!({
        "status": "ok",
        "operation": "loom_remove",
        "unit": id,
        "pruned": pruned,
        "dry_run": options.dry_run
    }))
}

pub(super) fn install(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = if options.index.is_none() {
        Some(Registry::discover(&root, options.registry.as_deref())?)
    } else {
        None
    };
    let index = options
        .index
        .as_deref()
        .map(|base| Index::open(base, options.token.as_deref()))
        .transpose()?;
    let source = match (&registry, &index) {
        (_, Some(index)) => Source::Index(index),
        (Some(registry), None) => Source::Git(registry),
        _ => return Err(("missing_loom_source".to_string(), 2)),
    };
    let manifest_path = root.join("loom.toml");
    let units = store::read_manifest(&manifest_path)?;
    let lock_path = root.join("loom.lock");
    let mut entries = store::read_lock(&lock_path)?;
    let mut installed = 0u64;
    let mut skipped = 0u64;
    for (id, version) in units {
        let spec = source.spec(&id)?;
        if version != spec.version {
            return Err(("loom_version_not_available".to_string(), 2));
        }
        let entry = entries.iter().find(|entry| entry.id == id).cloned();
        if options.locked {
            let Some(entry) = &entry else {
                return Err(("loom_lock_mismatch".to_string(), 1));
            };
            if entry.version != version || entry.sha256 != spec.sha256 {
                return Err(("loom_lock_mismatch".to_string(), 1));
            }
        }
        let destination = entry
            .as_ref()
            .map(|entry| root.join(&entry.path))
            .unwrap_or_else(|| default_destination(&root, &spec));
        if install_unit(&root, &spec, &destination, options.force, options.dry_run)? {
            skipped += 1;
        } else {
            installed += 1;
        }
        if !options.dry_run {
            let relative = relative(&root, &destination)?;
            record(&root, &spec, &relative)?;
            entries = store::read_lock(&lock_path)?;
        }
    }
    let dependencies = if options.dry_run {
        Vec::new()
    } else {
        install_dependencies(&root, &source, options.force, false)?
    };
    if !options.dry_run {
        write_integration(&root)?;
    }
    Ok(json!({
        "status": "ok",
        "operation": "loom_install",
        "dependencies": dependencies,
        "installed": installed,
        "skipped": skipped,
        "dry_run": options.dry_run
    }))
}

pub(super) fn update(raw: &[String]) -> Result<Value, Failure> {
    let id = raw.first().filter(|value| !value.starts_with('-')).cloned();
    let options = Options::parse(raw, usize::from(id.is_some()))?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = if options.index.is_none() {
        Some(Registry::discover(&root, options.registry.as_deref())?)
    } else {
        None
    };
    let index = options
        .index
        .as_deref()
        .map(|base| Index::open(base, options.token.as_deref()))
        .transpose()?;
    let source = match (&registry, &index) {
        (_, Some(index)) => Source::Index(index),
        (Some(registry), None) => Source::Git(registry),
        _ => return Err(("missing_loom_source".to_string(), 2)),
    };
    let manifest_path = root.join("loom.toml");
    let units = store::read_manifest(&manifest_path)?;
    let lock_path = root.join("loom.lock");
    let entries = store::read_lock(&lock_path)?;
    let mut updated = Vec::new();
    for (unit_id, version) in &units {
        if id.as_ref().is_some_and(|wanted| wanted != unit_id) {
            continue;
        }
        let spec = source.spec(unit_id)?;
        let entry = entries.iter().find(|entry| entry.id == *unit_id).cloned();
        let current = entry
            .as_ref()
            .map_or(version.as_str(), |entry| entry.version.as_str());
        if !semver::is_newer(&spec.version, current) {
            continue;
        }
        let destination = entry
            .as_ref()
            .map(|entry| root.join(&entry.path))
            .unwrap_or_else(|| default_destination(&root, &spec));
        if !options.force {
            let paths: Vec<String> = spec
                .files
                .iter()
                .map(|file| file.relative.clone())
                .collect();
            let hash = store::hash_installed(&destination, &paths);
            let expected = entry
                .as_ref()
                .map(|entry| entry.sha256.as_str())
                .unwrap_or("");
            if hash.as_deref() != Some(expected) {
                return Err(("loom_target_exists".to_string(), 2));
            }
        }
        install_unit(&root, &spec, &destination, true, options.dry_run)?;
        let relative = relative(&root, &destination)?;
        if !options.dry_run {
            record(&root, &spec, &relative)?;
        }
        updated.push(json!({"id": spec.id, "from": current, "to": spec.version}));
    }
    let dependencies = if options.dry_run {
        Vec::new()
    } else {
        install_dependencies(&root, &source, options.force, false)?
    };
    if !options.dry_run {
        write_integration(&root)?;
    }
    if let Some(id) = &id
        && !units.contains_key(id)
    {
        return Err(("loom_not_installed".to_string(), 2));
    }
    Ok(json!({
        "status": "ok",
        "operation": "loom_update",
        "updated": updated,
        "dependencies": dependencies,
        "dry_run": options.dry_run
    }))
}

pub(super) fn outdated(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let units = store::read_manifest(&root.join("loom.toml"))?;
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let mut rows = Vec::new();
    for (id, version) in &units {
        let spec = spec(&registry, id)?;
        let current = entries
            .iter()
            .find(|entry| entry.id == *id)
            .map_or(version.as_str(), |entry| entry.version.as_str());
        rows.push(json!({
            "id": id,
            "current": current,
            "available": spec.version,
            "outdated": semver::is_newer(&spec.version, current)
        }));
    }
    Ok(json!({
        "status": "ok",
        "operation": "loom_outdated",
        "count": rows.len(),
        "units": rows
    }))
}

pub(super) fn verify(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let units = store::read_manifest(&root.join("loom.toml"))?;
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let mut rows = Vec::new();
    let mut failed = false;
    for (id, version) in &units {
        let spec = spec(&registry, id)?;
        let entry = entries.iter().find(|entry| entry.id == *id);
        let (status, detail) = match entry {
            None => ("missing-lock", "loom.lock has no entry".to_string()),
            Some(entry) if entry.version != *version => (
                "version-mismatch",
                format!("manifest {version} != lock {}", entry.version),
            ),
            Some(entry) => {
                let destination = root.join(&entry.path);
                let paths: Vec<String> = spec
                    .files
                    .iter()
                    .map(|file| file.relative.clone())
                    .collect();
                match store::hash_installed(&destination, &paths) {
                    None => ("missing", destination.display().to_string()),
                    Some(hash) if hash == entry.sha256 && hash == spec.sha256 => {
                        ("ok", entry.path.clone())
                    }
                    Some(_) => ("modified", entry.path.clone()),
                }
            }
        };
        if status != "ok" {
            failed = true;
        }
        rows.push(json!({"id": id, "status": status, "detail": detail}));
    }
    let value = json!({
        "status": if failed { "failed" } else { "ok" },
        "operation": "loom_verify",
        "units": rows
    });
    Ok(value)
}

pub(super) fn search(raw: &[String]) -> Result<Value, Failure> {
    let query = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ("missing_loom_query".to_string(), 2))?
        .to_lowercase();
    let options = Options::parse(raw, 1)?;
    let registry = Registry::discover(&options.cwd()?, options.registry.as_deref())?;
    let manifest = &registry.manifest;
    let units = manifest["units"]
        .as_array()
        .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
    let matches: Vec<Value> = units
        .iter()
        .filter(|unit| {
            let id = unit["id"].as_str().unwrap_or("").to_lowercase();
            let summary = unit["summary"].as_str().unwrap_or("").to_lowercase();
            let tags = unit["tags"].as_array().is_some_and(|tags| {
                tags.iter().any(|tag| {
                    tag.as_str()
                        .is_some_and(|tag| tag.to_lowercase().contains(&query))
                })
            });
            let applications = unit["applications"].as_array().is_some_and(|apps| {
                apps.iter().any(|app| {
                    app.as_str()
                        .is_some_and(|app| app.to_lowercase().contains(&query))
                })
            });
            id.contains(&query) || summary.contains(&query) || tags || applications
        })
        .map(|unit| {
            json!({
                "id": unit["id"],
                "kind": unit["kind"],
                "version": unit["version"],
                "summary": unit["summary"],
                "tags": unit["tags"],
                "applications": unit["applications"]
            })
        })
        .collect();
    Ok(json!({
        "status": "ok",
        "operation": "loom_search",
        "query": query,
        "count": matches.len(),
        "units": matches
    }))
}

pub(super) fn list_installed(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    require_project(&root)?;
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let units = store::read_manifest(&root.join("loom.toml"))?;
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let mut rows = Vec::new();
    for (id, version) in &units {
        let spec = spec(&registry, id)?;
        let entry = entries.iter().find(|entry| entry.id == *id);
        rows.push(json!({
            "id": id,
            "version": entry.map_or(version.as_str(), |entry| entry.version.as_str()),
            "available": spec.version,
            "outdated": semver::is_newer(&spec.version, entry.map_or(version.as_str(), |entry| entry.version.as_str())),
            "path": entry.map(|entry| entry.path.clone())
        }));
    }
    Ok(json!({
        "status": "ok",
        "operation": "loom_list",
        "installed": true,
        "count": rows.len(),
        "units": rows
    }))
}

pub(super) fn guard(raw: &[String]) -> Result<Value, Failure> {
    let options = GuardOptions::parse(raw)?;
    let root = root_for(&options.cwd()?);
    if !root.join("loom.toml").is_file() {
        return Ok(json!({
            "status": "skipped",
            "operation": "loom_guard",
            "reason": "no_loom_project"
        }));
    }
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let roots = options.guard_roots(&root);
    let candidates = if options.all {
        scan_paths(&root, &roots)
    } else if let Some(revision) = &options.diff {
        changed_paths(
            &root,
            &["diff", "--name-only", "--diff-filter=AM", revision],
        )?
    } else {
        changed_paths(
            &root,
            &["diff", "--cached", "--name-only", "--diff-filter=AM"],
        )?
    };
    let mut checked = 0u64;
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for relative in candidates {
        let path = root.join(&relative);
        if !path.is_file() || !is_guarded(&relative, &roots) || !is_code(&relative) {
            continue;
        }
        checked += 1;
        let installed = entries.iter().any(|entry| {
            relative == entry.path || relative.starts_with(&format!("{}/", entry.path))
        });
        if installed {
            rows.push(json!({"path": relative, "source": "loom"}));
            continue;
        }
        if has_custom_marker(&path) {
            rows.push(json!({"path": relative, "source": "custom"}));
            continue;
        }
        failures.push(relative.clone());
        rows.push(json!({"path": relative, "source": "unprovenanced"}));
    }
    let failed = !failures.is_empty();
    Ok(json!({
        "status": if failed { "failed" } else { "ok" },
        "operation": "loom_guard",
        "mode": if options.all { "all" } else if options.diff.is_some() { "diff" } else { "staged" },
        "checked": checked,
        "files": rows,
        "error": if failed {
            json!({"code": "loom_guard_unprovenanced", "files": failures})
        } else {
            Value::Null
        }
    }))
}

struct GuardOptions {
    cwd: Option<String>,
    all: bool,
    diff: Option<String>,
    paths: Vec<String>,
}

impl GuardOptions {
    fn parse(raw: &[String]) -> Result<Self, Failure> {
        let mut options = Self {
            cwd: None,
            all: false,
            diff: None,
            paths: Vec::new(),
        };
        let mut index = 0;
        while index < raw.len() {
            match raw[index].as_str() {
                "--json" | "--brief" | "--staged" => index += 1,
                "--all" => {
                    options.all = true;
                    index += 1;
                }
                "--cwd" | "--diff" | "--path" => {
                    let value = raw
                        .get(index + 1)
                        .cloned()
                        .ok_or_else(|| ("missing_argument".to_string(), 2))?;
                    match raw[index].as_str() {
                        "--cwd" => options.cwd = Some(value),
                        "--diff" => options.diff = Some(value),
                        "--path" => options.paths.push(value),
                        _ => {}
                    }
                    index += 2;
                }
                other => return Err((format!("unknown_argument:{other}"), 2)),
            }
        }
        if options.all && options.diff.is_some() {
            return Err(("loom_guard_mode_conflict".to_string(), 2));
        }
        Ok(options)
    }

    fn cwd(&self) -> Result<PathBuf, Failure> {
        self.cwd.as_ref().map_or_else(
            || std::env::current_dir().map_err(store::io_error),
            |value| Ok(PathBuf::from(value)),
        )
    }

    fn guard_roots(&self, root: &Path) -> Vec<String> {
        let defaults = [
            "firmware/drivers",
            "firmware/lib",
            "src/drivers",
            "src/lib",
            "drivers",
            "lib",
        ];
        let values: Vec<String> = if self.paths.is_empty() {
            defaults.iter().map(|value| (*value).to_string()).collect()
        } else {
            self.paths.clone()
        };
        values
            .into_iter()
            .filter(|value| root.join(value).is_dir())
            .collect()
    }
}

fn changed_paths(root: &Path, args: &[&str]) -> Result<Vec<String>, Failure> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| (format!("loom_guard_git_failed:{error}"), 1))?;
    if !output.status.success() {
        return Err(("loom_guard_git_failed".to_string(), 1));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect())
}

fn scan_paths(root: &Path, roots: &[String]) -> Vec<String> {
    fn walk(project: &Path, path: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                walk(project, &entry.path(), out);
            } else if let Ok(relative) = entry.path().strip_prefix(project) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut paths = Vec::new();
    for guard in roots {
        walk(root, &root.join(guard), &mut paths);
    }
    paths
}

pub(super) fn index(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    if options.static_index {
        let output = options
            .out
            .as_deref()
            .ok_or_else(|| ("missing_index_output".to_string(), 2))?;
        let output = store::safe_join(&root, output)?;
        return super::static_index::write(&registry, &output);
    }
    let units = registry.manifest["units"]
        .as_array()
        .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
    let mut entries: Vec<Value> = units
        .iter()
        .map(|unit| {
            let id = unit["id"].as_str().unwrap_or("");
            json!({
                "id": id,
                "kind": unit["kind"],
                "version": unit["version"],
                "path": unit["path"],
                "summary": unit["summary"],
                "tags": unit["tags"],
                "applications": unit["applications"],
                "compatibility": unit["compatibility"],
                "requires": unit["requires"].as_array().cloned().unwrap_or_default(),
                "versions": registry_versions(&registry, id),
                "sha256": registry.lock["units"]
                    .get(id)
                    .and_then(|entry| entry["sha256"].as_str())
                    .unwrap_or("")
            })
        })
        .collect();
    entries.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let index = json!({
        "schema": 1,
        "source": registry.lock["source"],
        "ref": registry.lock["ref"],
        "commit": registry.lock["commit"],
        "license": registry.manifest["license"],
        "units": entries
    });
    if let Some(relative) = &options.out {
        let output = store::safe_join(&root, relative)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(store::io_error)?;
        }
        fs::write(
            &output,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&index)
                    .map_err(|_| ("invalid_loom_index".to_string(), 1))?
            ),
        )
        .map_err(store::io_error)?;
        return Ok(json!({
            "status": "ok",
            "operation": "loom_index",
            "output": output.strip_prefix(&root).unwrap_or(&output).to_string_lossy().replace('\\', "/"),
            "count": index["units"].as_array().map_or(0, Vec::len)
        }));
    }
    Ok(json!({"status": "ok", "operation": "loom_index", "index": index}))
}

pub(super) fn integrate(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    let files = integration_files(&root)?;
    if options.check {
        let stale: Vec<String> = files
            .iter()
            .filter(|(path, content)| fs::read_to_string(path).ok().as_deref() != Some(content))
            .map(|(path, _)| {
                path.strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        let failed = !stale.is_empty();
        return Ok(json!({
            "status": if failed { "failed" } else { "ok" },
            "operation": "loom_integrate",
            "check": true,
            "files": stale,
            "error": if failed {
                json!({"code": "loom_integrate_stale", "files": stale})
            } else {
                Value::Null
            }
        }));
    }
    if !options.dry_run {
        write_files(&files)?;
    }
    let mut applied = Vec::new();
    let mut warnings = Vec::new();
    if options.apply && !options.dry_run {
        apply_integration(
            &root,
            options.target.as_deref(),
            &mut applied,
            &mut warnings,
        )?;
    }
    Ok(json!({
        "status": "ok",
        "operation": "loom_integrate",
        "check": false,
        "files": files.iter().map(|(path, _)| path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/")).collect::<Vec<_>>(),
        "applied": applied,
        "warnings": warnings,
        "dry_run": options.dry_run
    }))
}

fn write_integration(root: &Path) -> Result<(), Failure> {
    let files = integration_files(root)?;
    write_files(&files)
}

fn write_files(files: &[(PathBuf, String)]) -> Result<(), Failure> {
    for (path, content) in files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(store::io_error)?;
        }
        if fs::read_to_string(path).ok().as_deref() != Some(content) {
            fs::write(path, content).map_err(store::io_error)?;
        }
    }
    Ok(())
}

fn apply_integration(
    root: &Path,
    target: Option<&str>,
    applied: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<(), Failure> {
    let makefile = root.join("Makefile");
    if makefile.is_file() {
        let block = "# loom: begin\ninclude firmware/loom/loom.mk\n# loom: end";
        if upsert_block(&makefile, "# loom: begin", "# loom: end", block)? {
            applied.push("Makefile".to_string());
        }
    }
    let cmake = root.join("CMakeLists.txt");
    if cmake.is_file() {
        let body = if let Some(target) = target {
            format!(
                "# loom: begin\ninclude(${{CMAKE_CURRENT_SOURCE_DIR}}/firmware/loom/loom.cmake)\nloom_attach({target})\n# loom: end"
            )
        } else {
            "# loom: begin\ninclude(${CMAKE_CURRENT_SOURCE_DIR}/firmware/loom/loom.cmake)\n# loom: end".to_string()
        };
        if upsert_block(&cmake, "# loom: begin", "# loom: end", &body)? {
            applied.push("CMakeLists.txt".to_string());
        }
    }
    let build_rs = root.join("build.rs");
    if build_rs.is_file() {
        let block = "// loom: begin\ninclude!(\"firmware/loom/loom.cargo.rs\");\n// loom: end";
        if upsert_block(&build_rs, "// loom: begin", "// loom: end", block)? {
            applied.push("build.rs".to_string());
        }
        let text = fs::read_to_string(&build_rs).map_err(store::io_error)?;
        if !text.contains("fn main") {
            let mut next = text;
            next.push_str("\nfn main() { build(); }\n");
            fs::write(&build_rs, next).map_err(store::io_error)?;
            applied.push("build.rs:main".to_string());
        } else if !text.contains("build();") {
            warnings.push("build.rs has fn main but does not call build()".to_string());
        }
    }
    Ok(())
}

fn upsert_block(path: &Path, begin: &str, end: &str, block: &str) -> Result<bool, Failure> {
    let text = fs::read_to_string(path).map_err(store::io_error)?;
    let next = if let (Some(start), Some(stop)) = (text.find(begin), text.find(end)) {
        if stop < start {
            return Err(("invalid_loom_marker".to_string(), 2));
        }
        format!("{}{}{}", &text[..start], block, &text[stop + end.len()..])
    } else {
        format!("{}\n{}\n", text.trim_end(), block)
    };
    if next != text {
        fs::write(path, next).map_err(store::io_error)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn integration_files(root: &Path) -> Result<Vec<(PathBuf, String)>, Failure> {
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let mut sources = Vec::new();
    let mut includes = Vec::new();
    for entry in &entries {
        let destination = root.join(&entry.path);
        if !destination.is_dir() {
            continue;
        }
        collect_sources(&destination, &entry.path, &mut sources);
        includes.push(entry.path.clone());
    }
    sources.sort();
    sources.dedup();
    includes.sort();
    includes.dedup();
    let dir = root.join("firmware").join("loom");
    let mut mk = String::from("# Generated by loom. Do not edit.\n");
    let mut cmake = String::from("# Generated by loom. Do not edit.\n");
    for include in &includes {
        mk.push_str(&format!("LOOM_INCLUDE_DIRS += -I{include}\n"));
        cmake.push_str(&format!("list(APPEND LOOM_INCLUDE_DIRS \"{include}\")"));
        cmake.push('\n');
    }
    for source in &sources {
        mk.push_str(&format!("LOOM_SOURCES += {source}\n"));
        cmake.push_str(&format!("list(APPEND LOOM_SOURCES \"{source}\")"));
        cmake.push('\n');
    }
    let mut cargo = String::from("// Generated by loom. Include this file from build.rs.\n");
    cargo.push_str("pub const LOOM_INCLUDE_DIRS: &[&str] = &[\n");
    for include in &includes {
        cargo.push_str(&format!("    \"{include}\",\n"));
    }
    cargo.push_str("];\n\npub const LOOM_SOURCES: &[&str] = &[\n");
    for source in &sources {
        cargo.push_str(&format!("    \"{source}\",\n"));
    }
    cargo.push_str("];\n\npub fn build() {\n    let mut build = cc::Build::new();\n    for dir in LOOM_INCLUDE_DIRS { build.include(dir); }\n    for source in LOOM_SOURCES { build.file(source); }\n    if !LOOM_SOURCES.is_empty() { build.compile(\"loom_units\"); }\n}\n");
    let readme = format!(
        "# loom integration\n\nGenerated by `loom integrate`; do not edit.\n\n## Make\n\n```make\ninclude firmware/loom/loom.mk\n```\n\n## CMake\n\n```cmake\ninclude(firmware/loom/loom.cmake)\ntarget_sources(your_target PRIVATE ${{LOOM_SOURCES}})\ntarget_include_directories(your_target PRIVATE ${{LOOM_INCLUDE_DIRS}})\n```\n\n## Cargo\n\n```rust\n// build.rs\ninclude!(\"firmware/loom/loom.cargo.rs\");\nfn main() {{ build(); }}\n```\n\nAdd `cc` as a build dependency.\n\nInstalled units: {}\n",
        entries.len()
    );
    Ok(vec![
        (dir.join("loom.mk"), mk),
        (dir.join("loom.cmake"), cmake),
        (dir.join("loom.cargo.rs"), cargo),
        (dir.join("README.md"), readme),
    ])
}

fn collect_installed_files(directory: &Path, relative: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|entry| entry.file_name());
    for entry in items {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "build" || name == ".git" || name == "LICENSE" || name == "PROVENANCE.md" {
            continue;
        }
        let next = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        if kind.is_dir() {
            collect_installed_files(&entry.path(), &next, out);
        } else if kind.is_file() {
            out.push(next);
        }
    }
}

fn collect_sources(directory: &Path, relative: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|entry| entry.file_name());
    for entry in items {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            if name != "tests" && name != "build" && !name.starts_with('.') {
                collect_sources(&entry.path(), &format!("{relative}/{name}"), out);
            }
            continue;
        }
        if kind.is_file()
            && (name.ends_with(".c") || name.ends_with(".cc") || name.ends_with(".cpp"))
        {
            out.push(format!("{relative}/{name}"));
        }
    }
}

struct Options {
    cwd: Option<String>,
    dest: Option<String>,
    force: bool,
    dry_run: bool,
    prune: bool,
    locked: bool,
    registry: Option<String>,
    index: Option<String>,
    token: Option<String>,
    check: bool,
    out: Option<String>,
    apply: bool,
    target: Option<String>,
    static_index: bool,
}

impl Options {
    fn parse(raw: &[String], start: usize) -> Result<Self, Failure> {
        let mut options = Self {
            cwd: None,
            dest: None,
            force: false,
            dry_run: false,
            prune: false,
            locked: false,
            registry: None,
            index: None,
            token: None,
            check: false,
            out: None,
            apply: false,
            target: None,
            static_index: false,
        };
        let mut index = start;
        while index < raw.len() {
            match raw[index].as_str() {
                "--json" | "--brief" => index += 1,
                "--force" => {
                    options.force = true;
                    index += 1;
                }
                "--dry-run" => {
                    options.dry_run = true;
                    index += 1;
                }
                "--prune" => {
                    options.prune = true;
                    index += 1;
                }
                "--locked" => {
                    options.locked = true;
                    index += 1;
                }
                "--check" => {
                    options.check = true;
                    index += 1;
                }
                "--static" => {
                    options.static_index = true;
                    index += 1;
                }
                "--out" => {
                    options.out = Some(
                        raw.get(index + 1)
                            .cloned()
                            .ok_or_else(|| ("missing_argument".to_string(), 2))?,
                    );
                    index += 2;
                }
                "--apply" => {
                    options.apply = true;
                    index += 1;
                }
                "--target" => {
                    options.target = Some(
                        raw.get(index + 1)
                            .cloned()
                            .ok_or_else(|| ("missing_argument".to_string(), 2))?,
                    );
                    index += 2;
                }
                "--cwd" | "--dest" | "--registry" | "--index" | "--token" => {
                    let value = raw
                        .get(index + 1)
                        .cloned()
                        .ok_or_else(|| ("missing_argument".to_string(), 2))?;
                    match raw[index].as_str() {
                        "--cwd" => options.cwd = Some(value),
                        "--dest" => options.dest = Some(value),
                        "--index" => options.index = Some(value),
                        "--token" => options.token = Some(value),
                        _ => options.registry = Some(value),
                    }
                    index += 2;
                }
                other => return Err((format!("unknown_argument:{other}"), 2)),
            }
        }
        Ok(options)
    }

    fn cwd(&self) -> Result<PathBuf, Failure> {
        self.cwd.as_ref().map_or_else(
            || std::env::current_dir().map_err(store::io_error),
            |value| Ok(PathBuf::from(value)),
        )
    }
}

pub(super) fn spec(registry: &Registry, id: &str) -> Result<Spec, Failure> {
    let unit = registry
        .unit(id)
        .ok_or_else(|| ("unknown_loom_unit".to_string(), 2))?;
    let files = registry.unit_files(unit)?;
    let sha256 = registry.lock["units"]
        .get(id)
        .and_then(|entry| entry["sha256"].as_str())
        .unwrap_or("")
        .to_string();
    Ok(Spec {
        id: id.to_string(),
        version: unit["version"].as_str().unwrap_or("").to_string(),
        kind: unit["kind"].as_str().unwrap_or("operator").to_string(),
        sha256,
        source: registry.lock["source"]
            .as_str()
            .unwrap_or("emb-loom")
            .to_string(),
        commit: registry.lock["commit"].as_str().unwrap_or("").to_string(),
        files,
        license: registry.license(),
        requires: unit["requires"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[allow(clippy::too_many_arguments)]
fn registry_versions(registry: &Registry, id: &str) -> Vec<String> {
    let pattern = format!("unit/{id}/v*");
    let output = git_output(registry.root.as_path(), &["tag", "-l", &pattern]).unwrap_or_default();
    let mut versions: Vec<String> = String::from_utf8_lossy(&output)
        .lines()
        .filter_map(|tag| {
            tag.rsplit_once("/v")
                .map(|(_, version)| version.to_string())
        })
        .collect();
    if let Some(unit) = registry.unit(id)
        && let Some(version) = unit["version"].as_str()
    {
        versions.push(version.to_string());
    }
    versions.sort();
    versions.dedup();
    versions
}

fn resolve_dependency(registry: &Registry, requirement: &str) -> Result<Spec, Failure> {
    let (id, range) = requirement
        .split_once('@')
        .map_or((requirement, "*"), |(id, range)| (id, range));
    let mut best: Option<String> = None;
    for version in registry_versions(registry, id) {
        if !semver::satisfies(&version, range) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|current| semver::is_newer(&version, current))
        {
            best = Some(version);
        }
    }
    let version = best.ok_or_else(|| ("loom_dependency_unsatisfied".to_string(), 2))?;
    spec_at(registry, id, &version)
}

fn spec_at(registry: &Registry, id: &str, version: &str) -> Result<Spec, Failure> {
    let current = spec(registry, id)?;
    if current.version == version {
        return Ok(current);
    }
    let tag = format!("unit/{id}/v{version}");
    let manifest = git_show(registry.root.as_path(), &format!("{tag}:loom.json"))?;
    let manifest: Value =
        serde_json::from_slice(&manifest).map_err(|_| ("invalid_loom_manifest".to_string(), 1))?;
    let unit = manifest["units"]
        .as_array()
        .and_then(|units| units.iter().find(|unit| unit["id"].as_str() == Some(id)))
        .ok_or_else(|| ("loom_version_not_available".to_string(), 2))?;
    let path = unit["path"]
        .as_str()
        .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
    let tree = git_output(
        registry.root.as_path(),
        &["ls-tree", "-r", "--name-only", &tag, "--", path],
    )?;
    let mut files = Vec::new();
    for relative in String::from_utf8_lossy(&tree).lines() {
        let name = relative
            .strip_prefix(&format!("{path}/"))
            .unwrap_or(relative)
            .to_string();
        files.push(UnitFile {
            relative: name,
            bytes: git_show(registry.root.as_path(), &format!("{tag}:{relative}"))?,
        });
    }
    if files.is_empty() {
        return Err(("loom_version_not_available".to_string(), 2));
    }
    let commit = String::from_utf8_lossy(&git_output(
        registry.root.as_path(),
        &["rev-parse", &format!("{tag}^{{commit}}")],
    )?)
    .trim()
    .to_string();
    let license = git_show(registry.root.as_path(), &format!("{tag}:LICENSE")).unwrap_or_default();
    Ok(Spec {
        id: id.to_string(),
        version: version.to_string(),
        kind: unit["kind"].as_str().unwrap_or("operator").to_string(),
        sha256: hash_files(&files),
        source: current.source,
        commit,
        files,
        license: String::from_utf8_lossy(&license).into_owned(),
        requires: unit["requires"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>, Failure> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|_| ("loom_version_not_available".to_string(), 2))?;
    if !output.status.success() {
        return Err(("loom_version_not_available".to_string(), 2));
    }
    Ok(output.stdout)
}

fn git_show(root: &Path, object: &str) -> Result<Vec<u8>, Failure> {
    git_output(root, &["show", object])
}

fn hash_files(files: &[UnitFile]) -> String {
    let mut hasher = sha2::Sha256::new();
    for file in files {
        hasher.update(file.relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&file.bytes);
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

#[allow(clippy::too_many_arguments)]
fn install_tree(
    root: &Path,
    source: &Source<'_>,
    unit: &Spec,
    destination: &Path,
    force: bool,
    dry_run: bool,
    visited: &mut HashSet<String>,
    report: &mut Vec<Value>,
) -> Result<(), Failure> {
    if !visited.insert(unit.id.clone()) {
        return Ok(());
    }
    for dependency in &unit.requires {
        let dependency_spec = source.resolve_dependency(dependency)?;
        let dependency_destination = default_destination(root, &dependency_spec);
        install_tree(
            root,
            source,
            &dependency_spec,
            &dependency_destination,
            force,
            dry_run,
            visited,
            report,
        )?;
    }
    let already = install_unit(root, unit, destination, force, dry_run)?;
    let relative = relative(root, destination)?;
    if !dry_run {
        record(root, unit, &relative)?;
    }
    report.push(json!({
        "id": unit.id,
        "version": unit.version,
        "destination": relative,
        "already_installed": already
    }));
    Ok(())
}

fn install_dependencies(
    root: &Path,
    source: &Source<'_>,
    force: bool,
    dry_run: bool,
) -> Result<Vec<Value>, Failure> {
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let mut visited = HashSet::new();
    let mut report = Vec::new();
    for entry in entries {
        let unit = source.spec(&entry.id)?;
        for dependency in &unit.requires {
            let dependency_spec = source.resolve_dependency(dependency)?;
            let destination = default_destination(root, &dependency_spec);
            install_tree(
                root,
                source,
                &dependency_spec,
                &destination,
                force,
                dry_run,
                &mut visited,
                &mut report,
            )?;
        }
    }
    Ok(report)
}

fn install_unit(
    _root: &Path,
    spec: &Spec,
    destination: &Path,
    force: bool,
    dry_run: bool,
) -> Result<bool, Failure> {
    let mut conflict = false;
    let mut complete = true;
    if destination.exists() {
        for file in &spec.files {
            match fs::read(destination.join(&file.relative)) {
                Ok(bytes) if bytes == file.bytes => {}
                Ok(_) => {
                    conflict = true;
                    complete = false;
                }
                Err(_) => complete = false,
            }
        }
    } else {
        complete = false;
    }
    if conflict && !force {
        return Err(("loom_target_exists".to_string(), 2));
    }
    if !dry_run {
        fs::create_dir_all(destination).map_err(store::io_error)?;
        for file in &spec.files {
            let target = destination.join(&file.relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(store::io_error)?;
            }
            fs::write(target, &file.bytes).map_err(store::io_error)?;
        }
        fs::write(destination.join("LICENSE"), &spec.license).map_err(store::io_error)?;
        fs::write(
            destination.join("PROVENANCE.md"),
            store::provenance(
                &spec.id,
                &spec.version,
                &spec.source,
                &spec.commit,
                &spec.sha256,
            ),
        )
        .map_err(store::io_error)?;
    }
    Ok(complete && !conflict)
}

fn record(root: &Path, spec: &Spec, relative: &str) -> Result<(), Failure> {
    let manifest_path = root.join("loom.toml");
    let mut units = store::read_manifest(&manifest_path)?;
    units.insert(spec.id.clone(), spec.version.clone());
    store::write_manifest(&manifest_path, &units)?;
    let lock_path = root.join("loom.lock");
    let mut entries = store::read_lock(&lock_path)?;
    entries.retain(|entry| entry.id != spec.id);
    entries.push(LockEntry {
        id: spec.id.clone(),
        version: spec.version.clone(),
        sha256: spec.sha256.clone(),
        path: relative.to_string(),
        source: spec.source.clone(),
        commit: spec.commit.clone(),
    });
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    store::write_lock(&lock_path, &entries)
}

fn destination(root: &Path, spec: &Spec, requested: Option<&str>) -> Result<PathBuf, Failure> {
    requested.map_or_else(
        || Ok(default_destination(root, spec)),
        |value| store::safe_join(root, value),
    )
}

fn default_destination(root: &Path, spec: &Spec) -> PathBuf {
    root.join("firmware")
        .join(if spec.kind == "driver" {
            "drivers"
        } else {
            "lib"
        })
        .join(&spec.id)
}

fn relative(root: &Path, destination: &Path) -> Result<String, Failure> {
    destination
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| ("invalid_loom_destination".to_string(), 2))
}

fn root_for(cwd: &Path) -> PathBuf {
    let start = cwd.to_path_buf();
    let mut current = start.clone();
    loop {
        if current.join("loom.toml").is_file()
            || current.join(".git").exists()
            || current.join(".emb-agent").is_dir()
        {
            return current;
        }
        if !current.pop() {
            return start;
        }
    }
}

fn require_project(_root: &Path) -> Result<(), Failure> {
    Ok(())
}

fn is_code(relative: &str) -> bool {
    [".c", ".h", ".cc", ".cpp", ".hpp"]
        .iter()
        .any(|suffix| relative.ends_with(suffix))
}

fn is_guarded(relative: &str, roots: &[String]) -> bool {
    roots
        .iter()
        .any(|root| relative == root || relative.starts_with(&format!("{root}/")))
}

fn has_custom_marker(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let window = &bytes[..bytes.len().min(4096)];
    String::from_utf8_lossy(window)
        .to_lowercase()
        .contains("loom: custom")
}

fn remove_empty_dirs(path: &Path) {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                remove_empty_dirs(&entry.path());
            }
        }
    }
    let _ = fs::remove_dir(path);
}

pub(super) fn pack(raw: &[String]) -> Result<Value, Failure> {
    let source_arg = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ("missing_pack_source".to_string(), 2))?;
    let value = |name: &str| {
        let index = raw.iter().position(|item| item == name)?;
        raw.get(index + 1).cloned()
    };
    let id = value("--id").ok_or_else(|| ("missing_pack_id".to_string(), 2))?;
    let kind = value("--kind").ok_or_else(|| ("missing_pack_kind".to_string(), 2))?;
    let out = value("--out").ok_or_else(|| ("missing_pack_out".to_string(), 2))?;
    let version = value("--version").unwrap_or_else(|| "0.1.0".to_string());
    let summary = value("--summary").unwrap_or_else(|| format!("Packed {id}."));
    let tags = csv(value("--tags").as_deref());
    let applications = csv(value("--applications").as_deref());
    let requires = csv(value("--requires").as_deref());
    if !id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(("invalid_pack_id".to_string(), 2));
    }
    if kind != "driver" && kind != "operator" {
        return Err(("invalid_pack_kind".to_string(), 2));
    }
    let source = PathBuf::from(source_arg);
    if !source.is_dir() || !source.join("README.md").is_file() {
        return Err(("pack_source_invalid".to_string(), 2));
    }
    let mut files = Vec::new();
    collect_pack_files(&source, "", &mut files)?;
    if !files.iter().any(|file| file.relative.ends_with(".c"))
        || !files.iter().any(|file| file.relative.ends_with(".h"))
    {
        return Err(("pack_source_invalid".to_string(), 2));
    }
    let destination = PathBuf::from(&out);
    if destination.join("loom.json").exists() {
        return Err(("pack_output_exists".to_string(), 2));
    }
    let directory = format!(
        "{}s/{id}",
        if kind == "driver" {
            "driver"
        } else {
            "operator"
        }
    );
    for file in &files {
        let target = destination.join(&directory).join(&file.relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(store::io_error)?;
        }
        fs::write(target, &file.bytes).map_err(store::io_error)?;
    }
    let license = if source.join("LICENSE").is_file() {
        fs::read_to_string(source.join("LICENSE")).map_err(store::io_error)?
    } else if source.join("LICENSE.md").is_file() {
        fs::read_to_string(source.join("LICENSE.md")).map_err(store::io_error)?
    } else {
        return Err(("pack_license_missing".to_string(), 2));
    };
    fs::create_dir_all(&destination).map_err(store::io_error)?;
    fs::write(destination.join("LICENSE"), license).map_err(store::io_error)?;
    let sha256 = pack_hash(&files);
    let manifest = json!({
        "schema": 1,
        "name": id,
        "version": version,
        "license": "see LICENSE",
        "units": [{
            "id": id,
            "kind": kind,
            "version": version,
            "path": directory,
            "summary": summary,
            "tags": tags,
            "applications": applications,
            "requires": requires,
            "compatibility": {"language": "c99", "alloc": false, "float": false, "blocking": false}
        }]
    });
    let lock = json!({
        "schema": 1,
        "source": "loom pack",
        "ref": version,
        "commit": "",
        "units": {id.clone(): {"version": version, "sha256": sha256}}
    });
    fs::write(
        destination.join("loom.json"),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|_| ("invalid_pack_manifest".to_string(), 1))?,
    )
    .map_err(store::io_error)?;
    fs::write(
        destination.join("loom.lock"),
        serde_json::to_vec_pretty(&lock).map_err(|_| ("invalid_pack_lock".to_string(), 1))?,
    )
    .map_err(store::io_error)?;
    Ok(json!({
        "status": "ok",
        "operation": "loom_pack",
        "id": id,
        "version": version,
        "output": out,
        "files": files.len()
    }))
}

fn csv(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn collect_pack_files(
    directory: &Path,
    prefix: &str,
    out: &mut Vec<UnitFile>,
) -> Result<(), Failure> {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map_err(store::io_error)?
        .map(|entry| entry.map_err(store::io_error))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == "build" || name.starts_with('.') {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry.file_type().map_err(store::io_error)?;
        if kind.is_dir() {
            collect_pack_files(&entry.path(), &relative, out)?;
        } else if kind.is_file() {
            out.push(UnitFile {
                relative,
                bytes: fs::read(entry.path()).map_err(store::io_error)?,
            });
        }
    }
    Ok(())
}

fn pack_hash(files: &[UnitFile]) -> String {
    let mut hasher = sha2::Sha256::new();
    for file in files {
        hasher.update(file.relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&file.bytes);
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

pub(super) fn doctor(raw: &[String]) -> Result<Value, Failure> {
    let options = Options::parse(raw, 0)?;
    let root = root_for(&options.cwd()?);
    let lock = store::read_lock(&root.join("loom.lock"))?;
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    for entry in &lock {
        let destination = root.join(&entry.path);
        if !destination.is_dir() {
            failures.push(json!({"id": entry.id, "problem": "missing_path", "path": entry.path}));
            continue;
        }
        let mut paths = Vec::new();
        collect_installed_files(&destination, "", &mut paths);
        if paths.is_empty() {
            failures
                .push(json!({"id": entry.id, "problem": "missing_sources", "path": entry.path}));
            continue;
        }
        let hash = store::hash_installed(&destination, &paths).unwrap_or_default();
        if hash != entry.sha256 {
            failures.push(
                json!({"id": entry.id, "problem": "modified", "path": entry.path,
                                 "hash": hash, "expected": entry.sha256, "files": paths}),
            );
        }
    }
    for (path, content) in integration_files(&root)? {
        if fs::read_to_string(&path).ok().as_deref() != Some(&content) {
            failures.push(json!({
                "id": "integration",
                "problem": "stale",
                "path": path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace('\\', "/")
            }));
        }
    }
    for file in ["CMakeLists.txt", "Makefile", "build.rs"] {
        if root.join(file).is_file()
            && !fs::read_to_string(root.join(file))
                .unwrap_or_default()
                .contains("firmware/loom")
        {
            warnings.push(json!({"file": file, "problem": "include_missing"}));
        }
    }
    let failed = !failures.is_empty();
    Ok(json!({
        "status": if failed { "failed" } else { "ok" },
        "operation": "loom_doctor",
        "units": lock.len(),
        "failures": failures,
        "warnings": warnings,
        "error": if failed { json!({"code": "loom_doctor_failed"}) } else { Value::Null }
    }))
}

fn push_registry(root: &Path, message: &str, sign: bool) -> Result<(), Failure> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .map_err(|_| ("loom_registry_not_git".to_string(), 2))?;
    if !status.status.success() {
        return Err(("loom_registry_not_git".to_string(), 2));
    }
    if !status.stdout.is_empty() {
        let add = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["add", "-A"])
            .status()
            .map_err(|_| ("loom_publish_push_failed".to_string(), 1))?;
        if !add.success() {
            return Err(("loom_publish_push_failed".to_string(), 1));
        }
        let mut commit = Command::new("git");
        commit.arg("-C").arg(root).args(["commit", "-m", message]);
        if sign {
            commit.arg("-S");
        }
        if !commit
            .status()
            .map_err(|_| ("loom_publish_push_failed".to_string(), 1))?
            .success()
        {
            return Err(("loom_publish_push_failed".to_string(), 1));
        }
    }
    let push = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["push", "origin", "HEAD"])
        .status()
        .map_err(|_| ("loom_publish_push_failed".to_string(), 1))?;
    if !push.success() {
        return Err(("loom_publish_push_failed".to_string(), 1));
    }
    Ok(())
}

pub(super) fn publish(raw: &[String]) -> Result<Value, Failure> {
    let source_arg = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ("missing_publish_source".to_string(), 2))?;
    let value = |name: &str| {
        let index = raw.iter().position(|item| item == name)?;
        raw.get(index + 1).cloned()
    };
    let id = value("--id").ok_or_else(|| ("missing_publish_id".to_string(), 2))?;
    let kind = value("--kind").ok_or_else(|| ("missing_publish_kind".to_string(), 2))?;
    let registry =
        value("--registry").ok_or_else(|| ("missing_publish_registry".to_string(), 2))?;
    let version = value("--version").unwrap_or_else(|| "0.1.0".to_string());
    let summary = value("--summary").unwrap_or_else(|| format!("Packed {id}."));
    let tags = csv(value("--tags").as_deref());
    let applications = csv(value("--applications").as_deref());
    let requires = csv(value("--requires").as_deref());
    let force = raw.iter().any(|item| item == "--force");
    let push = raw.iter().any(|item| item == "--push");
    let sign = raw.iter().any(|item| item == "--sign");
    if kind != "driver" && kind != "operator" {
        return Err(("invalid_publish_kind".to_string(), 2));
    }
    let source = PathBuf::from(source_arg);
    if !source.is_dir() || !source.join("README.md").is_file() {
        return Err(("publish_source_invalid".to_string(), 2));
    }
    let mut files = Vec::new();
    collect_pack_files(&source, "", &mut files)?;
    if !files.iter().any(|file| file.relative.ends_with(".c"))
        || !files.iter().any(|file| file.relative.ends_with(".h"))
    {
        return Err(("publish_source_invalid".to_string(), 2));
    }
    let output = PathBuf::from(&registry);
    fs::create_dir_all(&output).map_err(store::io_error)?;
    let manifest_path = output.join("loom.json");
    let mut manifest = if manifest_path.is_file() {
        serde_json::from_slice::<Value>(&fs::read(&manifest_path).map_err(store::io_error)?)
            .map_err(|_| ("invalid_publish_manifest".to_string(), 1))?
    } else {
        json!({"schema":1,"name":"loom-registry","version":version,"license":"see LICENSE","units":[]})
    };
    let units = manifest["units"]
        .as_array_mut()
        .ok_or_else(|| ("invalid_publish_manifest".to_string(), 1))?;
    if units
        .iter()
        .any(|unit| unit["id"].as_str() == Some(id.as_str()))
        && !force
    {
        return Err(("loom_unit_already_published".to_string(), 2));
    }
    let directory = format!(
        "{}s/{id}",
        if kind == "driver" {
            "driver"
        } else {
            "operator"
        }
    );
    let target = output.join(&directory);
    if target.exists() {
        fs::remove_dir_all(&target).map_err(store::io_error)?;
    }
    for file in &files {
        let path = target.join(&file.relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(store::io_error)?;
        }
        fs::write(path, &file.bytes).map_err(store::io_error)?;
    }
    if !output.join("LICENSE").is_file() {
        let license = ["LICENSE", "LICENSE.md"]
            .iter()
            .find_map(|name| {
                let path = source.join(name);
                path.is_file()
                    .then(|| fs::read_to_string(path).ok())
                    .flatten()
            })
            .ok_or_else(|| ("publish_license_missing".to_string(), 2))?;
        fs::write(output.join("LICENSE"), license).map_err(store::io_error)?;
    }
    units.retain(|unit| unit["id"] != id);
    units.push(json!({
        "id": id,
        "kind": kind,
        "version": version,
        "path": directory,
        "summary": summary,
        "tags": tags,
        "applications": applications,
        "requires": requires,
        "compatibility": {"language":"c99","alloc":false,"float":false,"blocking":false}
    }));
    units.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|_| ("invalid_publish_manifest".to_string(), 1))?,
    )
    .map_err(store::io_error)?;
    let lock_path = output.join("loom.lock");
    let mut lock = if lock_path.is_file() {
        serde_json::from_slice::<Value>(&fs::read(&lock_path).map_err(store::io_error)?)
            .map_err(|_| ("invalid_publish_lock".to_string(), 1))?
    } else {
        json!({"schema":1,"source":"loom publish","ref":version,"commit":"","units":{}})
    };
    lock["units"][&id] = json!({"version":version,"sha256":pack_hash(&files)});
    fs::write(
        &lock_path,
        serde_json::to_vec_pretty(&lock).map_err(|_| ("invalid_publish_lock".to_string(), 1))?,
    )
    .map_err(store::io_error)?;
    if push {
        push_registry(&output, &format!("publish {id} {version}"), sign)?;
    }
    Ok(
        json!({"status":"ok","operation":"loom_publish","id":id,"version":version,"registry":registry,"pushed":push}),
    )
}

pub(super) fn yank(raw: &[String]) -> Result<Value, Failure> {
    let id = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ("missing_yank_id".to_string(), 2))?;
    let value = |name: &str| {
        let index = raw.iter().position(|item| item == name)?;
        raw.get(index + 1).cloned()
    };
    let registry = value("--registry").ok_or_else(|| ("missing_yank_registry".to_string(), 2))?;
    let push = raw.iter().any(|item| item == "--push");
    let sign = raw.iter().any(|item| item == "--sign");
    let output = PathBuf::from(&registry);
    let manifest_path = output.join("loom.json");
    let mut manifest =
        serde_json::from_slice::<Value>(&fs::read(&manifest_path).map_err(store::io_error)?)
            .map_err(|_| ("invalid_yank_manifest".to_string(), 1))?;
    let units = manifest["units"]
        .as_array_mut()
        .ok_or_else(|| ("invalid_yank_manifest".to_string(), 1))?;
    let Some(index) = units
        .iter()
        .position(|unit| unit["id"].as_str() == Some(id.as_str()))
    else {
        return Err(("loom_unit_not_published".to_string(), 2));
    };
    let unit = units.remove(index);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|_| ("invalid_yank_manifest".to_string(), 1))?,
    )
    .map_err(store::io_error)?;
    let lock_path = output.join("loom.lock");
    if lock_path.is_file() {
        let mut lock =
            serde_json::from_slice::<Value>(&fs::read(&lock_path).map_err(store::io_error)?)
                .map_err(|_| ("invalid_yank_lock".to_string(), 1))?;
        if let Some(map) = lock["units"].as_object_mut() {
            map.remove(id);
        }
        fs::write(
            lock_path,
            serde_json::to_vec_pretty(&lock).map_err(|_| ("invalid_yank_lock".to_string(), 1))?,
        )
        .map_err(store::io_error)?;
    }
    if raw.iter().any(|item| item == "--prune")
        && let Some(path) = unit["path"].as_str()
    {
        let _ = fs::remove_dir_all(output.join(path));
    }
    if push {
        push_registry(&output, &format!("yank {id}"), sign)?;
    }
    Ok(
        json!({"status":"ok","operation":"loom_yank","id":id,"pruned":raw.iter().any(|item| item == "--prune"),"pushed":push}),
    )
}
