# Architecture

Desktop application: a native Rust binary (winit + wgpu + egui) that spawns
child WebView windows via wry. Not Tauri-bundled, despite the `src-tauri/`
directory name.

## Technology stack

- **Rust** — native terminal stack: winit (event loop, IME), wgpu (GPU surface),
  egui (in-process UI), swash + zeno + fontdb (font rasterization), portable-pty
  (PTY abstraction)
- **Rust + wry** — child WebView windows (Markdown viewer, JSON/YAML data
  viewer, settings panel). Linux uses GTK + WebKitGTK, Windows uses WebView2
  (no extra DLL needed)
- **TypeScript** (vanilla, no framework) — the child WebView frontends
  (`src-tauri/{viewer,settings}/web/`) and the shared web modules they import
  from (`src-tauri/web-shared/`)
- **Bun** — TypeScript bundler / test runner / package manager for the child
  WebView bundles only. The Rust binary embeds the bundles via `build.rs`

## Feature gates

The `gui` feature (default-on) toggles the windowed terminal stack:

- **`gui` on** — full binary: winit + wgpu + egui terminal, wry child WebViews,
  mux/tabs/PTY, term_core/term_images/mux_ipc, font stack (swash/zeno/fontdb),
  bell/notifications/clipboard/SVG icon
- **`gui` off** (`--no-default-features`) — CLI only: just the `markdown` /
  `json` / `yaml` / `image` subcommands dispatched from `cli/`. The CLI deb
  (`emterm-cli`) ships this build and depends only on libc6

When you add a module that uses GUI-only crates (winit, wgpu, wry, swash, etc.)
declare it under `#[cfg(feature = "gui")]` in `src-tauri/src/lib.rs`. CLI-shared
code depends only on the always-built crates (serde / clap / image /
app_settings / etc.).

## Platform support

Linux and Windows. macOS is out of scope.
