# Implementation Plan: Mux in CLI Build

## Overview

Make the eMterm mux subsystem part of the CLI build by demoting it from
its short-lived dedicated `mux` cargo feature to "always built". The
mux subsystem has no GUI dependency, so no feature gate is needed.

This plan also reverts the build-infrastructure additions made by the
prior `feat(mux): split mux into standalone cargo feature` commit so
the public surface (`make` targets, deb packages, env vars) returns to
its pre-split shape.

## Phase 1: Cargo.toml feature reshape

**Files:** `src-tauri/Cargo.toml`

- Remove the `mux = [...]` feature block introduced by the prior commit.
- Remove `"mux"` from the `gui = [...]` feature list.
- Move the following dependencies out of `optional = true` so they are
  always built: `tokio`, `tokio-util`, `futures`, `chrono`, `anyhow`,
  `hostname`, `vt100`, `portable-pty`, `term_core`, `mux_ipc`.

**Acceptance:**
- `cargo check` (default features) compiles.
- `cargo check --no-default-features` compiles.

## Phase 2: lib.rs gate removal

**Files:** `src-tauri/src/lib.rs`

- Remove the `#[cfg(feature = "mux")]` gate above `pub mod mux`,
  `pub mod pty`, `pub mod scroll`, `pub mod wakeup`, and
  `pub mod self_exec`.

**Acceptance:**
- The above modules are reachable in both GUI and CLI builds.

## Phase 3: main.rs dispatch flip

**Files:** `src-tauri/src/main.rs`

- Remove the `#[cfg(feature = "mux")]` and `#[cfg(not(feature = "mux"))]`
  arms around `if sub == "mux"`. The mux dispatch always proceeds to
  `emterm::mux::cli::run`.
- Restore `mux` to the `#[cfg(feature = "gui")] use emterm::{...}` list
  so `run_gui` can use the unqualified `mux::tmux_import::...` path.
- Remove the fallback error message
  (`"emterm: \`mux\` is not available in this build."`).

**Acceptance:**
- `emterm mux <args>` dispatches to `mux::cli::run` in every build.

## Phase 4: `mux::tmux_import` stays GUI-only

**Files:** `src-tauri/src/mux/mod.rs`, `src-tauri/src/mux/cli.rs`

- Keep the existing `#[cfg(feature = "gui")] pub mod tmux_import;` in
  `mux/mod.rs`.
- Keep the existing `#[cfg(feature = "gui")] use ...` and call-site
  gates in `mux/cli.rs::execute_mux`.

**Acceptance:**
- CLI build does not reference `settings_store` (which is GUI-only)
  through `tmux_import`.

## Phase 5: Revert build-infrastructure additions

**Files:** `Makefile`, `scripts/build-dpkg.sh`

- `Makefile`: delete the `mux-build` and `mux-dpkg` PHONY entries and
  targets. Update the `cli-build` and `cli-dpkg` help strings to note
  that the CLI build now includes mux.
- `scripts/build-dpkg.sh`:
  - Delete the `EMTERM_MUX_ONLY` env var detection and its associated
    branches in the build / package-name / control-file selection.
  - Delete the `HEADLESS` variable; restore the previous
    `if [ -z "$CLI_ONLY" ]` guards on GUI-asset blocks.
  - Delete the `Conflicts:` / `Replaces:` lines from all three deb
    control stanzas (only two stanzas remain: `emterm`, `emterm-cli`).
  - Update the `emterm-cli` deb Description body to list the mux
    subcommands alongside the existing image / markdown / json / yaml
    entries.

**Acceptance:**
- `make` recipe surface is back to `build` / `cli-build` / `dpkg` /
  `cli-dpkg`.
- `bash -n scripts/build-dpkg.sh` passes.
- Building either deb does not require any new env var.

## Phase 6: `viewer_kinds` SSOT (kept from prior commit)

**Files:** `src-tauri/src/viewer_kinds.rs`,
`src-tauri/src/viewer/mod.rs`, `src-tauri/src/mux/scrollback_filter.rs`

- The new `viewer_kinds.rs` SSOT file, the `pub use` re-export in
  `viewer/mod.rs`, and the `mux::scrollback_filter` import switch
  introduced by the prior commit are retained as-is. Without them, the
  CLI build would still fail to link `scrollback_filter` because the
  original `REPLAYABLE_VIEWER_KINDS` lived in the GUI-only `viewer`
  module.

## Phase 7: `mux::prefix` test fix (kept from prior commit)

**Files:** `src-tauri/src/mux/prefix.rs`

- The test-side rewrite from `crate::settings::parse_mux_action_chord`
  to `crate::mux::prefix::parse_prefix_key` is retained, so
  `cargo test --no-default-features --lib` exercises the prefix code
  without depending on the GUI `settings` module.

## Verification

After all phases:

1. `cargo check` (GUI) → exit 0
2. `cargo check --no-default-features` (CLI) → exit 0
3. `cargo test --lib -- --test-threads=1` (GUI) → all pass
4. `cargo test --no-default-features --lib -- --test-threads=1` (CLI)
   → all pass
5. `bash -n scripts/build-dpkg.sh` → exit 0
6. `grep -n 'mux\b' Makefile` → only `cli-build` / `cli-dpkg` help text
   references mux; no `mux-build` / `mux-dpkg` targets remain.
7. `grep -n 'EMTERM_MUX\|MUX_ONLY\|emterm-mux' scripts/build-dpkg.sh`
   → no matches.
8. `grep -n '^mux = \[\|"mux"' src-tauri/Cargo.toml` → no matches
   (no `mux` feature, no `"mux"` in the `gui` feature list).

## Rollback

If any phase regresses behavior, `git restore` the affected file. The
change set is small enough that per-file restore is sufficient.
