# Implementation Plan: Mux CLI Feature Split

## Overview

Refactor the `emterm` crate's Cargo feature topology to split the mux subsystem
(daemon / bridge / CLI / PTY) out of the `gui` feature into a standalone `mux`
feature. The default `gui` build remains bit-identical via `gui = ["mux", ...]`,
and `--no-default-features --features mux` becomes the new headless CLI+mux
build that powers a new `emterm-mux` deb package.

## Objectives

- Introduce a `mux` Cargo feature owning the mux/PTY runtime dependencies.
- Make `gui` require `mux` so existing GUI builds stay bit-identical.
- Re-gate `mux`, `pty`, and `scroll` module declarations from `feature = "gui"`
  to `feature = "mux"`.
- Lift the two shared symbols (`REPLAYABLE_VIEWER_KINDS`, `ScrollPosition`)
  that mux already references from `gui`-only modules into positions
  compilable under `mux` alone.
- Flip the `emterm mux …` dispatch in `main.rs` from `feature = "gui"` to
  `feature = "mux"`, with a neutral error message for non-mux builds.
- Add `make mux-build` and `make mux-dpkg` plus an `EMTERM_MUX_ONLY=1` branch
  in `scripts/build-dpkg.sh` to produce the new `emterm-mux` deb.

## Prerequisites

### Development Environment

- Rust toolchain (workspace default, edition 2024)
- `bun` for the GUI build path (viewer / settings web bundles); not needed
  for `make mux-build` or `make cli-build`
- `dpkg-deb` and standard Linux build tooling for deb packaging
- `cargo xwin` toolchain only for the Windows cross-build smoke check

### Dependencies

No new external crates. The optional dependency table in `src-tauri/Cargo.toml`
already marks every crate that this plan moves as `optional = true`. The work
is entirely structural — `[features]` reorganization plus a small number of
`cfg` rewrites and one symbol move.

## Architecture Overview

### Technology Stack

- **Language**: Rust (edition 2024, workspace-pinned toolchain)
- **Build system**: Cargo features + `make` wrappers + `scripts/build-dpkg.sh`
- **Key Cargo features**:
  - `default = ["gui"]` (preserved)
  - `gui = ["mux", ...gui-only deps...]` (rewritten)
  - `mux = [...mux/PTY deps...]` (NEW)

### Design Approach

**Three-tier feature topology.** The crate gains a middle tier between
`always-built CLI` and `full GUI`:

```
CLI-only            CLI + mux             GUI (default)
(no features)       (--features mux)      (default features)
─────────────       ─────────────────     ───────────────────
markdown/json/      markdown/json/        markdown/json/
yaml/image          yaml/image            yaml/image
                    emterm mux daemon     emterm mux daemon
                    emterm mux attach     emterm mux attach
                                          windowed terminal
                                          (winit + wgpu + wry)
```

`gui = ["mux", ...]` guarantees the GUI tier is a strict superset, so every
existing deployment keeps the same behavior.

**Four cross-feature symbols are hoisted out of the `gui`-only surface.**
The compile-blockers when `mux` runs without `gui` are:

1. `REPLAYABLE_VIEWER_KINDS` — currently in `viewer/mod.rs`, which stays
   `gui`-gated because `viewer` pulls in `wry`. Moved into a new always-built
   `viewer_kinds` module; `viewer/mod.rs` re-exports it.
2. `ScrollPosition` — a 23-line enum in `scroll.rs`, currently gated on `gui`
   only by happenstance. The module declaration in `lib.rs` is re-gated from
   `gui` to `mux`.
3. `wakeup` module — `pty/mod.rs:616` calls `crate::wakeup::wake()` to nudge
   the winit event loop after each PTY read. The module itself is winit-free
   (`OnceLock` + `Arc`), so the same `gui → mux` gate flip in `lib.rs` is
   enough; under the mux-only tier `wake()` stays a no-op because no event
   loop installs a wake function.
4. `self_exec` module — `mux/daemon.rs:154,210` calls
   `crate::self_exec::self_exe_path()` / `note_spawn_failure()` when spawning
   the daemon child. OS-API only (no GUI deps). Same `gui → mux` gate flip in
   `lib.rs`.

A fifth source of breakage lives in `#[cfg(test)]` only: the
`mux/prefix.rs` test module calls `crate::settings::parse_mux_action_chord`
in 12 places, but `settings` is `gui`-gated and unreachable under `--features
mux` alone. The fix is to rewrite those 12 test bodies to call
`crate::mux::prefix::parse_prefix_key` directly — `parse_mux_action_chord`
is just a thin wrapper over it. The GUI-side `parse_mux_action_chord`
function stays for production callers.

**`mux::tmux_import` stays GUI-only.** It writes to `settings_store`
(GUI-only). The fix is a one-line `cfg` on the submodule declaration, not a
refactor; its sole call site (`main.rs:run_gui`) is already under
`feature = "gui"`.

### Component Interaction

