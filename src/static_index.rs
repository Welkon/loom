//! Export a Git registry as a Cargo-style static registry.

use super::ops::spec;
use super::registry::{Registry, UnitFile};
use super::spec::Spec;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use tar::{Builder, Header};

type Failure = (String, u8);

pub(super) fn write(registry: &Registry, output: &Path) -> Result<Value, Failure> {
    let units = registry.manifest["units"]
        .as_array()
        .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
    fs::create_dir_all(output.join("tarballs")).map_err(io_error)?;
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut hashes = Vec::new();
    for unit in units {
        let id = unit["id"]
            .as_str()
            .ok_or_else(|| ("invalid_loom_manifest".to_string(), 1))?;
        let spec = spec(registry, id)?;
        let bytes = tarball(&spec)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let name = format!("{}-{}.tar.gz", spec.id, spec.version);
        fs::write(output.join("tarballs").join(&name), &bytes).map_err(io_error)?;
        lines.push((
            index_path(id),
            format!(
                "{}\n",
                json!({
                    "id": spec.id, "version": spec.version, "kind": spec.kind,
                    "path": unit["path"], "requires": spec.requires,
                    "sha256": hash, "commit": spec.commit, "yanked": false
                })
            ),
        ));
        hashes.push(json!({"id": id, "version": spec.version, "sha256": hash}));
    }
    lines.sort_by(|left, right| left.0.cmp(&right.0));
    for (relative, line) in &lines {
        let path = output.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fs::write(path, line).map_err(io_error)?;
    }
    fs::write(
        output.join("config.json"),
        serde_json::to_vec_pretty(&json!({
            "dl": "tarballs/{id}-{version}.tar.gz",
            "auth-required": false
        }))
        .map_err(|_| ("invalid_loom_index".to_string(), 1))?,
    )
    .map_err(io_error)?;
    Ok(json!({
        "status": "ok", "operation": "loom_index_static",
        "output": output.to_string_lossy().replace('\\', "/"),
        "count": units.len(), "units": hashes
    }))
}

fn tarball(spec: &Spec) -> Result<Vec<u8>, Failure> {
    let root = format!("{}-{}", spec.id, spec.version);
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);
    let mut files: Vec<&UnitFile> = spec.files.iter().collect();
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    for file in files {
        append(
            &mut builder,
            &format!("{root}/{}", file.relative),
            &file.bytes,
        )?;
    }
    if !spec.license.is_empty() {
        append(
            &mut builder,
            &format!("{root}/LICENSE"),
            spec.license.as_bytes(),
        )?;
    }
    let encoder = builder.into_inner().map_err(io_error)?;
    let mut encoder = encoder;
    encoder.flush().map_err(io_error)?;
    encoder.finish().map_err(io_error)
}

fn append(
    builder: &mut Builder<GzEncoder<Vec<u8>>>,
    name: &str,
    bytes: &[u8],
) -> Result<(), Failure> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, name, bytes)
        .map_err(io_error)
}

fn index_path(id: &str) -> String {
    match id.len() {
        0 | 1 => format!("1/{id}"),
        2 => format!("2/{id}"),
        3 => format!("3/{}/{}", &id[..1], id),
        _ => format!("{}/{}/{}", &id[..2], &id[2..4], id),
    }
}

fn io_error(error: std::io::Error) -> Failure {
    (format!("loom_io_failed:{error}"), 1)
}

#[cfg(test)]
mod tests {
    use super::index_path;

    #[test]
    fn uses_cargo_prefixes() {
        assert_eq!(index_path("median"), "me/di/median");
        assert_eq!(index_path("a"), "1/a");
    }
}
