# Commands

Build, test, and run commands for this project. Run every command from the
project root — see `core-build-location.md` for why, and for the
`CARGO_TARGET_DIR` matrix these commands rely on.

## Setup

```bash
bun install
make setup   # rustup target add x86_64-pc-windows-msvc + cargo install cargo-xwin
```

## Running and building

| Purpose | Command |
| --- | --- |
| Development (Linux host, GUI) | `make dev` — `bun run build:viewer` + `build:settings` + `cargo run` |
| Release build (Linux, GUI) | `make build` — writes to `src-tauri/target-host` |
| CLI-only build (no winit/wgpu/wry) | `make cli-build` — `cargo build --release --no-default-features` |
| Windows cross-build | `make win-build` — `cargo xwin build --release --target x86_64-pc-windows-msvc`, writes to `src-tauri/target-win` |
| deb (GUI) | `make dpkg` — `build/emterm_<ver>_<arch>.deb`, depends on libwebkit2gtk-4.1-0 |
| deb (CLI only) | `make cli-dpkg` — `build/emterm-cli_<ver>_<arch>.deb`, depends on libc6 |

The GUI build (`--features gui`, default-on) embeds `viewer/dist/` and
`settings/dist/` via `build.rs`. Before any manual GUI release build, run:

```bash
bun run build:viewer
bun run build:settings
```

`make build` / `make dpkg` run these for you. CLI-only builds
(`--no-default-features`) skip the dist check entirely and need no bun
involvement.

## Testing and verification

**Rust unit + integration tests** (default features):
```bash
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml
```

**CLI-only feature check** (verifies feature gates still compile):
```bash
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

**TypeScript (child WebView bundles):**
```bash
bun test            # uses test-setup.ts (happy-dom + i18n init)
bun run typecheck   # tsc --noEmit, scoped to src-tauri/{viewer,settings}/web
```

## Icon generation

Requires `rsvg-convert` or `magick` (ImageMagick):

```bash
bash scripts/generate-icons.sh
```

Generates `src-tauri/icons/{32x32,128x128,128x128@2x}.png` from
`assets/eMterm.svg`. Called automatically by `make dpkg`.
