# Verification Document: Mux CLI Feature Split

## Overview

**Feature**: mux-cli-feature-split
**SPEC.md**: `doc/tasks/mux-cli-feature-split/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-cli-feature-split/IMPLEMENTATION.md`

This task is a structural refactor (Cargo features + a handful of `cfg`
flips + one symbol move). Most verification is at the build / packaging
level rather than the unit-test level. The matrix below crosses the three
build tiers (CLI-only, CLI+mux, GUI) against the verifiable surfaces (build,
test, binary behavior, deb shape).

## Build Verification

### Default GUI build (regression check)

- Command: `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release
  --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0; binary at
  `src-tauri/target-host/release/emterm`; behavior unchanged from before
  this task.

### CLI-only build (regression check)

- Command: `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release
  --no-default-features --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0; behavior unchanged from before this task,
  including the existing "this build provides only CLI subcommands" error
  for `emterm` with no subcommand, and the SPEC-mandated neutral
  "`mux` is not available in this build" error for `emterm mux …`.

### CLI+mux build (new)

- Command: `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release
  --no-default-features --features mux --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0; binary at
  `src-tauri/target-host/release/emterm`; the binary handles
  `emterm mux daemon` and `emterm mux attach`.

### Windows cross-build (regression check)

- Command: `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build
  --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0; binary at
  `src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe`.

### `cargo check` matrix (fast iteration)

- `CARGO_TARGET_DIR=src-tauri/target cargo check
  --manifest-path src-tauri/Cargo.toml`
- `CARGO_TARGET_DIR=src-tauri/target cargo check
  --manifest-path src-tauri/Cargo.toml --no-default-features`
- `CARGO_TARGET_DIR=src-tauri/target cargo check
  --manifest-path src-tauri/Cargo.toml --no-default-features --features mux`
- `CARGO_TARGET_DIR=src-tauri/target cargo check
  --manifest-path src-tauri/Cargo.toml --no-default-features --features gui`
  (equivalent to default; orthogonality smoke check for NFR3)
- Expected: all four exit 0 with no compile errors and no new warnings
  beyond pre-existing ones.

### Actual results (Phase 3 sdd.4-implement, 2026-06-23)

| Command (cargo check)                                                                | Result    |
| ------------------------------------------------------------------------------------ | --------- |
| default GUI                                                                          | exit 0    |
| `--no-default-features` (CLI-only)                                                   | exit 0    |
| `--no-default-features --features mux` (CLI+mux, new)                                | exit 0    |
| `--no-default-features --features gui` (orthogonality, equivalent to default)        | exit 0    |

No new warnings observed beyond the pre-existing ones. `cargo build
--release` matrix is deferred to sdd.6-verify per project policy
("リリースビルドを勝手に走らせない").

## Test Verification

- Command (default GUI): `CARGO_TARGET_DIR=src-tauri/target cargo test
  --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only): `CARGO_TARGET_DIR=src-tauri/target cargo test
  --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (CLI+mux, new): `CARGO_TARGET_DIR=src-tauri/target cargo test
  --manifest-path src-tauri/Cargo.toml --no-default-features --features mux`
- Coverage: no new coverage targets — existing tests stay green. The
  CLI+mux invocation MUST cover the mux/PTY unit tests that previously
  ran only under default features.

### Actual results (Phase 3 sdd.4-implement, 2026-06-23)

| Invocation                                                                                                            | Lib tests passed   | Integration test (`cli_subcommands`) |
| --------------------------------------------------------------------------------------------------------------------- | ------------------ | ------------------------------------ |
| `cargo test --lib -- --test-threads=1` (default GUI)                                                                  | 1911 / 1911 (3 ignored) | 12 / 12                              |
| `cargo test --no-default-features --features mux --lib -- --test-threads=1` (CLI+mux, new)                            | 501 / 501 (2 ignored)   | 12 / 12                              |
| `cargo test --no-default-features --test cli_subcommands` (CLI-only)                                                  | — (lib not run; no mux/pty tests reachable in this tier)            | 12 / 12                              |

Notes:

- `tabs::tests::ts*` replay tests are known to be non-deterministic under
  parallel execution; the default-GUI lib invocation was forced to
  `--test-threads=1` per the project memory entry. With single-threaded
  execution all 1911 lib tests pass.
- `mux_throughput.rs` integration test is marked `#[ignore]` (spawns a
  real daemon process) and is reported as ignored, unchanged from before
  this task.
- The "Lib tests passed" delta between default GUI (1911) and CLI+mux
  (501) reflects the GUI-only tests (`viewer`, `app`, `ui`, `tabs`, etc.)
  not being reachable under `--features mux` alone, which is the
  intended outcome of FR3 / FR6.