| From                        | Imports                       | Compiles when             |
| --------------------------- | ----------------------------- | ------------------------- |
| `mux::scrollback_filter`    | `crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS` (NEW path) | `feature = "mux"`         |
| `mux::window_group`         | `crate::scroll::ScrollPosition` | `feature = "mux"`         |
| `pty::mod` (PTY reader)     | `crate::wakeup::wake()`         | `feature = "mux"`         |
| `mux::daemon` (spawn child) | `crate::self_exec::self_exe_path()` / `note_spawn_failure()` | `feature = "mux"` |
| `viewer/mod.rs` (gui-only)  | re-exports `viewer_kinds::REPLAYABLE_VIEWER_KINDS` via `pub use` | `feature = "gui"` |
| `app.rs` (gui-only)         | `crate::scroll::ScrollPosition` | `feature = "gui"` (gui ⊃ mux) |
| `window_host.rs` (gui-only), status-bar providers | `crate::wakeup::WakeFn / shared_wake_fn / install` | `feature = "gui"` (gui ⊃ mux) |
| `app.rs`, `viewer/mod.rs`, `settings_launcher` (gui-only) | `crate::self_exec::*` | `feature = "gui"` (gui ⊃ mux) |
| GUI input pipeline (`ime`, `window_host`, `app`) | `crate::pty::input::Modifiers` | `feature = "gui"` (gui ⊃ mux) |
| `main.rs` mux dispatch      | `emterm::mux::cli::run`       | `feature = "mux"`         |
| `main.rs` `run_gui`         | `emterm::mux::tmux_import::…` | `feature = "gui"`         |

The transitive `gui ⊃ mux` relationship means no GUI call site changes — the
`mux` symbols stay reachable from GUI code without any explicit feature gate
on the call site.

## Implementation Phases

The phases are ordered so each one ends in a buildable, testable state. Phases
1–4 reshape internals (one symbol move, one `cfg` rewrite). Phase 5 introduces
the new build target. Phase 6 wires packaging. Phase 7 verifies all three
tiers + Windows cross-build.

---

### Phase 1: Hoist `REPLAYABLE_VIEWER_KINDS` to a CLI-shared module

**Goal**: Make the constant compilable without enabling the `gui` feature so
`mux::scrollback_filter` can keep using it under `--features mux`.

**Files to Create**:
- `src-tauri/src/viewer_kinds.rs` — small CLI-shared module declaring the
  constant.

**Files to Modify**:
- `src-tauri/src/lib.rs` — declare `pub mod viewer_kinds;` in the
  "CLI-shared modules" section (no `cfg`).
- `src-tauri/src/viewer/mod.rs` — replace the inline `pub const
  REPLAYABLE_VIEWER_KINDS` definition with `pub use
  crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;` to keep
  `crate::viewer::REPLAYABLE_VIEWER_KINDS` reachable for the GUI test that
  references it.
- `src-tauri/src/mux/scrollback_filter.rs` — change its import from
  `crate::viewer::REPLAYABLE_VIEWER_KINDS` to
  `crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS` (so it does not pull in the
  `gui`-gated `viewer` module).

**Key Components**:

| Component                                  | Responsibility                                                                                            | Precondition                                | Postcondition                                                   |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------- | ------------------------------------------- | --------------------------------------------------------------- |
| `viewer_kinds::REPLAYABLE_VIEWER_KINDS`    | Single source of truth for the four replayable viewer kinds (`markdown`, `image`, `json`, `yaml`).        | None.                                       | Reachable under any feature combination including no features. |
| `viewer::REPLAYABLE_VIEWER_KINDS` re-export | Backward-compat alias so GUI call sites keep compiling unchanged.                                         | `feature = "gui"` is on.                    | `crate::viewer::REPLAYABLE_VIEWER_KINDS` resolves to the new SSOT. |

**Processing Flow**:
1. New module hosts the constant unconditionally.
2. `viewer` module re-exports it for GUI consumers.
3. `mux` consumer switches to the new path so it is not transitively GUI-only.

**Implementation Steps**:
1. **Create `viewer_kinds.rs`** — declare `pub const REPLAYABLE_VIEWER_KINDS`
   with the same `&[&str]` value and a doc comment pointing to its callers.
2. **Wire into `lib.rs`** — add the module declaration alongside `cli`, `i18n`,
   `localtime`, `logging`, `settings_core` (no `cfg`).
3. **Re-export from `viewer/mod.rs`** — replace the inline definition with a
   `pub use` so the existing `viewer::REPLAYABLE_VIEWER_KINDS` path keeps
   working in the GUI test.
4. **Repoint `scrollback_filter.rs`** — change the import statement to the
   new path.

**Dependencies**: Foundational; nothing else depends on this phase being
deferred.

**Testing Approach**:
- Unit: existing `viewer/mod.rs` drift test (`drift_dispatch_kinds_match_ssot`)
  must keep passing because it asserts equality against the re-exported alias.
- Build: `cargo check --no-default-features` continues to succeed (the new
  module compiles standalone). `cargo check` with default features keeps
  building.

