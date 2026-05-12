# term_core

Pure-Rust ANSI parser, terminal grid, scrollback ring buffer, and Unicode
processing core for eMterm. Created in Phase 2 of the `term-core-rust-crate`
SDD by lifting `wasm/src/` into this crate and stripping the wasm-bindgen
surface.

## Consumers

- `wasm/` — thin wrapper that re-exposes the API through wasm-bindgen for
  the existing TypeScript/Tauri build.
- `native-poc/` — Linux PoC native terminal binary (workspace member since
  Phase 6).
- Future native terminal targets — depend via `path` or `version`.

## API surface

The entry point is `term_core::terminal_core::TerminalCore`. Drive it with
`process_pty_data(&[u8])` and observe state via the `get_cell_*`,
`get_cursor_*`, `cols`, `rows`, and `resize` accessors.

Terminal-driven side effects (OSC titles, BEL, emterm OSC extension, etc.)
are delivered through the `term_core::callbacks::TerminalCallbacks` trait.
Native consumers provide their own implementation; the wasm wrapper provides
one backed by `js_sys::Function`.

## Dependencies

Pure Rust only:

- `serde` (with `derive`)
- `bincode`
- `log`
- `unicode-width`

No `wasm-bindgen`, `js-sys`, `web-sys`, or `serde-wasm-bindgen`.