### Test Scenarios from SPEC.md

| ID    | Scenario                                                                                                       | Expected Result                                                                                                              | Test Type             |
| ----- | -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| TS-1  | `cargo check` (default) succeeds                                                                               | exit 0, no new warnings                                                                                                      | Build (cargo check)   |
| TS-2  | `cargo check --no-default-features` succeeds                                                                   | exit 0, no new warnings                                                                                                      | Build (cargo check)   |
| TS-3  | `cargo check --no-default-features --features mux` succeeds                                                    | exit 0, no compile errors, no new warnings                                                                                   | Build (cargo check)   |
| TS-4  | `cargo build --release` (default GUI) succeeds and the binary still launches the windowed terminal             | exit 0; binary at `src-tauri/target-host/release/emterm` (or `.exe`)                                                          | Build (cargo build)   |
| TS-5  | `cargo build --release --no-default-features` (CLI-only) succeeds and behavior is unchanged                    | exit 0; `emterm mux daemon` prints the new neutral error and exits 2                                                          | Build + smoke (manual) |
| TS-6  | `cargo build --release --no-default-features --features mux` succeeds                                          | exit 0; binary responds to `emterm mux daemon` and `emterm mux attach`                                                       | Build + smoke (manual) |
| TS-7  | `cargo xwin build --release --target x86_64-pc-windows-msvc` (default) succeeds                                | exit 0; `emterm.exe` produced                                                                                                | Build (cross-build)   |
| TS-8  | `cargo test` (default) passes                                                                                  | exit 0                                                                                                                       | Unit / integration    |
| TS-9  | `cargo test --no-default-features` (CLI-only) passes                                                           | exit 0                                                                                                                       | Unit / integration    |
| TS-10 | `cargo test --no-default-features --features mux` (CLI+mux) passes                                             | exit 0; mux/PTY unit tests now run in this combination                                                                       | Unit / integration    |
| TS-11 | `make dpkg` and `make cli-dpkg` produce unchanged debs; `make mux-dpkg` produces a libc6-only `emterm-mux` deb | three deb files in `build/`; `dpkg-deb --info` reports the expected `Depends` lines (libc6+webkit/gtk; libc6; libc6)            | Packaging (manual)    |
| TS-12 | `lib.rs` contains no `feature = "gui"` gate in front of `mod mux`, `mod pty`, `mod scroll`, `mod wakeup`, or `mod self_exec` | `grep -n 'cfg(feature = \"gui\")' src-tauri/src/lib.rs` shows no entry on the lines immediately preceding those five declarations | Static grep           |
| TS-13 | `mux/prefix.rs` test bodies no longer reference `crate::settings::parse_mux_action_chord`                       | `grep -n 'crate::settings::parse_mux_action_chord' src-tauri/src/mux/prefix.rs` returns no match                              | Static grep           |

## Code Quality Verification

- Format: `cargo fmt --all` (project policy: rustfmt enforced via
  `style_edition = 2024`).
- Static analysis: none beyond `cargo check` (no clippy gate in this
  repo); the existing `cargo check` invocations are the static analysis
  bar.

### Actual results (Phase 3 sdd.4-implement, 2026-06-23)

- Per the project memory entry "cargo fmt をクレート全体に走らせない",
  `rustfmt --edition 2024` was applied only to the 8 source files this
  task modified (`viewer_kinds.rs`, `lib.rs`, `viewer/mod.rs`,
  `mux/scrollback_filter.rs`, `mux/mod.rs`, `mux/prefix.rs`,
  `mux/cli.rs`, `main.rs`). No format changes were required.
- Static grep (TS-12, TS-13):
  - `grep` over `src-tauri/src/lib.rs` for `cfg(feature = "gui")` shows
    no entry preceding `mod mux`, `mod pty`, `mod scroll`, `mod wakeup`,
    or `mod self_exec` — all five carry `cfg(feature = "mux")` as
    required.
  - `grep -n 'crate::settings::parse_mux_action_chord'
    src-tauri/src/mux/prefix.rs` returns no match (exit 1) — all 12 test
    call sites were migrated to `crate::mux::prefix::parse_prefix_key`.

## File Structure Verification

### Files to Create

- [x] `src-tauri/src/viewer_kinds.rs` — CLI-shared module hosting
  `REPLAYABLE_VIEWER_KINDS`.

### Files to Modify

- [x] `src-tauri/Cargo.toml` — `[features]` rewrite: new `mux`, `gui`
  requires `mux`.
