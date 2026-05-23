# native-poc Build Location

The `native-poc/` crate uses dedicated `CARGO_TARGET_DIR` paths so it
doesn't clobber the main Tauri build cache, **and** so multiple Claude
Code sessions agree on where the binary lives.

## Work from the project root — do NOT `cd`

Always run cargo from the project root
(`/home/sakura/src/my_projects/tauri/emterm/`). Do **not** `cd
native-poc/` first.

Why: Claude Code's Bash tool keeps a persistent shell cwd. If this
session `cd`s into `native-poc/`, the next command the assistant tells
the user to run (e.g. `./native-poc/target-host/release/emterm-native-poc`)
is written against the *user's* shell cwd (= project root), but the
assistant has lost track of that — leading to "no such file" confusion.
Use `--manifest-path` and an explicit `CARGO_TARGET_DIR` instead.

## Build commands

Always set `CARGO_TARGET_DIR` to one of the paths below. Do **not** let
cargo fall back to the workspace-default `target/` — another session may
already be running `native-poc/target-host/release/emterm-native-poc`,
and writing the new binary somewhere else means your fix won't be
visible to them.

| Purpose                      | Target dir               | Run command (from project root)                                                                                                                |
| ---------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Quick check / unit tests     | `native-poc/target`      | `CARGO_TARGET_DIR=native-poc/target cargo check --manifest-path native-poc/Cargo.toml`<br>`CARGO_TARGET_DIR=native-poc/target cargo test --manifest-path native-poc/Cargo.toml --bin emterm-native-poc` |
| Release binary the user runs | `native-poc/target-host` | `CARGO_TARGET_DIR=native-poc/target-host cargo build --release --manifest-path native-poc/Cargo.toml`                                          |

The release binary lives at:

```
native-poc/target-host/release/emterm-native-poc
```

## When to rebuild release

Rebuild `target-host/release/` whenever you change `native-poc/` source
that the user is actively running. Keep the commands above (run from the
project root with `--manifest-path`) so relative `native-poc/target` /
`native-poc/target-host` resolve consistently between sessions.

## Why two target dirs

- `target/` keeps the fast debug + test cycle isolated from the
  release binary the user runs, so a `cargo test` in this session
  doesn't trigger a multi-minute relink of the running release build.
- `target-host/` is the **single source of truth** for "the binary the
  user launches" — keep all release builds writing here so concurrent
  sessions stay in sync.
