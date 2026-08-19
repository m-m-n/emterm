# Debugging Constraints

This app has specific constraints on how debugging information can be collected.
Follow these rules when investigating issues with the user.

## DevTools are NOT available

Do not instruct the user to:
- Open DevTools / inspect the DOM
- Evaluate expressions in the browser console of the child WebView windows
- Read `window.*` state interactively
- Set breakpoints in the browser

The native terminal has no WebView at all (it is wgpu+swash rendered), and the
child WebView windows (Markdown viewer, settings panel, data viewer) are
wry-hosted with DevTools disabled in release builds, so any instruction that
depends on them is unactionable.

## Log output

- Format: `[LEVEL] <message>`, emitted by the `crate::logging` module
  (`src-tauri/src/logging.rs`, env_logger backed).
- Child WebView `console.*` calls are forwarded over the wry IPC channel and
  merged into the same backend log.
- Log file (Linux): `~/.local/share/net.laser5.app.emterm/logs/emterm.log`
- Release builds persist only `warn` and higher. `log::debug!` / `log::info!`
  and `console.debug` / `console.log` / `console.info` are dropped.

## How to investigate

1. Read `emterm.log` directly (the path above) — do not ask the user "where is
   the log".
2. If existing log output is insufficient, add temporary `log::warn!` /
   `log::error!` (Rust) or `console.warn` / `console.error` (TS in the child
   WebView) diagnostics in the suspect code path. Levels below `warn` will not
   appear in release logs.
3. Have the user reproduce, then re-read the log.

Never say "open DevTools and check X" — always phrase investigation in terms of
log output.