- [x] `src-tauri/src/lib.rs` — declare `viewer_kinds`; gate `mux`, `pty`,
  `scroll`, `wakeup`, `self_exec` on `feature = "mux"`.
- [x] `src-tauri/src/main.rs` — flip `mux` dispatch cfg; update error message.
- [x] `src-tauri/src/mux/mod.rs` — gate `pub mod tmux_import;` on `feature =
  "gui"`.
- [x] `src-tauri/src/mux/prefix.rs` — rewrite 12 `#[cfg(test)]` call sites
  from `crate::settings::parse_mux_action_chord` to
  `crate::mux::prefix::parse_prefix_key`.
- [x] `src-tauri/src/mux/scrollback_filter.rs` — import
  `REPLAYABLE_VIEWER_KINDS` from `crate::viewer_kinds`.
- [x] `src-tauri/src/viewer/mod.rs` — replace the inline `REPLAYABLE_VIEWER_KINDS`
  definition with a `pub use crate::viewer_kinds::…` re-export.
- [x] `Makefile` — add `mux-build` / `mux-dpkg` targets and update `.PHONY`.
- [x] `scripts/build-dpkg.sh` — add an `EMTERM_MUX_ONLY` branch.

### Unplanned modification surfaced during sdd.4-implement

- `src-tauri/src/mux/cli.rs` — `execute_mux()` directly calls
  `super::tmux_import::import_tmux_conf_if_needed()`, which IMPLEMENTATION.md
  had said lives only in `main.rs:run_gui`. With `mux::tmux_import` now
  gated on `feature = "gui"` per FR4, the `use` and the call site must
  also be `#[cfg(feature = "gui")]`. Resolved inline: a single `#[cfg]`
  on the `use` statement and one on the call, with a comment explaining
  why the mux-only build cannot reach `settings_store`. Doc impact:
  IMPLEMENTATION.md §"Component Interaction" should be updated by
  sdd.6-verify to list `mux::cli::execute_mux` as another GUI-only caller
  of `tmux_import`, alongside `main.rs:run_gui`.

## SPEC.md Compliance

### Success Criteria

| ID    | Criterion                                                                                                                  | How to Verify                                                                                                                  |
| ----- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| SC-1  | `cargo check --no-default-features --features mux` succeeds with no compile error / no new warning                         | TS-3                                                                                                                           |
| SC-2  | `cargo build --release --no-default-features --features mux` produces a working CLI+mux binary                             | TS-6 plus manual `emterm mux daemon` invocation                                                                                |
| SC-3  | `cargo build --release` (default) produces a binary indistinguishable from the pre-task GUI build                          | TS-4 plus manual GUI smoke test (launches windowed terminal, opens viewers, `emterm mux …` works)                              |
| SC-4  | `cargo build --release --no-default-features` produces a binary indistinguishable from the pre-task CLI-only build         | TS-5 plus manual `emterm markdown <file>` smoke test plus new neutral error string for `emterm mux …`                          |
| SC-5  | `make mux-dpkg` produces `emterm-mux_<ver>_<arch>.deb` with `Depends: libc6`                                                | TS-11                                                                                                                          |
| SC-6  | `make dpkg` and `make cli-dpkg` produce unchanged deb packages                                                             | TS-11                                                                                                                          |
| SC-7  | All `cargo test` invocations in the test scenarios pass                                                                    | TS-8, TS-9, TS-10                                                                                                              |

### Functional Requirements Coverage

| Requirement | Phase             | Verification (TS)                       |
| ----------- | ----------------- | --------------------------------------- |
| FR1         | Phase 2           | TS-1, TS-2, TS-3                        |
| FR2         | Phase 2           | TS-1, TS-2, TS-3                        |
| FR3         | Phase 3           | TS-3, TS-12                             |
| FR4         | Phase 3           | TS-3, TS-6                              |
| FR5         | Phase 4           | TS-3, TS-8, TS-9                        |
| FR6         | Phase 1, Phase 3  | TS-3, TS-6, TS-10, TS-12                |
| FR6.1       | Phase 3           | TS-10, TS-13                            |
| FR7         | Phase 3           | TS-3, TS-6                              |
| FR8         | Phase 5           | TS-10 (covered transitively via deb)    |
| FR9         | Phase 6           | TS-10 (covered transitively), TS-11     |
| NFR1        | Phase 1–6         | TS-1, TS-2, TS-4, TS-5, TS-11           |
| NFR2        | Phase 2–3         | TS-3 (qualitative: cli-mux < gui)       |
| NFR3        | Phase 2           | TS-1, TS-3 (gui-only `cargo check` arm) |
| NFR4        | Phase 6           | TS-11                                   |

