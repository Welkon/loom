//! `loom` registry view and project package commands.

mod index;
mod ops;
mod registry;
mod semver;
mod spec;
mod store;

use registry::Registry;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static REGISTRY_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

pub(crate) fn registry_override() -> Option<PathBuf> {
    REGISTRY_OVERRIDE
        .get()
        .and_then(|slot| slot.lock().ok().and_then(|value| value.clone()))
}

/// Run a command with an explicit registry root, restoring the previous root.
pub fn run_with_registry(args: &[String], registry: Option<&Path>) -> (String, u8) {
    let slot = REGISTRY_OVERRIDE.get_or_init(|| Mutex::new(None));
    let previous = slot
        .lock()
        .ok()
        .map(|mut value| std::mem::replace(&mut *value, registry.map(PathBuf::from)));
    let result = run(args);
    if let Some(previous) = previous
        && let Ok(mut value) = slot.lock()
    {
        *value = previous;
    }
    result
}

/// Claim the loom namespace.
pub fn handles(args: &[String]) -> bool {
    args.first().is_some_and(|value| value == "loom")
}

/// Route list/show/add/remove/install/update/outdated/verify/search.
pub fn run(args: &[String]) -> (String, u8) {
    let action = args.get(1).map(String::as_str).unwrap_or("list");
    let result = match action {
        "list" => list(&args[2..]),
        "show" | "info" => show_command(&args[2..]),
        "add" => ops::add(&args[2..]),
        "remove" => ops::remove(&args[2..]),
        "install" => ops::install(&args[2..]),
        "update" => ops::update(&args[2..]),
        "outdated" => ops::outdated(&args[2..]),
        "verify" => ops::verify(&args[2..]),
        "search" => ops::search(&args[2..]),
        "guard" => ops::guard(&args[2..]),
        "integrate" => ops::integrate(&args[2..]),
        "index" => ops::index(&args[2..]),
        "pack" => ops::pack(&args[2..]),
        "doctor" => ops::doctor(&args[2..]),
        "publish" => ops::publish(&args[2..]),
        "yank" => ops::yank(&args[2..]),
        "help" | "--help" | "-h" => Ok(json!({
            "status": "ok",
            "operation": "loom_help",
            "commands": [
                "loom list [--installed] [--kind KIND] [--tag TAG]",
                "loom show ID [--readme]",
                "loom search QUERY",
                "loom add ID[@VERSION] [--index URL|DIR] [--dest DIR] [--force] [--dry-run]",
                "loom remove ID [--prune] [--force] [--dry-run]",
                "loom install [--locked] [--force] [--dry-run]",
                "loom update [ID] [--force] [--dry-run]",
                "loom outdated",
                "loom verify",
                "loom guard [--staged|--all|--diff REV] [--path DIR]",
                "loom integrate [--check|--apply] [--target NAME] [--cwd DIR]",
                "loom index [--out FILE] [--registry DIR|URL] [--cwd DIR]",
                "loom pack DIR --id ID --kind driver|operator --out DIR",
                "loom doctor [--cwd DIR]",
                "loom publish DIR --id ID --kind driver|operator --registry DIR",
                "loom yank ID --registry DIR [--prune]"
            ]
        })),
        _ => Err(("unknown_loom_command".to_string(), 2)),
    };
    match result {
        Ok(value) => {
            let exit = u8::from(value["status"] == "failed");
            (value.to_string(), exit)
        }
        Err((code, exit)) => (
            json!({
                "command_error_version": 1,
                "status": "failed",
                "operation": format!("loom_{action}"),
                "error": {"code": code}
            })
            .to_string(),
            exit,
        ),
    }
}