**Acceptance Criteria**:
- [ ] `cargo check --no-default-features --manifest-path src-tauri/Cargo.toml`
  succeeds and references the new module by virtue of declaring it.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` (default GUI)
  succeeds and the viewer drift test still passes.

**Estimated Effort**: small

---

### Phase 2: Add the `mux` feature in `Cargo.toml`

**Goal**: Introduce the `mux` feature and rewrite the `gui` feature list so
`gui = ["mux", ...gui-only deps...]`.

**Files to Modify**:
- `src-tauri/Cargo.toml` — `[features]` section only.

**Key Components**:

| Component        | Responsibility                                                                                       | Precondition                       | Postcondition                                                                      |
| ---------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------- |
| `[features].mux` | Enables `tokio`, `tokio-util`, `futures`, `chrono`, `anyhow`, `hostname`, `vt100`, `portable-pty`, `term_core`, `mux_ipc`. | All those crates marked `optional = true`. | `--features mux` produces a buildable feature set (compile errors land in Phase 3+). |
| `[features].gui` | Enables `mux` plus all windowed-terminal deps (winit, wgpu, egui, egui-wgpu, wry, swash, zeno, fontdb, ab_glyph, resvg, rodio, arboard, notify-rust, raw-window-handle, pollster, term_images, regex, unicode-width, unicode-segmentation, gtk, opener). | `mux` feature exists.              | `--features gui` (or default) is a strict superset of `--features mux`.            |
| `default`        | `["gui"]` — unchanged.                                                                               | `gui` feature exists.              | Default cargo invocations produce the same binary as before.                       |

**Processing Flow**:
1. Add `mux` feature listing only the runtime crates the mux daemon / bridge /
   PTY pipeline directly consume.
2. Rewrite `gui` so its first entry is `"mux"` and the mux-runtime crates are
   removed from its own list (they come in transitively).
3. Keep `term_images` in `gui` (it is only used by the GUI image renderer).
4. Confirm every moved crate already carries `optional = true` in
   `[dependencies]` — no `[dependencies]` rewrite expected.

**Implementation Steps**:
1. **Draft `mux` feature** — list the ten dep references from the
   specification's FR2.
2. **Rewrite `gui` feature** — prepend `"mux"`, drop the ten moved deps,
   keep the GUI-specific deps including `term_images`.
3. **Sanity-scan `[dependencies]`** — confirm each moved crate has
   `optional = true`; no edits expected, but capture any anomaly here.

**Dependencies**: Requires Phase 1 in tree (otherwise Phase 3's compile under
`--features mux` will hit `viewer` first). Blocks Phases 3–6.

**Testing Approach**:
- Build: at the end of this phase `cargo check --no-default-features
  --features mux` is expected to fail with a small, enumerable set of compile
  errors that Phase 3 fixes. Capture those errors here as the input set for
  Phase 3.
- Build: `cargo check` (default) and `cargo check --no-default-features` must
  still succeed (no regression while `mux` is unused).

**Acceptance Criteria**:
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` succeeds.
- [ ] `cargo check --no-default-features --manifest-path src-tauri/Cargo.toml`
  succeeds.
- [ ] `cargo check --no-default-features --features mux` produces only the
  expected compile errors (mux/pty modules not declared yet under
  `feature = "mux"`), not unrelated noise.

**Estimated Effort**: small

---

### Phase 3: Re-gate `mux`, `pty`, `scroll`, `wakeup`, `self_exec` modules + fix `mux::prefix` test refs

**Goal**: Make `mux`, `pty`, `scroll`, `wakeup`, and `self_exec` reachable under
`feature = "mux"`, and rewrite the `mux::prefix` test module so it no longer
references the `gui`-only `crate::settings::parse_mux_action_chord` helper.

**Files to Modify**:
- `src-tauri/src/lib.rs` — flip five `#[cfg(feature = "gui")]` gates to
  `#[cfg(feature = "mux")]`:
  - `pub mod mux;`
  - `pub mod pty;`
  - `pub mod scroll;`
  - `pub mod wakeup;` (consumed by `pty::mod` reader)
  - `pub mod self_exec;` (consumed by `mux::daemon` spawn)
- `src-tauri/src/mux/mod.rs` — gate the existing `pub mod tmux_import;`
  declaration with `#[cfg(feature = "gui")]`.
- `src-tauri/src/mux/prefix.rs` — rewrite the 12 `#[cfg(test)]` call sites
  that use `crate::settings::parse_mux_action_chord(spec)` to instead call
  `crate::mux::prefix::parse_prefix_key(spec)` (same return type, same
  parsing behavior). The non-test `crate::settings::parse_mux_action_chord`
  function and its production callers are not touched.

**Key Components**:

