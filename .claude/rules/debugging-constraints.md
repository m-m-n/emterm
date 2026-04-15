# Debugging Constraints

This Tauri app has specific constraints on how debugging information can be collected. Follow these rules when investigating issues with the user.

## DevTools are NOT available

Do not instruct the user to:
- Open DevTools / inspect the DOM
- Evaluate expressions in the browser console
- Read `window.*` state interactively
- Set breakpoints in the browser

The Tauri build used by the user does not expose DevTools, so any instruction that depends on them is unactionable.

## Use the log file instead

- Log file path (Linux): `~/.local/share/net.laser5.app.emterm/logs/emterm.log`
- Produced by the Tauri (Rust) logging plugin. Frontend `console.*` calls are forwarded to the backend and merged into the same file.
- Release builds persist only `warn` and higher. `console.debug` / `console.log` / `console.info` are dropped.

## How to investigate

1. Read `emterm.log` directly (the path above) — do not ask the user "where is the log".
2. If existing log output is insufficient, add temporary `console.warn` / `console.error` diagnostics in the suspect code path. `console.debug` / `console.log` will not appear in release logs.
3. Have the user reproduce, then re-read the log.

Never say "open DevTools and check X" — always phrase investigation in terms of log output.
