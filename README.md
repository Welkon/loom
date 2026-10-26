# loom

`loom` installs pinned embedded C units into a project.

It is the CLI and lockfile layer, independent of the catalog. The catalog lives
at <https://git.pcbsmt.me/rh/emb-loom>, and the site at
<https://loom.pcbsmt.me>.

```sh
loom search "load cell"
loom add ads1115
loom integrate
loom verify
loom guard --staged
```

## Registry

A registry is a directory with `loom.json` and `loom.lock`. Select it with
`--registry DIR`, `EMB_LOOM_REGISTRY`, or the package/project fallback paths.
The CLI never bundles a catalog.

`--registry` also accepts a Git URL (`https://`, `ssh://`, `git@`, or
`file://`). The registry is cloned into `$XDG_CACHE_HOME/loom/registries` (or
`~/.cache/loom/registries`). Set `LOOM_REGISTRY_REFRESH=1` to discard the cache
before the next clone.

## Publish and yank

```sh
loom publish ./unit --id my-unit --kind driver --registry ./registry
loom publish ./unit --id my-unit --kind driver --registry ./registry --push
loom yank my-unit --registry ./registry --prune --push
```

`publish` adds or replaces a unit in an existing registry. `yank` removes it
from the index and lock; `--prune` also removes its files. `--push` commits the
registry and pushes it; add `--sign` to create a signed Git commit.

## Pack

```sh
loom pack ./my-unit --id my-unit --kind driver --out ./registry
```

The output is a small registry containing `loom.json`, `loom.lock`, the unit,
and its license. `loom add` can consume it directly.

## Registry index

```sh
loom index --out index.json
```

The index records unit versions, paths, requirements, and SHA-256 values.
Publish it with the catalog; consumers can use a Git/HTTPS registry URL.

## Doctor

```sh
loom doctor
```

Checks installed unit hashes, the generated `firmware/loom` fragments, and
whether a CMake/Make/Cargo file already includes them. It reports warnings
for missing build includes and fails on modified units or stale fragments.

## Integrate

`loom integrate` writes:

```
firmware/loom/loom.mk
firmware/loom/loom.cmake
firmware/loom/loom.cargo.rs
firmware/loom/README.md
```

Make/CMake/Cargo can include the generated fragments. `loom add`, `remove`,
`install`, and `update` refresh them automatically. `loom integrate --check`
fails when they are stale.

`loom integrate --apply` adds a marker block to an existing `Makefile`,
`CMakeLists.txt`, or `build.rs`; rerunning replaces the block. CMake accepts
`--target NAME` to emit `loom_attach(NAME)`.

For Cargo, `loom.cargo.rs` exposes `LOOM_SOURCES`, `LOOM_INCLUDE_DIRS`, and
`build()`; include it from `build.rs` and add `cc` as a build dependency.

## License

AGPL-3.0-or-later. Catalog units keep their own MIT/BSD/Apache licenses.