| Component                       | Responsibility                                                                                | Precondition                            | Postcondition                                                       |
| ------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------- | ------------------------------------------------------------------- |
| `crate::mux` module             | Mux daemon, bridge, CLI, IPC, session manager, snapshot, scrollback filter, dialog, prefix.   | `feature = "mux"`.                       | Reachable under both `mux` and `gui` (gui ⊃ mux).                   |
| `crate::pty` module             | PTY session, input encoder, ring buffer, visibility helpers, passthrough scanner.             | `feature = "mux"`.                       | Reachable under both `mux` and `gui`.                               |
| `crate::scroll` module          | Pure `ScrollPosition` value type (no transitive deps).                                        | `feature = "mux"`.                       | Compilable under `mux` alone; used by `mux::window_group` and `app`. |
| `crate::wakeup` module          | `OnceLock`-backed wake function; consumed by `pty::mod` reader and by GUI status-bar runtime / `window_host`. | `feature = "mux"`. | Reachable under both `mux` and `gui`. Under mux-only no event loop installs a wake function, so `wake()` stays a no-op. |
| `crate::self_exec` module       | OS-API helper that returns the current executable path; used by `mux::daemon` to spawn the daemon child, and by GUI launchers (`app`, `viewer`, `settings_launcher`). | `feature = "mux"`. | Reachable under both `mux` and `gui`. No winit/wgpu/wry deps. |
| `crate::mux::tmux_import` submod | One-shot `tmux.conf` auto-import; writes to GUI-only `settings_store`.                        | `feature = "gui"`.                       | Built only in the GUI tier; mux-only build does not see it.         |
| `mux::prefix` `#[cfg(test)]` body | Compiles under any feature that enables `mux`.                                              | `mux::prefix::parse_prefix_key` is `pub`.                                       | No `crate::settings` reference remains in the test body.            |

**Processing Flow**:
1. `lib.rs` widens the five `mux`/`pty`/`scroll`/`wakeup`/`self_exec`
   declarations to mux-gate.
2. `mux/mod.rs` narrows `tmux_import` so the GUI-only sub-module is not
   compiled in the mux-only tier.
3. `mux/prefix.rs` test bodies switch from `crate::settings::parse_mux_action_chord`
   to `crate::mux::prefix::parse_prefix_key` so they compile under
   `--features mux` (where `crate::settings` is not reachable).
4. After this phase, `cargo check --no-default-features --features mux`
   should reach the rest of `mux/*` cleanly. Any remaining compile error
   would indicate yet another undiscovered gui-only-symbol reference; the
   spec catalogs four module-level offenders + the prefix test refs, but a
   surprise found here should be documented inline.

**Implementation Steps**:
1. **Flip `lib.rs` gates** — change the `cfg` on the five module
   declarations (`mux`, `pty`, `scroll`, `wakeup`, `self_exec`) from `gui`
   to `mux`.
2. **Gate `tmux_import`** — add `#[cfg(feature = "gui")]` to the existing
   `pub mod tmux_import;` line in `mux/mod.rs`. Confirm the only caller
   (`main.rs:run_gui`) is already under `feature = "gui"`.
3. **Rewrite mux prefix tests** — replace each
   `crate::settings::parse_mux_action_chord(...)` with
   `crate::mux::prefix::parse_prefix_key(...)` inside the `#[cfg(test)]`
   module of `mux/prefix.rs`. Keep return-type expectations identical.
4. **Verify build** — run `cargo check --no-default-features --features mux`
   and resolve any unexpected compile error by gating the offending statement
   or pulling its dependency through `crate::viewer_kinds` / `crate::scroll`
   / `crate::wakeup` / `crate::self_exec` (all now mux-reachable).
5. **Verify test compile** — run `cargo test --no-default-features
   --features mux --no-run` to ensure the test binaries link under the
   mux-only feature combination.

**Dependencies**: Requires Phases 1–2. Blocks Phases 4–7.

**Testing Approach**:
- Build: `cargo check --no-default-features --features mux` succeeds.
- Build: `cargo check` (default GUI) still succeeds — the `gui ⊃ mux` chain
  brings in the same modules.
- Unit: `cargo test --no-default-features --features mux` runs; the test set
  is limited to what compiles under that feature combination (no GUI tests).

**Acceptance Criteria**:
- [ ] `cargo check --no-default-features --features mux` succeeds with no
  compile error.
