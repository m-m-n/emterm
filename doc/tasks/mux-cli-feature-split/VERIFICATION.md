# Verification Document: Mux in CLI Build

## Overview

**Feature**: mux-cli-feature-split (revised)

Verifies that the eMterm mux subsystem is included in the CLI build
(`cargo build --release --no-default-features`, `emterm-cli` deb)
without introducing a new cargo feature, make target, or deb package.

## Test Suites

### TS-1: GUI build still compiles

Command (from project root):

```
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: exit 0, zero warnings.

### TS-2: CLI build compiles

Command:

```
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

Expected: exit 0, zero warnings.

### TS-3: GUI tests pass

Command:

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
```

Expected: all tests pass (1911 / 1911 at the time of writing).

### TS-4: CLI tests pass

Command:

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib -- --test-threads=1
```

Expected: all tests pass (501 / 501 at the time of writing).

### TS-5: No `mux` feature in Cargo.toml

Command:

```
grep -nE '^mux = \[|"mux"' src-tauri/Cargo.toml
```

Expected: no matches.

### TS-6: No mux gate in lib.rs

Command:

```
grep -n 'cfg(feature = "mux")' src-tauri/src/lib.rs
```

Expected: no matches.

### TS-7: No mux feature gate in main.rs

Command:

```
grep -n 'cfg(feature = "mux")\|cfg(not(feature = "mux"))' src-tauri/src/main.rs
```

Expected: no matches.

### TS-8: Build-dpkg.sh shape

Command:

```
grep -n 'EMTERM_MUX\|MUX_ONLY\|HEADLESS\|emterm-mux' scripts/build-dpkg.sh
bash -n scripts/build-dpkg.sh
```

Expected: no matches; syntax check passes.

### TS-9: Makefile shape

Command:

```
grep -nE '^mux-(build|dpkg):' Makefile
```

Expected: no matches (`mux-build` and `mux-dpkg` are removed).

### TS-10: tmux_import stays GUI-gated

Command:

```
grep -n 'tmux_import' src-tauri/src/mux/mod.rs src-tauri/src/mux/cli.rs
```

Expected: `#[cfg(feature = "gui")]` precedes every `tmux_import`
reference outside of doc comments.

## Manual Verification (deferred)

Manual checks involving a real release build / SSH host / deb install
are outside the scope of this iteration. They include:

- M1: `make build` produces the GUI binary and `emterm` deb runs.
- M2: `make cli-build` produces the CLI binary and `./src-tauri/target-host/release/emterm mux --daemon` starts a daemon.
- M3: `make cli-dpkg` produces `emterm-cli_<ver>_<arch>.deb` whose
  `dpkg-deb --info` shows `Depends: libc6` only.
- M4: On a clean SSH host (libc6 only), installing the CLI deb and
  running `emterm mux --daemon` succeeds; `emterm mux attach` from
  another shell bridges in.

These remain user-driven (the orchestrator does not trigger release
builds without an explicit request).
