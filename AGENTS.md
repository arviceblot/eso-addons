# AGENTS.md

## Overview

ESO Addon Manager — cross-platform GUI app (egui/eframe) for managing The Elder Scrolls Online addons. SQLite via SeaORM for local storage. Cargo workspace with 7 members, Rust edition 2024 (root, bbcode, bbcode-egui, tools) and 2021 (entity, migration).

## Workspace Layout

| Crate | Role |
|---|---|
| Root (`eso-addon-manager`) | Main GUI binary. Entrypoint `src/main.rs`. Views in `src/views/`. |
| `core` (`eso-addons-core`) | Business logic: API client, addon service, config, DB access. |
| `entity` | **Generated** SeaORM entity structs. Do not edit by hand. |
| `migration` | SeaORM SQLite migrations. Add new files here for schema changes. |
| `bbcode` | BBCode tokenizer / parser / AST (library). |
| `bbcode-egui` | Renders BBCode AST into egui widgets. |
| `tools` | Dev-only binaries (`scan_corpus`). `publish = false`. |

## Essential Commands

```bash
# Run the app (GUI)
cargo run eso-addon-manager

# Full CI-style check (check -> fmt -> clippy -> test)
./check.sh

# Individual steps
cargo check --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
```

Run a single package: add `-p <crate>` (e.g. `cargo test -p bbcode`).

## SeaORM Entity & Migration Workflow

`entity/` contains auto-generated code. After changing a migration or schema:

```bash
# Requires DATABASE_URL in .env (see below)
sea-orm-cli generate entity -o entity/src
```

New migrations go in `migration/src/` with the `m<YYYYMMDD>_<HHMMSS>_<name>.rs` naming convention. See existing files for the pattern.

## Database

SQLite. `.env` is gitignored and must be created locally. The app loads it via `dotenv` on startup (`src/main.rs`). The only variable needed is `DATABASE_URL`, e.g.:

```
DATABASE_URL=sqlite://<path-to-your-local-db>/addons.db
```

Set it to a writable path for local dev. CI does not depend on a real database; core tests exercise in-memory SQLite.

## Pre-commit Hooks

`.pre-commit-config.yaml` runs `cargo fmt`, `cargo check`, and `markdown-toc`. Install with `pre-commit install` or run manually with `pre-commit run --all-files`.

## AppImage Build (Linux releases)

Requires `cargo-zigbuild` and `ziglang==0.16.0` (installed via `pip3`). Builds with glibc 2.28 floor.

```bash
# Build
./scripts/build-appimage.sh

# Smoke test (X11 + Wayland, needs xvfb/weston)
./scripts/smoke-deps.sh   # install deps (CI helper, uses sudo)
./scripts/smoke-appimage.sh
```

The smoke test sets `ESO_ADDONS_SMOKE_TEST=1` so the app renders one frame, prints a marker, then exits cleanly.

## macOS Releases

Universal binary built by merging `aarch64-apple-darwin` and `x86_64-apple-darwin` via `lipo`. See `.github/workflows/release.yml` job `build_macos_universal`.

## Release Profile

`Cargo.toml` release profile: `opt-level = 'z'`, LTO, single codegen unit, `panic = 'abort'`, stripped. Optimized for small binary size.

## Test Fixtures

`test-addons/` contains sample ESO addon directories and backup JSON files used by core tests. Do not delete or rename without updating the tests that reference them.

## Gotchas

- CI sets `RUSTFLAGS=-D warnings` and `RUSTDOCFLAGS=-D warnings`. Any warning is a CI failure.
- `entity/` is generated — never hand-edit those files. Regenerate instead.
- The `.vscode/tasks.json` has stale `npm run dev/build` tasks from a template; ignore them. Use `cargo run eso-addon-manager` instead.
- The binary name is `eso-addon-manager` (not `eso-addons`). The old name appears in stale VS Code launch configs.