- [ ] `cargo test --no-default-features --features mux --no-run` succeeds
  (test binaries link).
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` (default) still
  succeeds; no GUI module references break.
- [ ] `grep -n 'cfg(feature = "gui")' src-tauri/src/lib.rs` shows no entry
  for `mod mux`, `mod pty`, `mod scroll`, `mod wakeup`, or `mod self_exec`.
- [ ] `grep -n 'crate::settings::parse_mux_action_chord' src-tauri/src/mux/prefix.rs`
  returns nothing (all test refs migrated).

**Estimated Effort**: small

---

### Phase 4: Flip the `emterm mux` dispatch in `main.rs`

**Goal**: Route `emterm mux …` invocations whenever `mux` is built in, and
emit a neutral error otherwise.

**Files to Modify**:
- `src-tauri/src/main.rs` — within the existing
  `if sub == "mux" { … }` block:
  - Replace `#[cfg(feature = "gui")]` with `#[cfg(feature = "mux")]`.
  - Replace `#[cfg(not(feature = "gui"))]` with `#[cfg(not(feature = "mux"))]`.
  - Replace the existing error message
    (`emterm: \`mux\` is not available in this CLI-only build. / Install the GUI build (\`emterm\`) to use \`emterm mux\`.`)
    with the SPEC-mandated neutral phrasing
    (`emterm: \`mux\` is not available in this build. / Install a build that includes the \`mux\` feature (\`emterm\` or \`emterm-mux\`) to use \`emterm mux\`.`).
  - Also update the `use emterm::{... mux ...}` import statement at top of
    `main.rs` from `#[cfg(feature = "gui")]` to whatever keeps `mux` reachable
    only when the cfg arm needs it. Concretely: the existing `use emterm::{…
    mux, …};` line is `#[cfg(feature = "gui")]`; either split `mux` into its
    own `#[cfg(feature = "mux")]` `use` statement or rely on the fully-qualified
    `emterm::mux::cli::run` path inside the cfg arm and drop `mux` from the
    GUI-only `use` list. Pick the approach that minimizes diff noise.

**Key Components**:

| Component                                    | Responsibility                                                       | Precondition                          | Postcondition                                                                                |
| -------------------------------------------- | -------------------------------------------------------------------- | ------------------------------------- | -------------------------------------------------------------------------------------------- |
| `main` — `mux` arm under `feature = "mux"`   | Invokes `emterm::mux::cli::run`, propagates its exit code.           | Binary was built with `mux` enabled.  | `emterm mux …` works in both the GUI and CLI+mux binaries.                                   |
| `main` — `mux` arm under `not(feature = "mux")` | Prints the neutral error and exits 2.                                | Binary built without `mux`.           | CLI-only binary's behavior matches the existing `emterm-cli` error contract (same exit code 2). |

**Processing Flow**:
1. Subcommand dispatch unchanged for `markdown` / `json` / `yaml` / `image`.
2. `sub == "mux"` branch's cfg is widened to `feature = "mux"`.
3. Non-mux fallback emits a new neutral message and exits 2.

**Implementation Steps**:
1. **Rewrite the cfg gates** inside the existing `if sub == "mux"` block.
2. **Update the error message** to the SPEC-mandated wording.
3. **Adjust the `use emterm::{…}` import** so `mux` is reachable under the
   `mux` feature (not just `gui`). Prefer a fully-qualified call site to keep
   the diff localized.

**Dependencies**: Requires Phases 1–3 (mux module needs to be reachable under
`feature = "mux"`). Blocks Phase 7 smoke tests.

**Testing Approach**:
- Integration: building with `--no-default-features --features mux` produces
  a binary that responds to `emterm mux daemon` (smoke-tested in Phase 7).
- Integration: building with `--no-default-features` produces a binary whose
  `emterm mux daemon` invocation prints the new neutral message and exits 2.
- Manual: `tests/cli_subcommands.rs` does not currently cover the `mux`
  branch; we will not add a test that asserts on the specific error string,
  because the string is part of the user-facing contract and SPEC FR5 already
  pins it. If a future TS-5 is added in VERIFICATION it can assert exit code.

**Acceptance Criteria**:
- [ ] `cargo check --no-default-features` still succeeds and the binary's
  non-mux `emterm mux …` arm carries the new neutral message.
- [ ] `cargo check --no-default-features --features mux` succeeds and the
  mux arm calls `emterm::mux::cli::run`.
- [ ] `cargo check` (default GUI) succeeds.

**Estimated Effort**: small

---

### Phase 5: Add `make mux-build` and `make mux-dpkg` targets

**Goal**: Expose the new build at the developer-facing `make` layer.

**Files to Modify**:
- `Makefile` — add two targets, extend `.PHONY`, and update the `help` output.

**Key Components**:

| Component        | Responsibility                                                                                                    | Precondition                       | Postcondition                                                       |
| ---------------- | ----------------------------------------------------------------------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------- |
| `mux-build` target | Run `cargo build --release --no-default-features --features mux` against `CARGO_TARGET_HOST`.                    | Phases 1–4 landed.                  | Produces `src-tauri/target-host/release/emterm` (CLI+mux binary).   |
| `mux-dpkg` target  | Delegate to `EMTERM_MUX_ONLY=1 bash scripts/build-dpkg.sh`.                                                       | Phase 6 landed.                     | Produces `build/emterm-mux_<ver>_<arch>.deb`.                       |
| `.PHONY` list    | Declare both targets phony so `make` does not match a same-named file.                                            | None.                               | `make mux-build` / `make mux-dpkg` are robust to stray files.        |

**Processing Flow**:
1. `make mux-build` invokes cargo with the same `CARGO_TARGET_HOST` and
   `--manifest-path` conventions as `cli-build`.
2. `make mux-dpkg` is the deb-packaging counterpart that calls into the
   existing script with the new env var.

