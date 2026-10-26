# Changelog

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
