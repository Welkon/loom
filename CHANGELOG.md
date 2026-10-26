# Changelog

## 0.12.1

- `install` and `update` accept `--index`, so a static registry can restore
  or advance a project without a Git registry.

## 0.12.0

- Add Cargo-style static registries: per-unit index files, `config.json`,
  version tarballs, SHA-256 checks, and a content-addressed cache.
- `loom add ID --index URL|DIR` installs from a static index without cloning
  the registry.

## 0.11.0

- Add `publish/yank --push` and optional `--sign` for Git registries.

## 0.10.0

- Add semver constraints for `requires`: `^`, `~`, comparators, and exact.

## 0.9.0

- Resolve `unit/<id>/v<version>` Git tags for historical installs.
- List available unit versions in `loom index`.

## 0.8.0

- Add `loom integrate --apply` for Make, CMake, and Cargo marker blocks.

## 0.7.0

- Add `loom publish` and `loom yank` for local registry maintenance.

## 0.6.0

- Add `loom doctor` for installed hashes, integration fragments, and build
  include checks.

## 0.5.1

- Let an explicit `--registry` override the emb-agent runtime fallback.

## 0.5.0

- Add `loom pack` for producing an addable local registry from a unit.

## 0.4.0

- Add `loom index` for standalone registry indexes.

## 0.3.0

- Accept Git/HTTPS/file registry URLs with a local clone cache.
- Add `LOOM_REGISTRY_REFRESH=1` for cache refresh.

## 0.2.0

- Resolve `requires` recursively when adding, installing, or updating units.
- Include required units in `loom.lock` and build fragments.

## 0.1.1

- Fix the registry override lifetime in `run_with_registry`.

## 0.1.0

- Split the loom CLI and lockfile from emb-agent.
- Add `loom integrate` with Make, CMake, and Cargo fragments.