**Implementation Steps**:
1. **Define `mux-build`** — mirror the shape of `cli-build`, but pass
   `--no-default-features --features mux`. No `web` prerequisite (no viewer
   bundle in this tier).
2. **Define `mux-dpkg`** — invoke `EMTERM_MUX_ONLY=1 bash
   scripts/build-dpkg.sh`. The script handles cargo invocation internally.
3. **Add to `.PHONY`** — append `mux-build` and `mux-dpkg`.
4. **Update `help`** — the existing `awk` rule autogenerates help from
   `## …` comments on each target; supplying them on the new targets gets
   the entries for free.

**Dependencies**: Requires Phase 4 (so `make mux-build` produces a working
binary). `mux-dpkg` requires Phase 6.

**Testing Approach**:
- Manual: `make mux-build` produces a binary at the expected path.
- Manual: `make mux-dpkg` produces the expected `.deb` (after Phase 6).
- Build: `make build` and `make cli-build` continue to work unchanged.

**Acceptance Criteria**:
- [ ] `make mux-build` exits 0 and writes the binary to
  `src-tauri/target-host/release/emterm`.
- [ ] `make help` lists `mux-build` and `mux-dpkg`.

**Estimated Effort**: small

---

### Phase 6: Extend `scripts/build-dpkg.sh` with `EMTERM_MUX_ONLY` mode

**Goal**: Generate the new `emterm-mux` deb with the right `Depends`,
description, and skipped GUI assets (icons / desktop file / postinst-postrm).

**Files to Modify**:
- `scripts/build-dpkg.sh` — add an `EMTERM_MUX_ONLY` branch that mirrors the
  shape of the existing `EMTERM_CLI_ONLY` branch but builds with
  `--no-default-features --features mux`, names the package `emterm-mux`, and
  ships the new description string.

**Key Components**:

| Component                            | Responsibility                                                                                                  | Precondition                              | Postcondition                                                                |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------- | ----------------------------------------- | ---------------------------------------------------------------------------- |
| Mode detection                       | Recognize `EMTERM_MUX_ONLY=1`. When both `EMTERM_MUX_ONLY` and `EMTERM_CLI_ONLY` are set, MUX wins, CLI is ignored with a warning. | env vars are mutually exclusive (warn).   | Single packaging mode is selected per invocation.                            |
| Package metadata                     | `DEB_PACKAGE="emterm-mux"`, deb file `emterm-mux_<ver>_<arch>.deb`.                                             | mode is mux-only.                          | Output deb is named correctly.                                               |
| Build invocation                     | Run `cargo build --release --no-default-features --features mux` against `CARGO_TARGET_HOST`.                   | cargo is available.                       | Binary exists at the standard release path before packaging.                 |
| File layout                          | Copy binary + docs only (no `.desktop`, no icons, no postinst/postrm).                                          | mode is mux-only.                          | Same minimalist layout as the CLI-only deb.                                  |
| `DEBIAN/control`                     | `Package: emterm-mux`, `Section: utils`, `Depends: libc6`, description and command list per SPEC FR9.            | mode is mux-only.                          | `dpkg-deb --info` reports exactly that metadata.                             |

**Processing Flow**:
1. Parse `EMTERM_MUX_ONLY` and `EMTERM_CLI_ONLY` at script entry.
2. If both are set, log a warning, prefer mux-only, unset CLI-only locally.
3. Branch on the resolved mode to choose package name, build command, file
   layout, and control template.
4. After dpkg-deb succeeds, move the artifact to `build/`.

**Implementation Steps**:
1. **Add a `MUX_ONLY` variable** mirroring the existing `CLI_ONLY` shape.
2. **Resolve mode conflict** — when both env vars are set, emit a yellow
   warning and proceed as `MUX_ONLY`.
3. **Choose `DEB_PACKAGE`** — `emterm-mux` for mux-only, fall back to
   `emterm-cli` or `emterm` for the other modes.
4. **Branch the build command** — call cargo with
   `--no-default-features --features mux` in mux-only mode; the existing CLI
   and GUI branches remain.
5. **Skip GUI-only file plumbing** in mux-only mode, the same way the script
   already does for CLI-only mode (no icons, no `.desktop`, no postinst /
   prerm / postrm).
6. **Add a new control template** for mux-only with the description text
   SPEC FR9 specifies. Keep the existing `${VERSION}` / `${DEB_ARCH}` `sed`
   substitution.

**Dependencies**: Requires Phases 1–4 for the binary to build. Blocks `make
mux-dpkg` from being useful.

**Testing Approach**:
- Manual: `EMTERM_MUX_ONLY=1 bash scripts/build-dpkg.sh` produces
  `build/emterm-mux_<ver>_<arch>.deb`.
- Manual: `dpkg-deb --info build/emterm-mux_*.deb` reports `Package:
  emterm-mux`, `Section: utils`, `Depends: libc6`.
