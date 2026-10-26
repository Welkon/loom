//! Project install, remove, install, update, outdated, verify, and search.

use super::registry::{Registry, UnitFile};
use super::semver;
use super::store::{self, LockEntry};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type Failure = (String, u8);

struct Spec {
    id: String,
    version: String,
    kind: String,
    sha256: String,
    source: String,
    commit: String,
    files: Vec<UnitFile>,
    license: String,
    requires: Vec<String>,
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
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let spec = spec(&registry, id)?;
    if requested.is_some_and(|version| version != spec.version) {
        return Err(("loom_version_not_available".to_string(), 2));
    }
    let destination = destination(&root, &spec, options.dest.as_deref())?;
    let mut visited = HashSet::new();
    let mut installed = Vec::new();
    install_tree(
        &root,
        &registry,
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
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let manifest_path = root.join("loom.toml");
    let units = store::read_manifest(&manifest_path)?;
    let lock_path = root.join("loom.lock");
    let mut entries = store::read_lock(&lock_path)?;
    let mut installed = 0u64;
    let mut skipped = 0u64;
    for (id, version) in units {
        let spec = spec(&registry, &id)?;
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
        install_dependencies(&root, &registry, options.force, false)?
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
    let registry = Registry::discover(&root, options.registry.as_deref())?;
    let manifest_path = root.join("loom.toml");
    let units = store::read_manifest(&manifest_path)?;
    let lock_path = root.join("loom.lock");
    let entries = store::read_lock(&lock_path)?;
    let mut updated = Vec::new();
    for (unit_id, version) in &units {
        if id.as_ref().is_some_and(|wanted| wanted != unit_id) {
            continue;
        }
        let spec = spec(&registry, unit_id)?;
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
        install_dependencies(&root, &registry, options.force, false)?
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
    Ok(json!({
        "status": "ok",
        "operation": "loom_integrate",
        "check": false,
        "files": files.iter().map(|(path, _)| path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/")).collect::<Vec<_>>(),
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
    check: bool,
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
            check: false,
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
                "--cwd" | "--dest" | "--registry" => {
                    let value = raw
                        .get(index + 1)
                        .cloned()
                        .ok_or_else(|| ("missing_argument".to_string(), 2))?;
                    match raw[index].as_str() {
                        "--cwd" => options.cwd = Some(value),
                        "--dest" => options.dest = Some(value),
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

fn spec(registry: &Registry, id: &str) -> Result<Spec, Failure> {
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

fn install_tree(
    root: &Path,
    registry: &Registry,
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
        let dependency_spec = spec(registry, dependency)?;
        let dependency_destination = default_destination(root, &dependency_spec);
        install_tree(
            root,
            registry,
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
    registry: &Registry,
    force: bool,
    dry_run: bool,
) -> Result<Vec<Value>, Failure> {
    let entries = store::read_lock(&root.join("loom.lock"))?;
    let mut visited = HashSet::new();
    let mut report = Vec::new();
    for entry in entries {
        let unit = spec(registry, &entry.id)?;
        for dependency in &unit.requires {
            let dependency_spec = spec(registry, dependency)?;
            let destination = default_destination(root, &dependency_spec);
            install_tree(
                root,
                registry,
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
