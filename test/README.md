# Test Instructions for AI Agents

This document provides guidelines for AI agents when writing and executing tests
in the `emterm` repository.

## Test Framework

- **Rust**: built-in `cargo test` with `#[test]` / `#[cfg(test)]` modules.
  Workspace crates use the standard library test harness. Long-running
  benchmarks are gated with `#[ignore]` and invoked explicitly via
  `--include-ignored`.
- **TypeScript** (child WebView bundles only): `bun test` with
  `test-setup.ts` (happy-dom + i18n init).

There is no separate unit-test framework crate (no `proptest`, no `criterion`).
Keep new tests in the same style as the existing ones.

## Test Execution

### Unit Tests (Rust, primary)

Always set `CARGO_TARGET_DIR=src-tauri/target` and pass `--manifest-path`
so concurrent sessions agree on the build output location (see
`.claude/rules/build-location.md`).

```bash
# Run all library tests for the main binary crate
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml --lib

# Run library tests for a workspace crate (e.g. term_core)
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path crates/term_core/Cargo.toml --lib
```

Notes:

- Tests live in `--lib`; `--bin emterm` reports 0 tests
  (see MEMORY: `project_test_execution_notes`).
- The `tabs.rs` replay tests are non-deterministic when run in parallel.
  If they flake, re-run with `-- --test-threads=1`.

### Integration Tests (Rust)

```bash
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml --test cli_subcommands
```

`src-tauri/tests/` holds integration tests (`cli_subcommands.rs`,
`mux_throughput.rs`, `mux_hot_upgrade.rs`). Fixtures live under
`src-tauri/tests/fixtures/`.

#### `mux_hot_upgrade.rs` (mux daemon hot-upgrade)

End-to-end test for the mux daemon's in-place `execve` upgrade
(feature-docs/mux-daemon-hot-upgrade): spawns a real daemon in an isolated
`XDG_RUNTIME_DIR`, drives a real shell through it, triggers an upgrade over
the raw mux wire protocol, and asserts the shell's PID survives unchanged
and files it created remain observable, that a zero-pane upgrade succeeds,
that a successful upgrade logs a distinguishable handoff-start entry with
the adopted pane count, and that an upgrade rejected by the handoff-schema
probe leaves the original daemon serving with its pane still live. Unix
only (`#![cfg(unix)]`); every wait is bounded with a named timeout.

```bash
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1
```

`--test-threads=1` is required: scenarios spawn real daemon processes and
real PTYs, so serializing them avoids resource contention between
scenarios.

### Benchmarks (Rust, opt-in)

Performance benches are unit tests gated with `#[ignore]`. Invoke explicitly:

```bash
CARGO_TARGET_DIR=src-tauri/target cargo test --release \
  --manifest-path crates/term_core/Cargo.toml --lib \
  snapshot_replay_bench_2mib_seq -- --nocapture --include-ignored
```

### TypeScript Tests (child WebView bundles)

```bash
bun test            # uses test-setup.ts (happy-dom + i18n init)
bun run typecheck   # tsc --noEmit, scoped to src-tauri/{viewer,settings}/web
```

### E2E Tests

None at the moment. There is no `docker-compose.e2e.yml` and no
`e2e-tests/` directory. End-to-end behavior is validated manually by the
user (or via `cargo test --test mux_throughput` style integration tests).

## Test File Organization

```
src-tauri/
  src/<module>.rs         # Unit tests live under #[cfg(test)] mod tests {} inline
  tests/                  # Integration tests + fixtures
    cli_subcommands.rs
    mux_throughput.rs
    fixtures/
crates/
  <crate>/src/<module>.rs # Same inline-tests convention
  <crate>/src/bench.rs    # Opt-in #[ignore] benches (term_core has one)
```

Prefer inline `#[cfg(test)] mod tests {}` for unit tests next to the
code under test. Use `src-tauri/tests/` (top-level `tests/` dir in a
crate) only when you need a separate compilation unit (integration test).

## Writing Tests

### Test Naming Conventions

- `fn <subject>_<scenario>_<expected>()` is the dominant pattern in
  `crates/term_core/` (e.g. `cursor_show_interrupt_default_off`).
- Benches use `<subject>_bench_<size>_<shape>` (e.g.
  `snapshot_replay_bench_2mib_seq`).

### Test Structure

- Construct a `TerminalCore` (or other unit-under-test) explicitly per
  test — no shared global fixtures.
- Drive PTY-like input via `process_pty_data` / `build_from_snapshot`
  rather than typing escape sequences by hand when convenient.
- Assert on observable contracts: grid contents, `scrollback_evicted_total`,
  `get_scrollback_length()`, etc. Avoid asserting on internal-only state.

## Adding New Tests

1. Find the closest existing test in the same module and mirror its
   construction style.
2. If the test exercises a long-running performance path, gate it with
   `#[ignore]` and document the invocation comment as in
   `bench.rs::snapshot_replay_bench_2mib_seq`.
3. Run the full `--lib` suite at least once before considering the
   change done.

## Common Patterns

- **`SnapshotReplay` round-trip**: `build_from_snapshot` builds a fresh
  core; assertions then compare it to a synchronously-built core fed the
  same payload.
- **Bypass vs. non-bypass parity**: the `scrollback_bypass` contract
  keeps `scrollback_evicted_total` byte-identical between the two paths
  even though `scrollback_slim` is empty in the bypass case. Tests that
  cross this boundary should make the contract explicit.
- **`AtomicBool` cancellation**: `build_from_snapshot` takes a
  `&AtomicBool` cancel token. Tests that don't exercise cancellation
  pass a stack-local `AtomicBool::new(false)`.

## Project-Specific Constraints

- DevTools are NOT available. WebView code is debugged via the log file
  `~/.local/share/net.laser5.app.emterm/logs/emterm.log` (release
  persists `warn` and above only). See
  `.claude/rules/debugging-constraints.md`.
- Do not `cd` into `src-tauri/`. Stay at the project root and use
  `--manifest-path` (see `.claude/rules/build-location.md`).