- Manual: `dpkg-deb --contents` shows no `.desktop`, no icons.
- Regression: `bash scripts/build-dpkg.sh` (default GUI) and
  `EMTERM_CLI_ONLY=1 bash scripts/build-dpkg.sh` (CLI-only) produce the
  same artifacts as before.

**Acceptance Criteria**:
- [ ] `EMTERM_MUX_ONLY=1 bash scripts/build-dpkg.sh` produces the expected
  `.deb`.
- [ ] `dpkg-deb --info` on the result reports `Depends: libc6` and no GTK /
  WebKit deps.
- [ ] Default GUI and `EMTERM_CLI_ONLY=1` runs are unchanged.

**Estimated Effort**: medium

---

### Phase 7: Cross-feature build + test sweep

**Goal**: Confirm all three tiers (CLI-only, CLI+mux, GUI) plus the Windows
cross-build still work, and that the test suite passes under each feature
combination the matrix exercises.

**Files to Modify**: none (this phase is verification only).

**Key Components**:

| Component                | Responsibility                                                                                                  | Precondition         | Postcondition                                                  |
| ------------------------ | --------------------------------------------------------------------------------------------------------------- | -------------------- | -------------------------------------------------------------- |
| Build matrix             | Run `cargo build --release` and `cargo check` in all three feature combinations.                                | Phases 1–6 landed.   | All three binaries are produced; CLI-only is byte-comparable.  |
| Test matrix              | Run `cargo test` under default, no-default, and `mux` features. Library tests, plus `tests/cli_subcommands.rs`. | Phases 1–6 landed.   | All test invocations exit 0.                                   |
| Windows cross-build      | `cargo xwin build --release --target x86_64-pc-windows-msvc` (default features).                                | xwin toolchain ready. | The Windows GUI binary still builds.                          |
| Deb sanity scan          | `dpkg-deb --info` and `--contents` on all three debs.                                                           | Phase 6 landed.      | Each deb matches its expected `Depends` line and file layout. |

**Processing Flow**:
1. Run each cargo command in the matrix; record exit codes.
2. For each deb, run `dpkg-deb --info` / `--contents` and compare with the
   expected shape recorded in VERIFICATION.md.
3. Grep `lib.rs` to confirm no `feature = "gui"`-only gate remains in front
   of `mod mux`, `mod pty`, or `mod scroll`.

**Implementation Steps**:
1. **Run the build matrix** described in VERIFICATION.md §"Build
   Verification".
2. **Run the test matrix** described in VERIFICATION.md §"Test Verification".
3. **Inspect the three debs** as described in VERIFICATION.md §"Packaging
   Verification".
4. **Document any deviation** (e.g. CLI-only test count differs from
   pre-task baseline) inline in VERIFICATION.md's result section once the
   implement step has run.

**Dependencies**: Requires Phases 1–6.

**Testing Approach**: This phase IS the test phase; see VERIFICATION.md for
the full table.