fn list(raw: &[String]) -> Result<Value, (String, u8)> {
    if raw.iter().any(|value| value == "--installed") {
        let filtered: Vec<String> = raw
            .iter()
            .filter(|value| value.as_str() != "--installed")
            .cloned()
            .collect();
        return ops::list_installed(&filtered);
    }
    let mut kind = None;
    let mut tag = None;
    let mut registry_arg = None;
    let mut cwd = None;
    let mut index = 0;
    while index < raw.len() {
        match raw[index].as_str() {
            "--json" | "--brief" => index += 1,
            "--cwd" | "--registry" => {
                let value = raw
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| ("missing_argument".to_string(), 2))?;
                if raw[index] == "--cwd" {
                    cwd = Some(value);
                } else {
                    registry_arg = Some(value);
                }
                index += 2;
            }
            "--kind" => {
                kind = Some(
                    raw.get(index + 1)
                        .cloned()
                        .ok_or_else(|| ("missing_argument".to_string(), 2))?,
                );
                index += 2;
            }
            "--tag" => {
                tag = Some(
                    raw.get(index + 1)
                        .cloned()
                        .ok_or_else(|| ("missing_argument".to_string(), 2))?,
                );
                index += 2;
            }
            other => return Err((format!("unknown_argument:{other}"), 2)),
        }
    }
    let cwd = cwd.map_or_else(
        || std::env::current_dir().map_err(io_error),
        |value| Ok(PathBuf::from(value)),
    )?;
    let registry = Registry::discover(&cwd, registry_arg.as_deref())?;
    let units = registry.manifest["units"]
        .as_array()
        .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
    let selected: Vec<Value> = units
        .iter()
        .filter(|unit| kind.as_ref().is_none_or(|value| unit["kind"] == *value))
        .filter(|unit| {
            tag.as_ref().is_none_or(|value| {
                unit["tags"]
                    .as_array()
                    .is_some_and(|tags| tags.iter().any(|tag| tag == value))
            })
        })
        .map(|unit| {
            json!({
                "id": unit["id"],
                "kind": unit["kind"],
                "version": unit["version"],
                "summary": unit["summary"],
                "tags": unit["tags"],
                "path": unit["path"]
            })
        })
        .collect();
    Ok(json!({
        "status": "ok",
        "operation": "loom_list",
        "count": selected.len(),
        "units": selected
    }))
}

fn show_command(raw: &[String]) -> Result<Value, (String, u8)> {
    let id = raw
        .first()
        .filter(|value| !value.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("");
    let mut readme = false;
    let mut registry_arg = None;
    let mut cwd = None;
    let mut index = usize::from(!id.is_empty());
    while index < raw.len() {
        match raw[index].as_str() {
            "--json" => index += 1,
            "--readme" => {
                readme = true;
                index += 1;
            }
            "--cwd" | "--registry" => {
                let value = raw
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| ("missing_argument".to_string(), 2))?;
                if raw[index] == "--cwd" {
                    cwd = Some(value);
                } else {
                    registry_arg = Some(value);
                }
                index += 2;
            }
            other => return Err((format!("unknown_argument:{other}"), 2)),
        }
    }
    show(id, readme, registry_arg.as_deref(), cwd)
}

fn show(
    id: &str,
    with_readme: bool,
    registry_arg: Option<&str>,
    cwd: Option<String>,
) -> Result<Value, (String, u8)> {
    if id.is_empty() {
        return Err(("missing_loom_unit".to_string(), 2));
    }
    let cwd = cwd.map_or_else(
        || std::env::current_dir().map_err(io_error),
        |value| Ok(PathBuf::from(value)),
    )?;
    let registry = Registry::discover(&cwd, registry_arg)?;
    let unit = registry
        .unit(id)
        .cloned()
        .ok_or_else(|| ("unknown_loom_unit".to_string(), 2))?;
    let sha256 = registry.lock["units"]
        .get(id)
        .and_then(|entry| entry["sha256"].as_str())
        .unwrap_or("");
    let mut value = json!({
        "status": "ok",
        "operation": "loom_show",
        "unit": unit,
        "sha256": sha256,
        "license": registry.manifest["license"],
        "source": {
            "repo": registry.lock["source"],
            "ref": registry.lock["ref"],
            "commit": registry.lock["commit"]
        }
    });
    if with_readme {
        let readme = registry
            .readme(id)
            .ok_or_else(|| ("loom_readme_missing".to_string(), 1))?;
        value["readme"] = json!(readme);
    }
    Ok(value)
}

fn io_error(error: std::io::Error) -> (String, u8) {
    (format!("loom_io_failed:{error}"), 1)
}