## E2E Testing

This project does not ship an automated E2E suite for the native build.
The `e2e-tests/` directory at the repo root predates the native-poc branch
and is not run against the current binary. The user-facing flow is
covered by the manual scenarios below.

## Manual Testing (E2E Not Possible)

### M1: GUI tier smoke test (unchanged behavior)

- [ ] `make build` succeeds.
- [ ] `./src-tauri/target-host/release/emterm` launches the windowed
      terminal.
- [ ] Inside that terminal, `emterm mux daemon &` starts the daemon and
      `emterm mux attach` bridges into it.
- [ ] `emterm markdown <file>` displays a Markdown document in the child
      viewer.

### M2: CLI+mux tier (new) smoke test on a host with only libc6

- [ ] On a host with no `libwebkit2gtk-4.1-0` / `libgtk-3-0` /
      `libglib2.0-0` installed, `sudo dpkg -i emterm-mux_<ver>_<arch>.deb`
      succeeds.
- [ ] `emterm mux daemon &` starts the daemon.
- [ ] `emterm mux attach` (in another shell on the same host) brings up
      a shell session.
- [ ] `emterm markdown <file>` emits the OSC Markdown sequence to stdout
      (same as the CLI-only deb).
- [ ] `emterm` (no subcommand) prints the existing
      "this build provides only CLI subcommands" usage and exits 2 (GUI
      not present in this tier).

### M3: CLI-only tier (unchanged) smoke test

- [ ] `make cli-build` succeeds; `make cli-dpkg` produces
      `emterm-cli_<ver>_<arch>.deb`.
- [ ] `dpkg-deb --info` on it shows `Depends: libc6` (no change from
      pre-task).
- [ ] `emterm mux daemon` prints the new SPEC-mandated neutral error
      ("emterm: `mux` is not available in this build. / Install a build
      that includes the `mux` feature (`emterm` or `emterm-mux`) to use
      `emterm mux`.") and exits 2.
- [ ] `emterm markdown <file>` works.

### M4: Deb file shape

- [ ] `dpkg-deb --contents build/emterm_<ver>_<arch>.deb` shows the same
      file layout as before the task (binary + icons + `.desktop` +
      postinst/postrm + docs).
- [ ] `dpkg-deb --contents build/emterm-cli_<ver>_<arch>.deb` shows the
      same minimal layout as before (binary + docs, no GUI assets).
- [ ] `dpkg-deb --contents build/emterm-mux_<ver>_<arch>.deb` shows the
      same minimal layout as the CLI-only deb (binary + docs, no GUI
      assets, no `.desktop` entry).
- [ ] `dpkg-deb --info build/emterm-mux_<ver>_<arch>.deb` reports
      `Package: emterm-mux`, `Section: utils`, `Depends: libc6`,
      `Maintainer: m-m-n <51132276+m-m-n@users.noreply.github.com>`, and
      the SPEC-prescribed description.

### M5: SSH-side deployment dry-run (UC1)

- [ ] Copy `emterm-mux_<ver>_<arch>.deb` to a clean Ubuntu host that has
      only `libc6` installed.
- [ ] `sudo dpkg -i` succeeds without missing-dependency errors.
- [ ] `emterm mux daemon &` starts and writes to its log.
- [ ] From a second shell, `emterm mux attach` bridges in and a shell
      session is interactive.

## Performance Verification (qualitative)

The SPEC asserts a qualitative ordering "CLI-only < CLI+mux < GUI" for
both compile time and binary size. There is no fixed threshold to assert.

- [ ] Subjective: `make mux-build` finishes faster than `make build` on
      the same machine and the resulting binary is smaller than the GUI
      release binary (winit / wgpu / wry / GTK / WebKitGTK / swash /
      zeno / fontdb / resvg are not linked).

## Security Verification

Not applicable. This task is structural and introduces no new code paths,
no new IPC surfaces, and no new file/network reads.

## Verification Summary

| Category                    | Items | Automated (cargo) | E2E | Manual |
| --------------------------- | ----- | ----------------- | --- | ------ |
| Build matrix                | 7     | 7                 | 0   | 0      |
| Unit / integration          | 3     | 3                 | 0   | 0      |
| Static grep (TS-12, TS-13)  | 2     | 2                 | 0   | 0      |
| Packaging (TS-11)           | 1     | 0                 | 0   | 1      |
| Manual smoke (M1–M5)        | 5     | 0                 | 0   | 5      |
| **Total**                   | **18** | **12**           | **0** | **6**  |
