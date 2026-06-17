# CLAUDE.md

eMterm is a native terminal emulator for Linux and Windows with a wgpu+swash render pipeline and child WebView windows for rich Markdown / JSON / YAML / image display and the settings panel.

## What (Technology Stack)

**Primary technologies:**
- Rust — native terminal stack: winit (event loop, IME), wgpu (GPU surface), egui (in-process UI), swash + zeno + fontdb (font rasterization), portable-pty (PTY abstraction)
- Rust + wry — child WebView windows (Markdown viewer, JSON/YAML data viewer, settings panel). Linux uses GTK + WebKitGTK, Windows uses WebView2 (no extra DLL needed)
- TypeScript (vanilla, no framework) — the child WebView frontends (`src-tauri/{viewer,settings}/web/`) and the shared web modules they import from (`src-tauri/web-shared/`)
- Bun — TypeScript bundler / test runner / package manager for the child WebView bundles only. The Rust binary embeds the bundles via `build.rs`

**Project type:** Desktop application (native Rust + child WebView windows; not Tauri-bundled)

**Key features:**
- Full ANSI control sequence support (parsed by `crates/term_core`)
- Kitty Graphics Protocol / SIXEL for inline images
- Custom OSC extension for Markdown / JSON / YAML rendering in child WebView windows
- mux: tmux-style multiplexing (windows / tabs / panes) inside one process
- Low-latency typing with a wgpu render pipeline driven by the winit event loop

## Why (Project Purpose)

A modern terminal emulator that combines traditional terminal reliability with rich content rendering. It displays images and formatted Markdown / JSON / YAML directly in the terminal via control sequences while keeping latency low.

**Design philosophy:**
- AI-first: built for the AI era, prioritizing compatibility with AI coding tools like Claude Code
- Explicit display commands only (no auto-detection)
- Stateless CLI design (works over SSH)
- Robust isolation (XSS protection in child WebViews, resource management)

## How (Development Workflow)

### Setup

```bash
bun install
make setup   # rustup target add x86_64-pc-windows-msvc + cargo install cargo-xwin
```

### Icon Generation

Requires `rsvg-convert` or `magick` (ImageMagick):
```bash
bash scripts/generate-icons.sh
```
Generates `src-tauri/icons/{32x32,128x128,128x128@2x}.png` from `assets/eMterm.svg`. Called automatically by `make dpkg`.

### Running the Project

**Development (Linux host, GUI):**
```bash
make dev          # bun run build:viewer + build:settings + cargo run
```

**Release build (Linux, GUI):**
```bash
make build        # CARGO_TARGET_DIR=src-tauri/target-host cargo build --release
```

**CLI-only build (no winit/wgpu/wry):**
```bash
make cli-build    # cargo build --release --no-default-features
```

**Windows cross-build:**
```bash
make win-build    # CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc
```

**deb packages:**
```bash
make dpkg         # build/emterm_<ver>_<arch>.deb (GUI, depends on libwebkit2gtk-4.1-0)
make cli-dpkg     # build/emterm-cli_<ver>_<arch>.deb (CLI only, depends on libc6)
```

### Build Layout

The `src-tauri/` crate uses dedicated `CARGO_TARGET_DIR` paths to keep the
fast debug + test cycle isolated from the release binary the user runs:

| Purpose                      | Target dir                  |
| ---------------------------- | --------------------------- |
| Quick check / unit tests     | `src-tauri/target`          |
| Release binary (Linux host)  | `src-tauri/target-host`     |
| Windows cross-build          | `src-tauri/target-win`      |

Always pass `--manifest-path src-tauri/Cargo.toml` and `CARGO_TARGET_DIR=<one of the above>` so concurrent sessions agree on where the binary lives. The release binary lives at `src-tauri/target-host/release/emterm` (or `.exe` on Windows).

### Testing & Verification

**Rust unit + integration tests** (default features):
```bash
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml
```

