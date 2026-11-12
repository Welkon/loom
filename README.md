# loom

`loom` installs pinned embedded C units into a project.

It is the CLI and lockfile layer, independent of the catalog. The catalog lives
at <https://github.com/Welkon/loom-package>, and the site at
<https://loom.pcbsmt.me>.

`loom` runs on its own: no IDE, no project template, and no other tool in this
family. `emb-agent loom` reaches the same commands when that runtime is
installed, but nothing here needs it.

## Install

```sh
cargo install loomkit
```

The crate is `loomkit` because `loom` belongs to another crate. The command it
installs is `loom` and the library keeps the name `loom`, so a Git dependency
reads `loom = { package = "loomkit", git = "https://github.com/Welkon/loom.git",
tag = "v0.19.0" }`.

Without a Rust toolchain, take a release binary:

```sh
curl -fsSL https://raw.githubusercontent.com/Welkon/loom/main/install.sh | sh
```

The script picks the release for this platform, checks it against
`checksums-sha256.txt`, and installs to `~/.local/bin`. `LOOM_VERSION` selects a
release and `LOOM_BINDIR` the directory. Every release carries
`loom-<platform>`, `checksums-sha256.txt`, and `runtime-manifest.json` for the
five supported platforms: `linux-x86_64`, `linux-arm64`, `macos-x86_64`,
`macos-arm64`, and `windows-x86_64`.

From a checkout:

```sh
cargo install --git https://github.com/Welkon/loom.git --tag v0.19.0
```

## Quickstart

```sh
loom search "load cell"
loom add ads1115
loom integrate
loom verify
loom guard --staged
loom adapter ads1115      # write board/ports_ads1115.{h,c}
```

`loom search` matches catalog metadata, not source code, and it is lexical: a
query is split into keywords and ranked by the unit id, summary, tags and
applications each term hits. Use short terms (sensor family, protocol,
operation) plus `--kind driver|operator` and `--tag`; a long sentence works only
through its distinctive words.

No flag is needed to find the catalog: the first command that needs one clones
`Welkon/loom-package` into the cache. `--registry DIR|URL`, `--index URL|DIR`,
or `LOOM_CATALOG` point somewhere else; `LOOM_CATALOG=` (empty) disables the
fallback for offline work. `--index` reaches the static registry when a Git
clone is not wanted.

```sh
loom add ads1115 --index https://loom.pcbsmt.me
loom --version
```

## Static index

`--index` uses the same layout as a sparse Cargo registry:

```
config.json
me/di/median
tarballs/median-0.2.0.tar.gz
```

Each index file has one JSON line per version. Each line carries the tarball
SHA-256. Tarballs are cached under `$XDG_CACHE_HOME/loom/tarballs` by hash, so a
version is downloaded once. Old versions stay installable until they are
yanked; yanking marks a line, it does not delete history.

```sh
loom add median@0.1.0 --index https://loom.pcbsmt.me
loom add median --index ./site/dist
loom install --index https://loom.pcbsmt.me
loom update median --index https://loom.pcbsmt.me
```

The static client uses `curl` for HTTPS. A local directory needs no network.

For a private index, set `LOOM_TOKEN` or pass `--token`. The client sends a
Bearer token when `config.json` sets `"auth-required": true`; the lock and
provenance never contain the token.

## Registry

A registry is a directory with `loom.json` and `loom.lock`. Select it with
`--registry DIR`, `LOOM_REGISTRY`, `EMB_LOOM_REGISTRY`, or `LOOM_CATALOG`; with
nothing set, the published catalog is cloned into the cache. The CLI never
bundles a catalog.

`--index` reads a Cargo-style static registry: per-unit index files,
`config.json`, and one tarball per version. It accepts an HTTPS URL or a local
directory. `--registry` also accepts a Git URL (`https://`, `ssh://`, `git@`, or
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

`loom index --static --out DIR` exports the same registry as a Cargo-style
static tree: `config.json`, per-unit index files, and one tarball per current
unit version. Serve the directory over HTTPS, or pass it to `loom add --index`.

`loom yank ID@VERSION --index DIR` marks that version yanked in the static
index. The tarball and index line stay; `--undo` clears the flag.

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

## Adapter

`loom adapter ID` writes the project-owned port implementation for an installed
driver:

```
board/ports_<id>.h
board/ports_<id>.c
```

The header declares `board_<id>_port()`, the builder the unit README uses in its
integration block. The source has one stub per port function, with the exact
signature and a `TODO`; `delay_us` and `delay_ms` call `board_delay_us()` and
`board_delay_ms()`.

The generated source includes `target.h`. The command does not create it: that
file is where the project names pins and registers, and it is the only place
that includes a vendor header. `loom adapter` never writes an MCU name.

```sh
loom adapter hx711
loom adapter hx711 --check   # a port function has no assignment
loom adapter hx711 --force   # overwrite a hand-edited file
```

`--check` reads the unit header and fails when a port function has no
`port.<name> =` assignment, so a unit update that adds a function is caught.
Hand edits are preserved. An existing file is skipped without `--force`.

## Examples

`examples/sc8f072-hx711/` is a worked adapter for an 8-bit SCMCU target: the
generated stubs, a filled `board/target.h`, and a host test that runs the unit's
own behavior assertions without hardware or XC8.

```sh
make -C examples/sc8f072-hx711 test
```

## License

AGPL-3.0-or-later. Catalog units keep their own MIT/BSD/Apache licenses.