**Acceptance Criteria**:
- [ ] Every cell in the build / check / test matrix exits 0.
- [ ] `cargo xwin build --release --target x86_64-pc-windows-msvc` exits 0.
- [ ] `dpkg-deb --info build/emterm-mux_*.deb` shows `Depends: libc6` only.
- [ ] `grep -n 'cfg(feature = "gui")' src-tauri/src/lib.rs` shows no entry
  for `mod mux`, `mod pty`, `mod scroll`, `mod wakeup`, or `mod self_exec`.

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/
├── Cargo.toml                          (Phase 2: rewritten [features])
├── build.rs                            (unchanged — CARGO_FEATURE_GUI gate stays)
├── src/
│   ├── lib.rs                          (Phase 1: + viewer_kinds; Phase 3: mux/pty/scroll/wakeup/self_exec gates)
│   ├── main.rs                         (Phase 4: mux dispatch cfg flip + message)
│   ├── viewer_kinds.rs                 NEW (Phase 1)
│   ├── viewer/
│   │   └── mod.rs                      (Phase 1: re-export REPLAYABLE_VIEWER_KINDS)
│   ├── scroll.rs                       (unchanged contents; gate flipped in lib.rs)
│   ├── wakeup.rs                       (unchanged contents; gate flipped in lib.rs)
│   ├── self_exec.rs                    (unchanged contents; gate flipped in lib.rs)
│   ├── mux/
│   │   ├── mod.rs                      (Phase 3: tmux_import gated to gui)
│   │   ├── prefix.rs                   (Phase 3: tests call parse_prefix_key directly)
│   │   └── scrollback_filter.rs        (Phase 1: import from viewer_kinds)
│   └── pty/                            (unchanged contents; gate flipped in lib.rs)
├── tests/
│   └── cli_subcommands.rs              (unchanged)
Makefile                                (Phase 5: + mux-build, mux-dpkg)
scripts/
└── build-dpkg.sh                       (Phase 6: EMTERM_MUX_ONLY branch)
doc/tasks/mux-cli-feature-split/
├── SPEC.md
├── 要件定義書.md
├── IMPLEMENTATION.md                   (this file)
├── VERIFICATION.md
├── tasks.yaml
└── sdd.yaml                            (updated requirements.{ID}.tasks/tests)
```

## Testing Strategy

This task is structural (Cargo feature topology + a one-symbol move + cfg
rewrites). Most of the verification is at the build / packaging level, not
the unit-test level.

- **Unit tests**: existing tests must keep passing under default features.
  Phase 1's `pub use` re-export keeps the `viewer::REPLAYABLE_VIEWER_KINDS`
  drift test passing. Phase 3's `mux`/`pty` re-gate exposes the same module
  tree under both `mux` and `gui`, so the existing mux/pty tests now run
  under both `cargo test` and `cargo test --no-default-features --features mux`.
- **Integration tests**: `tests/cli_subcommands.rs` keeps working unchanged.
- **Build matrix**: the primary verification (see Phase 7 + VERIFICATION.md).
- **E2E**: out of scope — this project has no E2E suite for the native
  build, and the user-facing "SSH a host, install the deb, run `emterm mux
  daemon`" flow is deferred to manual / sdd.6-verify time.

## Dependencies

| Package           | Version | Purpose                                                          |
| ----------------- | ------- | ---------------------------------------------------------------- |
| (no new packages) | —       | This task only reorganizes existing `optional = true` deps.       |

## Risk Assessment

| Risk                                                                                                                | Likelihood | Impact | Mitigation                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------- | ---------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A mux module silently depends on a GUI-only symbol we have not catalogued (beyond `REPLAYABLE_VIEWER_KINDS`, `ScrollPosition`, `wakeup`, `self_exec`, and the `mux::prefix` test refs). | low        | medium | Phase 3 ends with `cargo check --no-default-features --features mux` AND `cargo test --no-default-features --features mux --no-run`; any new compile error there is surfaced and resolved before Phase 4. The spec catalogs all four module-level offenders plus the test refs as the known offenders. |
| A test that lives in `src/mux/` was implicitly GUI-only because the module was GUI-only, and now fails under `--features mux` due to a GUI dependency in its test body. | low        | low    | `cargo test --no-default-features --features mux` in Phase 7 surfaces this; the fix is to gate the offending `#[cfg(test)]` helper on `feature = "gui"` rather than re-gating the production code.                                                          |
| `EMTERM_MUX_ONLY` and `EMTERM_CLI_ONLY` set simultaneously by a CI matrix.                                          | low        | low    | Phase 6 emits a warning and picks mux; CI scripts can be audited at integration time.                                                                                                                                                                       |
| Build cost regression in the GUI tier because we accidentally moved a crate `gui` actually needs but `mux` does not. | low        | medium | The list in FR2 is exact and limited to mux/PTY runtime crates. Phase 2's `cargo check` (default) confirms the GUI still builds.                                                                                                                              |
| Windows cross-build (`cargo xwin`) breaks because the new feature interactions confuse target-specific dep tables.   | low        | medium | Phase 7 explicitly includes the Windows cross-build smoke check. The `[target.'cfg(...)']` sections in `Cargo.toml` are untouched by this task.                                                                                                                |

## Open Questions

- [ ] None — SPEC.md §Open Questions confirms all TBDs have been resolved
  during clarification.

## Success Metrics

- [ ] `cargo build --release` (default GUI) is bit-comparable to the
  pre-task GUI build in terms of dependency tree and observed behavior.
- [ ] `cargo build --release --no-default-features` produces the same CLI
  binary as before.
- [ ] `cargo build --release --no-default-features --features mux` produces
  a working `emterm` binary that runs `emterm mux daemon`.
- [ ] `make mux-dpkg` produces `emterm-mux_<ver>_<arch>.deb` with
  `Depends: libc6` and no GTK/WebKit references.
- [ ] `cargo test` passes under default, no-default, and `--features mux`.
- [ ] `cargo xwin build --release --target x86_64-pc-windows-msvc` succeeds.

## Requirement → Phase Coverage

| Requirement | Phase(s)        | Verification (TS)     |
| ----------- | --------------- | --------------------- |
| FR1         | Phase 2         | TS-1, TS-2, TS-3      |
| FR2         | Phase 2         | TS-1, TS-2, TS-3      |
| FR3         | Phase 3         | TS-3, TS-12           |
| FR4         | Phase 3         | TS-3, TS-6            |
| FR5         | Phase 4         | TS-3, TS-8, TS-9      |
| FR6         | Phase 1, Phase 3 | TS-3, TS-6, TS-10, TS-12 |
| FR6.1       | Phase 3         | TS-10, TS-13          |
| FR7         | Phase 3         | TS-3, TS-6            |
| FR8         | Phase 5         | TS-10                 |
| FR9         | Phase 6         | TS-10, TS-11          |
| NFR1        | Phase 1–6       | TS-1, TS-2, TS-4, TS-5, TS-11 |
| NFR2        | Phase 2, Phase 3 | TS-3 (qualitative)    |
| NFR3        | Phase 2         | TS-1, TS-3            |
| NFR4        | Phase 6         | TS-11                 |
