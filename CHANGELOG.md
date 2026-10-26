# Changelog

## 0.2.0

- Resolve `requires` recursively when adding, installing, or updating units.
- Include required units in `loom.lock` and build fragments.

## 0.1.1

- Fix the registry override lifetime in `run_with_registry`.

## 0.1.0

- Split the loom CLI and lockfile from emb-agent.
- Add `loom integrate` with Make, CMake, and Cargo fragments.
