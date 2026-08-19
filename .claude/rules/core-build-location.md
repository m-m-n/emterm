# Build Location

The `src-tauri/` crate uses dedicated `CARGO_TARGET_DIR` paths so
multiple Claude Code sessions agree on where the binary lives.

## Work from the project root — do NOT `cd`

Always run cargo from the project root
(`/home/sakura/src/my_projects/tauri/emterm/`). Do **not** `cd
src-tauri/` first.

Why: Claude Code's Bash tool keeps a persistent shell cwd. If this
session `cd`s into `src-tauri/`, the next command the assistant tells
the user to run (e.g. `./src-tauri/target-host/release/emterm`) is
written against the *user's* shell cwd (= project root), but the
assistant has lost track of that — leading to "no such file" confusion.
Use `--manifest-path` and an explicit `CARGO_TARGET_DIR` instead.

## Build commands

Always set `CARGO_TARGET_DIR` to one of the paths below. Do **not** let
cargo fall back to the workspace-default `target/` — another session may
already be running `src-tauri/target-host/release/emterm`, and writing
the new binary somewhere else means your fix won't be visible to them.

| Purpose                      | Target dir               | Run command (from project root)                                                                                                                                                       |
| ---------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Quick check / unit tests     | `src-tauri/target`       | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`<br>`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --bin emterm` |
| Release binary the user runs | `src-tauri/target-host`  | `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`                                                                                   |
| Windows cross-build          | `src-tauri/target-win`   | `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`                                               |

The release binary lives at:

```
src-tauri/target-host/release/emterm
```

(or `.exe` on Windows under `src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe`).

## CLI-only check

To verify the `--no-default-features` (CLI-only) build still compiles,
use the quick-check target dir:

```
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

## When to rebuild release

Rebuild `target-host/release/` whenever you change `src-tauri/` source
that the user is actively running. Keep the commands above (run from the
project root with `--manifest-path`) so relative `src-tauri/target` /
`src-tauri/target-host` resolve consistently between sessions.

## Why three target dirs

- `target/` keeps the fast debug + test + `--no-default-features` cycle
  isolated from the release binary the user runs, so a `cargo test` in
  this session doesn't trigger a multi-minute relink of the running
  release build.
- `target-host/` is the **single source of truth** for "the binary the
  user launches" — keep all release builds writing here so concurrent
  sessions stay in sync.
- `target-win/` isolates the Windows cross-build so it doesn't share
  artifacts with the Linux host build.

## Web bundle prerequisites

The GUI build embeds `viewer/dist/` and `settings/dist/` via `build.rs`, so the
bun bundles must exist before any GUI release build. See `core-commands.md`.