**CLI-only feature check** (verifies feature gates don't break):
```bash
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

**TypeScript (child WebView bundles):**
```bash
bun test            # uses test-setup.ts (happy-dom + i18n init)
bun run typecheck   # tsc --noEmit, scoped to src-tauri/{viewer,settings}/web
```

### Project Structure

```
src-tauri/                  - Rust crate (the `emterm` binary)
  Cargo.toml                - features: default=["gui"]; --no-default-features = CLI only
  build.rs                  - embeds viewer/dist + settings/dist when gui is on
  src/                      - Rust source
    main.rs                 - dispatch: cli subcommand vs --viewer / --image-viewer / --data-viewer / --settings / terminal
    lib.rs                  - module roster; GUI modules behind #[cfg(feature = "gui")]
    cli/                    - CLI subcommands (markdown / json / yaml / image)
    settings_core.rs        - CLI-shared (Language enum, settings_path)
    settings.rs             - GUI runtime settings (re-exports settings_core::*)
    {app,callbacks,render,ui,tabs,window_host,...}  - GUI-only modules
  viewer/web/               - Markdown / image / data viewer TypeScript entry
  viewer/dist/              - bun bundle output (gitignored)
  settings/web/             - settings panel TypeScript entry
  settings/dist/            - bun bundle output (gitignored)
  web-shared/               - TypeScript shared between viewer & settings web entries
                              (Markdown renderer, settings panel, i18n, etc.)
  assets/fonts/             - bundled Noto fonts (CJK, color emoji)
  icons/                    - PNGs generated by scripts/generate-icons.sh (gitignored)
  examples/                 - swash / font probes (required-features = ["gui"])
  tests/                    - integration tests (cli_subcommands.rs)
crates/
  app_settings              - settings.json schema (serde shape)
  term_core                 - ANSI parser + grid + Unicode width
  term_images               - Kitty / SIXEL decoders + APC/DCS parsers
  mux_ipc                   - mux protocol types
scripts/
  build-dpkg.sh             - deb packager (GUI or EMTERM_CLI_ONLY=1)
  generate-icons.sh         - SVG → PNG icons via rsvg-convert / magick
  measure-hidden-rss.sh     - RSS sampling helper
tmp/                        - temporary files and drafts (gitignored)
```

### Feature Gates

The `gui` feature (default-on) toggles the windowed terminal stack:

- **`gui` on** — full binary: winit + wgpu + egui terminal, wry child WebViews, mux/tabs/PTY, term_core/term_images/mux_ipc, font stack (swash/zeno/fontdb), bell/notifications/clipboard/SVG icon
- **`gui` off** (`--no-default-features`) — CLI only: just the `markdown` / `json` / `yaml` / `image` subcommands dispatched from `cli/`. The CLI deb (`emterm-cli`) ships this build and depends only on libc6

When you add a new module that uses GUI-only crates (winit, wgpu, wry, swash, etc.) declare it under `#[cfg(feature = "gui")]` in `src-tauri/src/lib.rs`. CLI-shared code should depend only on the always-built crates (serde / clap / image / app_settings / etc.).

### Logging

Logger output format: `[LEVEL] <message>` via `env_logger`. Frontend (child WebView) `console.*` calls are forwarded over the wry IPC channel and merged into the same backend log.

Log file (Linux): `~/.local/share/net.laser5.app.emterm/logs/emterm.log`. Release builds persist only `warn` and higher.

## CLI Commands

The application binary doubles as a CLI helper:
- `emterm` — launches the terminal (GUI build only)
- `emterm markdown <file>` — emit Markdown display sequence to stdout
- `emterm json <file>` — emit JSON display sequence to stdout
- `emterm yaml <file>` — emit YAML display sequence to stdout
- `emterm image <file> [--protocol kitty|sixel]` — emit image display sequence to stdout

**tmux support:** Inside tmux, CLI commands automatically wrap sequences in DCS passthrough (`ESC P tmux; ... ESC \`). Requires `set -g allow-passthrough on` in tmux config.
