# Changelog

## 0.19.0

- `loom search` treats the query as keywords instead of one substring. A phrase
  such as `TQ322 PMS150G Mini-C PIR` now matches the distinctive `PIR` term, and
  the result is ranked by the best field each term hits.
- `loom search` accepts `--kind KIND` and `--tag TAG`, as the emb-loom skill
  already documented. Before this release the same flags only worked on
  `loom list`, and search rejected them with `unknown_argument:--kind`.
- A read-only cache directory no longer fails search. When `HOME` or
  `XDG_CACHE_HOME` cannot accept the cloned catalog, the cache falls back to the
  system temp directory while keeping an existing cache usable.

## 0.18.0

- Publish to crates.io as `loomkit`; `loom` was taken. The installed command is
  still `loom`, and the library keeps the name `loom`, so a Git dependency moves
  to `loom = { package = "loomkit", git = ..., tag = "v0.18.0" }`.
- The packaged crate excludes `vendor/` and `.cargo/config.toml`, which exist for
  offline builds in this repository only.

## 0.17.0

- Standalone use: when nothing names a catalog, `loom` falls back to the
  published one (`LOOM_CATALOG`, then `Welkon/loom-package`), cloned once into
  the cache. `LOOM_REGISTRY` is accepted next to `EMB_LOOM_REGISTRY`, and an
  empty `LOOM_CATALOG` keeps `loom_registry_missing` for offline use.
- Prebuilt binaries: a release workflow publishes `loom-<platform>` for five
  targets with `checksums-sha256.txt` and `runtime-manifest.json`, and
  `install.sh` verifies the checksum before installing.

## 0.16.1

- `loom --version` prints the crate version, so a host can pick the newer of
  an installed binary and an embedded copy.

## 0.16.0

- `loom adapter ID` writes `board/ports_<id>.h` and `.c` with one stub per port
  function and a `board_<id>_port()` builder. `--check` reports a port function
  with no assignment; `--force` overwrites a hand-edited file.

## 0.15.0

- `yank ID@VERSION --index DIR` marks a static index version yanked; `--undo`
  clears it. The tarball and history remain.

## 0.14.0

- Add `loom index --static --out DIR` to export any Git registry as a static
  index with deterministic tarballs.

## 0.13.0

- Read `auth-required` from static `config.json` and send a Bearer token from
  `--token` or `LOOM_TOKEN`.

## 0.12.3

- Reject unsafe unit names, versions, and non-hex checksums from static
  indexes before building a cache path.

## 0.12.2

- Make the immutable-tag test set its own Git identity, so it passes in a
  clean container.

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
