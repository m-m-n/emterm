# eMterm native-poc

Linux-only Proof-of-Concept terminal that combines a `tao` + `wgpu` + `egui`
native main window with on-demand `wry` Markdown viewer windows. The goal is to
gather Go/No-Go evidence for restructuring eMterm out of the Tauri/WebView
shell.

Since Phase 6 of the `term-core-rust-crate` SDD, this crate is a member of the
repository-root Cargo workspace and depends on `term_core` (the pure-Rust ANSI
parser + terminal grid extracted from `wasm/src/`) via a `path` dependency.
The Phase 1 stand-in modules (`src/parser/`, `src/grid/`) have been removed.

## Build

```sh
cargo build -p emterm-native-poc           # workspace-aware
# or the legacy form:
cargo build --manifest-path native-poc/Cargo.toml
```

## Run

```sh
RUST_LOG=info cargo run -p emterm-native-poc
```

## Test

```sh
cargo test -p emterm-native-poc
```

## Format

```sh
cargo fmt --all
```

## Known limits (PoC scope)

- Linux only (Ubuntu 22.04 family). No macOS, no Windows.
- No inline images (Kitty/SIXEL out of scope).
- No mux / no split panes.
- Existing `wasm/src/` parser is **not** reused; a minimal new parser lives in
  `src/parser/`.
- E2E suite (`e2e-tests/`) is unrelated and stays on the Tauri build.
