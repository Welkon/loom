//! Standalone loom binary and explicit registry path.

use std::fs;

#[test]
fn standalone_loom_binary_reads_an_explicit_registry() {
    let root = std::env::temp_dir().join(format!("loom-standalone-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("operators/demo")).unwrap();
    fs::write(root.join("operators/demo/README.md"), "demo").unwrap();
    fs::write(root.join("operators/demo/demo.c"), "void demo(void) {}\n").unwrap();
    fs::write(root.join("operators/demo/demo.h"), "void demo(void);\n").unwrap();
    fs::write(root.join("LICENSE"), "MIT").unwrap();
    fs::write(root.join("loom.json"), r#"{"schema":1,"name":"test","version":"0.0.1","license":"MIT","units":[{"id":"demo","kind":"operator","version":"0.1.0","path":"operators/demo","summary":"Demo","tags":["demo"],"applications":["test"]}]}"#).unwrap();
    fs::write(root.join("loom.lock"), r#"{"schema":1,"source":"test","ref":"x","commit":"x","units":{"demo":{"version":"0.1.0","sha256":""}}}"#).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["list", "--registry", root.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["count"], 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn loom_reads_a_registry_from_disk() {
    let root = std::env::temp_dir().join(format!("loom-registry-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("operators/only")).unwrap();
    fs::write(
        root.join("loom.json"),
        r#"{"schema":1,"name":"temp","version":"9.9.9","license":"MIT","units":[{"id":"only","kind":"operator","version":"0.1.0","path":"operators/only","summary":"Only unit","tags":[],"applications":[]}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("loom.lock"),
        r#"{"schema":1,"source":"temp","ref":"x","commit":"x","units":{"only":{"version":"0.1.0","sha256":"abc"}}}"#,
    )
    .unwrap();
    fs::write(root.join("operators/only/README.md"), "only").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["list", "--registry", root.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["count"].as_u64(), Some(1));
    assert_eq!(value["units"][0]["id"], "only");
    let _ = fs::remove_dir_all(&root);
}

use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

fn write_registry(root: &Path, version: &str, body: &str) {
    let unit = root.join("operators/demo");
    fs::create_dir_all(&unit).unwrap();
    let readme = format!("# demo\n\nVersion {version}\n");
    let header = "void demo(void);\n";
    let mut hasher = Sha256::new();
    for (relative, content) in [
        ("README.md", readme.as_str()),
        ("demo.c", body),
        ("demo.h", header),
    ] {
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(content.as_bytes());
        hasher.update(b"\0");
    }
    let sha256 = format!("{:x}", hasher.finalize());
    fs::write(unit.join("demo.c"), body).unwrap();
    fs::write(unit.join("demo.h"), header).unwrap();
    fs::write(unit.join("README.md"), readme).unwrap();
    let manifest = json!({
        "schema": 1,
        "name": "temp",
        "version": version,
        "license": "MIT",
        "units": [{
            "id": "demo",
            "kind": "operator",
            "version": version,
            "path": "operators/demo",
            "summary": "Demo unit",
            "tags": ["demo"],
            "applications": ["test"]
        }]
    });
    let lock = json!({
        "schema": 1,
        "source": "temp",
        "ref": version,
        "commit": "x",
        "units": {"demo": {"version": version, "sha256": sha256}}
    });
    fs::write(
        root.join("loom.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("loom.lock"),
        serde_json::to_vec_pretty(&lock).unwrap(),
    )
    .unwrap();
}

#[test]
fn loom_update_moves_a_unit_to_the_registry_version() {
    let base = std::env::temp_dir().join(format!("loom-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let registry = base.join("registry");
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    write_registry(&registry, "0.1.0", "old\n");

    let add = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "add",
            "demo",
            "--registry",
            registry.to_str().unwrap(),
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "{add:?}");
    let file = project.join("firmware/lib/demo/demo.c");
    assert_eq!(fs::read_to_string(&file).unwrap(), "old\n");

    write_registry(&registry, "0.2.0", "new\n");
    let outdated = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "outdated",
            "--registry",
            registry.to_str().unwrap(),
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(outdated.status.success(), "{outdated:?}");
    let value: serde_json::Value = serde_json::from_slice(&outdated.stdout).unwrap();
    assert_eq!(value["units"][0]["available"], "0.2.0");
    assert_eq!(value["units"][0]["outdated"], true);

    let update = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "update",
            "--registry",
            registry.to_str().unwrap(),
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(update.status.success(), "{update:?}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");
    let manifest = fs::read_to_string(project.join("loom.toml")).unwrap();
    assert!(manifest.contains("demo = \"0.2.0\""));
    let lock = fs::read_to_string(project.join("loom.lock")).unwrap();
    assert!(lock.contains("version = \"0.2.0\""));

    let _ = fs::remove_dir_all(&base);
}

fn guard_project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("loom-guard-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("firmware/drivers/custom")).unwrap();
    fs::write(root.join("loom.toml"), "[units]\n").unwrap();
    root
}

#[test]
fn loom_guard_requires_provenance_or_custom_marker() {
    let root = guard_project("custom");
    let source = root.join("firmware/drivers/custom/custom.c");
    fs::write(&source, "void custom(void) {}\n").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["guard", "--all", "--cwd"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "failed");
    assert_eq!(value["error"]["code"], "loom_guard_unprovenanced");
    assert_eq!(value["checked"], 1);

    fs::write(
        &source,
        "/* loom: custom - board-specific */\nvoid custom(void) {}\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["guard", "--all", "--cwd"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["files"][0]["source"], "custom");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn loom_guard_accepts_an_installed_unit_and_skips_other_projects() {
    let root = guard_project("installed");
    fs::create_dir_all(root.join("firmware/drivers/median")).unwrap();
    fs::write(
        root.join("firmware/drivers/median/median.c"),
        "void median(void) {}\n",
    )
    .unwrap();
    fs::write(
        root.join("loom.lock"),
        "version = 1\n\n[[unit]]\nid = \"median\"\nversion = \"0.1.0\"\nsha256 = \"0\"\npath = \"firmware/drivers/median\"\nsource = \"test\"\ncommit = \"0\"\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["guard", "--all", "--cwd"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["files"][0]["source"], "loom");
    let _ = fs::remove_dir_all(&root);

    let empty = std::env::temp_dir().join(format!("loom-guard-empty-{}", std::process::id()));
    let _ = fs::remove_dir_all(&empty);
    fs::create_dir_all(&empty).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["guard", "--all", "--cwd"])
        .arg(&empty)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "skipped");
    let _ = fs::remove_dir_all(&empty);
}

#[test]
fn loom_integrate_writes_make_cmake_and_cargo_fragments() {
    let base = std::env::temp_dir().join(format!("loom-integrate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let registry = base.join("registry");
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    write_registry(&registry, "0.1.0", "void demo(void) {}\n");

    let add = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "add",
            "demo",
            "--registry",
            registry.to_str().unwrap(),
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "{add:?}");
    let make = fs::read_to_string(project.join("firmware/loom/loom.mk")).unwrap();
    assert!(make.contains("firmware/lib/demo/demo.c"));
    let cmake = fs::read_to_string(project.join("firmware/loom/loom.cmake")).unwrap();
    assert!(cmake.contains("LOOM_SOURCES"));
    let cargo = fs::read_to_string(project.join("firmware/loom/loom.cargo.rs")).unwrap();
    assert!(cargo.contains("LOOM_SOURCES"));
    assert!(cargo.contains("cc::Build"));

    let check = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "integrate",
            "--check",
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");

    fs::write(project.join("firmware/loom/loom.mk"), "stale\n").unwrap();
    let stale = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "integrate",
            "--check",
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success(), "{stale:?}");
    let value: serde_json::Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert_eq!(value["error"]["code"], "loom_integrate_stale");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn loom_add_installs_required_units_and_integrates_them() {
    let base = std::env::temp_dir().join(format!("loom-deps-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let registry = base.join("registry");
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    write_registry(&registry, "0.1.0", "void demo(void) {}\n");

    let base_unit = registry.join("operators/base");
    fs::create_dir_all(&base_unit).unwrap();
    fs::write(base_unit.join("README.md"), "# base\n").unwrap();
    fs::write(base_unit.join("base.h"), "void base(void);\n").unwrap();
    fs::write(base_unit.join("base.c"), "void base(void) {}\n").unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(registry.join("loom.json")).unwrap()).unwrap();
    manifest["units"][0]["requires"] = json!(["base"]);
    manifest["units"].as_array_mut().unwrap().push(json!({
        "id":"base","kind":"operator","version":"0.1.0","path":"operators/base",
        "summary":"Base","tags":["base"],"applications":["test"]
    }));
    fs::write(
        registry.join("loom.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let mut lock: serde_json::Value =
        serde_json::from_slice(&fs::read(registry.join("loom.lock")).unwrap()).unwrap();
    lock["units"]["base"] = json!({"version":"0.1.0","sha256":"x"});
    fs::write(
        registry.join("loom.lock"),
        serde_json::to_vec_pretty(&lock).unwrap(),
    )
    .unwrap();

    let add = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "add",
            "demo",
            "--registry",
            registry.to_str().unwrap(),
            "--cwd",
            project.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "{add:?}");
    assert!(project.join("firmware/lib/base/base.c").is_file());
    let manifest = fs::read_to_string(project.join("loom.toml")).unwrap();
    assert!(manifest.contains("base"));
    let make = fs::read_to_string(project.join("firmware/loom/loom.mk")).unwrap();
    assert!(make.contains("firmware/lib/base/base.c"));
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn loom_reads_a_git_registry_url() {
    let base = std::env::temp_dir().join(format!("loom-git-registry-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let registry = base.join("registry");
    fs::create_dir_all(&registry).unwrap();
    write_registry(&registry, "0.1.0", "void demo(void) {}\n");
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "loom@example.invalid"],
        vec!["config", "user.name", "loom"],
        vec!["add", "-A"],
        vec!["commit", "-qm", "fixture"],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(&registry)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let url = format!("file://{}", registry.display());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["list", "--registry", &url, "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["count"], 1);
    let _ = fs::remove_dir_all(&base);
}
