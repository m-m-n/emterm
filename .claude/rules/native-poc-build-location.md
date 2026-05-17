# native-poc Build Location

The `native-poc/` crate uses dedicated `CARGO_TARGET_DIR` paths so it
doesn't clobber the main Tauri build cache, **and** so multiple Claude
Code sessions agree on where the binary lives.

## Build commands

Always set `CARGO_TARGET_DIR` to one of the paths below. Do **not** let
cargo fall back to the workspace-default `target/` — another session may
already be running `native-poc/target-host/release/emterm-native-poc`,
and writing the new binary somewhere else means your fix won't be
visible to them.

| Purpose                  | Target dir              | Run command                                                 |
| ------------------------ | ----------------------- | ----------------------------------------------------------- |
| Quick check / unit tests | `./target` (inside `native-poc/`) | `CARGO_TARGET_DIR=./target cargo check`<br>`CARGO_TARGET_DIR=./target cargo test --bin emterm-native-poc` |
| Release binary the user runs | `./target-host`     | `CARGO_TARGET_DIR=./target-host cargo build --release`      |

The release binary lives at:

```
native-poc/target-host/release/emterm-native-poc
```

## When to rebuild release

Rebuild `target-host/release/` whenever you change `native-poc/` source
that the user is actively running. Always run cargo from inside
`native-poc/` so relative `./target` / `./target-host` resolve
correctly.

## Why two target dirs

- `target/` keeps the fast debug + test cycle isolated from the
  release binary the user runs, so a `cargo test` in this session
  doesn't trigger a multi-minute relink of the running release build.
- `target-host/` is the **single source of truth** for "the binary the
  user launches" — keep all release builds writing here so concurrent
  sessions stay in sync.
