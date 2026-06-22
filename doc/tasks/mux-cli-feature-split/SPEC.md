# Feature: Mux CLI Feature Split

## Overview

Split the `mux` subsystem (daemon / bridge / CLI / PTY) out of the `gui`
cargo feature into a standalone `mux` feature, so a `--no-default-features
--features mux` build can run `emterm mux --daemon` on headless SSH hosts
without pulling in winit / wgpu / wry / GTK / WebKitGTK. The default
`gui` feature now requires `mux`, so the GUI build is bit-identical in
behavior. A new `emterm-mux` deb package ships the CLI+mux binary for
remote/headless installation.

## Objectives

- Add a `mux` cargo feature that gates the mux daemon, mux bridge, mux
  CLI, and the PTY subsystem they consume.
- Make `gui` require `mux` so `cargo build --release` keeps the same
  output as today.
- Make `cargo build --release --no-default-features --features mux`
  produce a working CLI+mux binary that can serve as an SSH-side mux
  daemon.
- Add `make mux-build` and `make mux-dpkg` to build the binary and the
  `emterm-mux` deb package.
- Keep the existing `emterm` (GUI) and `emterm-cli` (CLI-only) debs
  unchanged.

## User Stories

### US1: Stand up a mux daemon on a headless SSH host

As an eMterm user, I want to install a small `emterm` binary on a
remote Linux server and run `emterm mux --daemon` there, so I can attach
to it from my local GUI eMterm over SSH.

**Acceptance Criteria:**
- [ ] `dpkg -i emterm-mux_<ver>_<arch>.deb` succeeds on a host with no
  webkit / gtk libraries installed (libc6 only).
- [ ] `emterm mux --daemon` starts a daemon on the host (or
  `emterm mux` auto-spawns one when starting a session).
- [ ] `emterm mux attach` from another shell on the same host bridges
  into the daemon.

### US2: GUI build unchanged

As an eMterm developer, I want `make build` and `make dpkg` to produce
the same GUI behavior as before, so nothing breaks for existing users.

**Acceptance Criteria:**
- [ ] `cargo build` and `cargo build --release` with default features
  succeed.
- [ ] The GUI binary still launches the windowed terminal, opens
  Markdown / image / data viewers, and runs `emterm mux --daemon` etc.
- [ ] `cargo test` (default features) passes.

### US3: CLI-only build unchanged

As an eMterm developer, I want `make cli-build` and `make cli-dpkg` to
keep producing the lean CLI-only deb (libc6 only), so the existing
SSH-deploy story still works for users who only need the
markdown/json/yaml/image subcommands.

**Acceptance Criteria:**
- [ ] `cargo build --release --no-default-features` succeeds.
- [ ] `emterm-cli_<ver>_<arch>.deb` has the same shape (Depends, file
  list) as before this task.
- [ ] `emterm mux --daemon` in the CLI-only binary still exits with the
  current "build does not include mux support" message.

## Technical Requirements

### Functional Requirements

- **FR1 (New `mux` feature):** Add `mux` to `[features]` in
  `src-tauri/Cargo.toml`. `default = ["gui"]` is preserved; `gui` is
  rewritten to include `"mux"` as its first entry. Net effect: every
  binary that ships today is unchanged.
- **FR2 (Dependency reclassification):** Move the following from the
  `gui` feature list to the `mux` feature list. They become "enabled
  whenever the `mux` feature is on," and `gui` re-enables them
  transitively through `gui = ["mux", ...]`:
  - `tokio`, `tokio-util`, `futures`
  - `chrono`, `anyhow`, `hostname`
  - `vt100`
  - `portable-pty`
  - `term_core`, `mux_ipc`

  `gui` keeps its own dependencies (winit, wgpu, egui, egui-wgpu, wry,
  swash, zeno, fontdb, ab_glyph, resvg, rodio, arboard, notify-rust,
  raw-window-handle, pollster, regex, unicode-width,
  unicode-segmentation, gtk, opener, term_images).
- **FR3 (Module gate rewrites in `lib.rs`):**
  - `pub mod mux;` becomes `#[cfg(feature = "mux")]`.
  - `pub mod pty;` becomes `#[cfg(feature = "mux")]`.
  - All other `#[cfg(feature = "gui")]` gates remain as they are
    today. Modules under `src-tauri/src/mux/*` keep their own internal
    structure; only the top-level declaration changes.
- **FR4 (Module gate inside `mux/`):** `mux::tmux_import` stays
  `#[cfg(feature = "gui")]` because it calls
  `crate::settings_store::save_patch_to`, which is GUI-only. Concretely:
  - `src-tauri/src/mux/mod.rs`: change the existing `pub mod
    tmux_import;` line to `#[cfg(feature = "gui")] pub mod
    tmux_import;`.
  - The `mux::tmux_import::import_tmux_conf_if_needed()` call in
    `main.rs:run_gui` is already inside the `feature = "gui"` `run_gui`
    function, so no further change is needed at the call site.
- **FR5 (Dispatch gate in `main.rs`):** In the
  `if sub == "mux" { ... }` block, swap `#[cfg(feature = "gui")]` and
  `#[cfg(not(feature = "gui"))]` to `#[cfg(feature = "mux")]` and
  `#[cfg(not(feature = "mux"))]`. The error message changes from
  ```
  emterm: `mux` is not available in this CLI-only build.
  Install the GUI build (`emterm`) to use `emterm mux`.
  ```
  to
  ```
  emterm: `mux` is not available in this build.
  Install a build that includes the `mux` feature (`emterm` or
  `emterm-mux`) to use `emterm mux`.
  ```
- **FR6 (Move GUI-side symbols mux depends on):**
  - `crate::viewer::REPLAYABLE_VIEWER_KINDS` (a `&[&str]` constant in
    `viewer/mod.rs:40`) is moved into a new CLI-shared module
    `src-tauri/src/viewer_kinds.rs`, declared from `lib.rs` as
    `pub mod viewer_kinds;` (no cfg gate). `viewer/mod.rs` re-exports
    it via `pub use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;` so
    existing GUI call sites keep compiling. `mux/scrollback_filter.rs`
    is rewritten to import from `crate::viewer_kinds`.
  - `crate::scroll::ScrollPosition` (a 23-line enum in `scroll.rs`)
    has its module declaration changed from
    `#[cfg(feature = "gui")] pub mod scroll;` to
    `#[cfg(feature = "mux")] pub mod scroll;`. This is safe because
    `gui = ["mux", ...]` means the GUI build still pulls it in, and
    `mux/window_group.rs` (which imports it) is also under `feature =
    "mux"`. The module file itself needs no changes.
  - `crate::wakeup` (called from `pty/mod.rs:616` as
    `crate::wakeup::wake()` to nudge the winit event loop after each
    PTY read) has its module declaration changed from
    `#[cfg(feature = "gui")] pub mod wakeup;` to
    `#[cfg(feature = "mux")] pub mod wakeup;`. The module itself has
    no winit dependency (only `OnceLock` + `Arc`); under the mux-only
    tier `wake()` stays a no-op because no event loop installs a wake
    function. Module body needs no changes.
  - `crate::self_exec` (called from `mux/daemon.rs:154,210` to find
    the current executable path when spawning the daemon child) has
    its module declaration changed from
    `#[cfg(feature = "gui")] pub mod self_exec;` to
    `#[cfg(feature = "mux")] pub mod self_exec;`. The module is
    OS-API only (no winit/wgpu/wry/swash/etc); GUI consumers
    (`app`, `viewer`, `settings_launcher`) still reach it via
    `gui ⊃ mux`. Module body needs no changes.
- **FR6.1 (Test-only symbol fix in `mux::prefix`):** The
  `#[cfg(test)]` module in `src-tauri/src/mux/prefix.rs` calls
  `crate::settings::parse_mux_action_chord(...)` in 12 places.
  `crate::settings` is GUI-only and cannot be reached under
  `--features mux` alone. Because `parse_mux_action_chord` is a
  one-line wrapper over `crate::mux::prefix::parse_prefix_key`,
  rewrite those 12 test call sites to call `parse_prefix_key`
  directly. The GUI-side `parse_mux_action_chord` function stays
  unchanged for non-test callers. After this, `cargo test
  --no-default-features --features mux` can compile the mux prefix
  tests.
- **FR7 (PTY gate unchanged in structure):** `src-tauri/src/pty/` keeps
  the same internal layout (`input`, `passthrough_scanner`, `ring`,
  `visibility`, top-level `mod.rs`). Only the `lib.rs` top-level
  `pub mod pty;` declaration is re-gated from `feature = "gui"` to
  `feature = "mux"`.
- **FR8 (Makefile targets):** Add two new targets:
  ```make
  mux-build: ## Release build (CLI + mux, --features mux only)
  	CARGO_TARGET_DIR=$(CARGO_TARGET_HOST) cargo build --release \
  	    --no-default-features --features mux $(MANIFEST)

  mux-dpkg: ## Build the CLI+mux deb package
  	EMTERM_MUX_ONLY=1 bash scripts/build-dpkg.sh
  ```
  Wire both into the `.PHONY` list.
- **FR9 (build-dpkg.sh extension):** Extend the script to recognize
  `EMTERM_MUX_ONLY=1`:
  - `DEB_PACKAGE="emterm-mux"`.
  - Build command:
    ```
    cargo build --manifest-path src-tauri/Cargo.toml --release \
        --no-default-features --features mux
    ```
  - Skip `.desktop` file, icons, GTK postinst/postrm (same as the
    `EMTERM_CLI_ONLY` path).
  - `DEBIAN/control`:
    ```
    Package: emterm-mux
    Version: ${VERSION}
    Section: utils
    Priority: optional
    Architecture: ${DEB_ARCH}
    Maintainer: m-m-n <51132276+m-m-n@users.noreply.github.com>
    Depends: libc6
    Description: CLI + mux daemon for eMterm (headless / SSH use)
     Command-line tools (image / markdown / json / yaml) plus the
     eMterm mux daemon and bridge. Intended for remote Linux hosts
     where the GUI build cannot be installed.
     .
     Commands:
      - emterm image|markdown|json|yaml: emit display escape sequences
      - emterm mux: start a mux session (auto-spawns the daemon)
      - emterm mux --daemon: run the eMterm mux daemon in the foreground
      - emterm mux attach: bridge into a running daemon
    ```
  - The three modes (`EMTERM_CLI_ONLY`, `EMTERM_MUX_ONLY`, default
    GUI) are mutually exclusive: when `EMTERM_MUX_ONLY=1` is set,
    `EMTERM_CLI_ONLY` is ignored with a warning. The default path
    (neither var set) stays the GUI build.

### Non-Functional Requirements

- **NFR1 (Backward compatibility):** `make build`, `make dpkg`,
  `make cli-build`, and `make cli-dpkg` outputs are functionally
  identical to their pre-task counterparts. The GUI deb's
  `Depends:` and binary behavior do not change. The CLI deb's
  `Depends: libc6` and "mux not available" error stay the same.
- **NFR2 (Build cost):** `cargo build --no-default-features --features
  mux` finishes faster than the GUI build because it skips winit /
  wgpu / wry / swash / zeno / fontdb / resvg / GTK / WebKitGTK / arboard
  / notify-rust / rodio. Exact numbers are not asserted; the
  qualitative "CLI-only < CLI+mux < GUI" cost ordering is the bar.
- **NFR3 (Feature orthogonality):** `--features mux,gui` and
  `--features gui` produce the same binary because `gui = ["mux",
  ...]`.
- **NFR4 (No new runtime deps):** The CLI+mux binary depends on libc6
  at runtime (just like the CLI-only deb), because tokio / portable-pty
  / term_core / mux_ipc are statically linked.

## Implementation Approach

### Architecture

**Feature-to-deb mapping after this task:**

```
cargo features                        deb package        Depends
────────────────────────────────────  ─────────────────  ──────────────────
default = gui (= mux + gui-only)      emterm             libc6 + webkit/gtk
mux (alone, no gui)                   emterm-mux (new)   libc6
(no features)                         emterm-cli         libc6
```

**Module gate map (post-task):**

```
src-tauri/src/lib.rs
  pub mod cli;             // always
  pub mod i18n;            // always
  pub mod localtime;       // always
  pub mod logging;         // always
  pub mod settings_core;   // always
  pub mod viewer_kinds;    // always   (new: SSOT for REPLAYABLE_VIEWER_KINDS)

  #[cfg(feature = "mux")] pub mod mux;
  #[cfg(feature = "mux")] pub mod pty;
  #[cfg(feature = "mux")] pub mod scroll;     // moved from gui-only
  #[cfg(feature = "mux")] pub mod wakeup;     // moved from gui-only
                                              // (pty::PtySession::reader uses it)
  #[cfg(feature = "mux")] pub mod self_exec;  // moved from gui-only
                                              // (mux::daemon spawns child via it)

  #[cfg(feature = "gui")] pub mod app;
  #[cfg(feature = "gui")] pub mod bell;
  ...  (all other gui-only modules unchanged)
  #[cfg(feature = "gui")] pub mod viewer;   // still gui-only; re-exports
                                            // viewer_kinds::REPLAYABLE_VIEWER_KINDS

src-tauri/src/mux/mod.rs
  pub mod apc;              // unchanged
  pub mod bridge;
  pub mod cli;
  pub mod daemon;
  pub mod dialog;
  pub mod ipc;
  pub mod prefix;
  pub mod scrollback_buffer;
  pub mod scrollback_filter;       // now imports crate::viewer_kinds
  pub mod session;
  pub mod snapshot;
  #[cfg(feature = "gui")] pub mod tmux_import;   // ← only changed line
  pub mod tmux_conf;
  pub mod window_group;            // imports crate::scroll::ScrollPosition,
                                   // both gated on feature = "mux"
```

### Cargo.toml diff sketch

```toml
[features]
default = ["gui"]

# Mux subsystem: PTY pipeline + mux daemon/bridge/CLI. Buildable as a
# standalone feature for SSH-side `emterm mux --daemon` deployments
# (`--no-default-features --features mux`). The `gui` feature requires
# this so the default windowed build is unchanged.
mux = [
    "dep:tokio",
    "dep:tokio-util",
    "dep:futures",
    "dep:chrono",
    "dep:anyhow",
    "dep:hostname",
    "dep:vt100",
    "dep:portable-pty",
    "dep:term_core",
    "dep:mux_ipc",
]

# Windowed terminal stack. Requires mux so `emterm mux …` keeps working
# in the GUI build.
gui = [
    "mux",
    "dep:winit",
    "dep:wgpu",
    "dep:egui",
    "dep:egui-wgpu",
    "dep:wry",
    "dep:swash",
    "dep:zeno",
    "dep:fontdb",
    "dep:ab_glyph",
    "dep:resvg",
    "dep:rodio",
    "dep:arboard",
    "dep:notify-rust",
    "dep:raw-window-handle",
    "dep:pollster",
    "dep:term_images",
    "dep:regex",
    "dep:unicode-width",
    "dep:unicode-segmentation",
    "dep:gtk",
    "dep:opener",
]
```

The corresponding `optional = true` attributes on the dependency table
entries already exist for every crate listed above; no `[dependencies]`
restructuring is needed beyond confirming `optional = true` is set on
each of the moved entries.

### File Structure

```
src-tauri/
├── Cargo.toml                       (updated [features])
├── src/
│   ├── lib.rs                        (mux/pty/scroll/wakeup/self_exec
│   │                                 → mux gate; viewer_kinds added)
│   ├── main.rs                       (mux dispatch cfg flipped)
│   ├── viewer_kinds.rs              NEW — SSOT for REPLAYABLE_VIEWER_KINDS
│   ├── viewer/mod.rs                 (re-export REPLAYABLE_VIEWER_KINDS)
│   ├── scroll.rs                     (unchanged contents; lib.rs gate
│   │                                 flips from gui to mux)
│   ├── wakeup.rs                     (unchanged contents; lib.rs gate
│   │                                 flips from gui to mux)
│   ├── self_exec.rs                  (unchanged contents; lib.rs gate
│   │                                 flips from gui to mux)
│   ├── mux/
│   │   ├── mod.rs                    (tmux_import gated to gui)
│   │   ├── prefix.rs                 (test bodies switch from
│   │   │                              crate::settings::parse_mux_action_chord
│   │   │                              to crate::mux::prefix::parse_prefix_key)
│   │   └── scrollback_filter.rs      (use crate::viewer_kinds)
│   └── pty/                          (unchanged)
├── Makefile                          (mux-build, mux-dpkg added)
└── scripts/
    └── build-dpkg.sh                 (EMTERM_MUX_ONLY branch added)
```

### Build commands matrix

```
cargo build                                         # GUI (default)
cargo build --release                               # GUI release
cargo build --no-default-features                   # CLI-only
cargo build --no-default-features --features mux    # CLI + mux (NEW)
cargo xwin build --release --target …               # Windows GUI
make build       → GUI
make cli-build   → CLI-only
make mux-build   → CLI + mux (NEW)
make win-build   → Windows GUI
make dpkg        → emterm_<ver>_<arch>.deb
make cli-dpkg    → emterm-cli_<ver>_<arch>.deb
make mux-dpkg    → emterm-mux_<ver>_<arch>.deb (NEW)
```

## Test Scenarios

### Build Tests

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` succeeds.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml
      --no-default-features` succeeds.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml
      --no-default-features --features mux` succeeds.
- [ ] `cargo build --release --manifest-path src-tauri/Cargo.toml`
      succeeds and the binary launches the GUI on Linux.
- [ ] `cargo build --release --no-default-features --features mux
      --manifest-path src-tauri/Cargo.toml` succeeds and produces a
      binary that runs `emterm mux --daemon`.
- [ ] `cargo build --release --no-default-features
      --manifest-path src-tauri/Cargo.toml` still produces the
      CLI-only binary (behaviorally identical to before).
- [ ] `cargo xwin build --release --target x86_64-pc-windows-msvc
      --manifest-path src-tauri/Cargo.toml` succeeds (Windows cross-
      build of the default GUI feature set).

### Unit / Integration Tests

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` (default
      features) passes — same as today.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml
      --no-default-features` (CLI-only) passes — same as today.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml
      --no-default-features --features mux` (CLI + mux) passes. Any
      test that uses `#[cfg(feature = "gui")]` stays gated on GUI;
      mux-side tests that previously ran only under `gui` because they
      live in `src/mux/` now run here too. (The plan step will list
      the concrete `--lib` tests expected to be enabled.)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --bin emterm`
      passes for the default feature set.

### Subcommand Smoke Tests

- [ ] `emterm` (GUI deb binary, no args) launches the windowed
      terminal.
- [ ] `emterm mux --daemon` (GUI deb binary) starts the daemon.
- [ ] `emterm mux --daemon` (CLI+mux binary) starts the daemon on a
      host that has only libc6 installed.
- [ ] `emterm mux attach` (CLI+mux binary) bridges into a running
      daemon.
- [ ] `emterm mux --daemon` (CLI-only binary) prints the "build does
      not include mux support" message and exits 2.
- [ ] `emterm markdown <file>` works in all three binaries.
- [ ] `emterm` (CLI+mux binary, no args) prints a help/usage message
      (same as the current CLI-only path) and exits non-zero, because
      the GUI subsystem is not built in. Implementation: `run_gui` is
      `#[cfg(feature = "gui")]`, so the no-subcommand branch falls
      through to the existing `#[cfg(not(feature = "gui"))]` arm.

### Packaging Tests

- [ ] `make dpkg` produces `build/emterm_<ver>_<arch>.deb` with
      `Depends: libc6, libwebkit2gtk-4.1-0, libgtk-3-0, libglib2.0-0`.
- [ ] `make cli-dpkg` produces `build/emterm-cli_<ver>_<arch>.deb`
      with `Depends: libc6`.
- [ ] `make mux-dpkg` produces `build/emterm-mux_<ver>_<arch>.deb`
      with `Depends: libc6`.
- [ ] `dpkg-deb --info` on the CLI+mux deb reports
      `Package: emterm-mux` and `Section: utils`.
- [ ] The three debs install side-by-side on the same machine
      without dpkg conflicts (different package names, identical
      `/usr/bin/emterm` path means only the last installed wins —
      explicitly documented behavior, not asserted as supported).

### E2E Tests

**Existing E2E tests**: none — this project does not ship an E2E
suite for the native build. The existing `e2e-tests/` directory at
the repo root predates the native-poc branch and is not run against
the current binary.

**Manual reproduction**:
1. SSH to a clean Ubuntu host that has only `libc6` installed
   (no webkit / gtk).
2. Install `emterm-mux_<ver>_amd64.deb`.
3. `emterm mux --daemon &`
4. `emterm mux attach`
5. Confirm a shell appears, type a command, see output.

### Edge Cases

- [ ] A user invokes `cargo build --features mux` on top of
      `default = ["gui"]` (i.e. `--features mux` without
      `--no-default-features`). Expected: same as `cargo build`, since
      `gui` already requires `mux`. No duplicate symbol errors.
- [ ] `mux::tmux_import` is referenced from `main.rs:run_gui`. Because
      `run_gui` is itself `#[cfg(feature = "gui")]` and `tmux_import`
      stays gated on `feature = "gui"`, the call site resolves only
      when both gates agree. No `feature = "mux"`-only build should
      try to call `import_tmux_conf_if_needed`.
- [ ] `crate::viewer::REPLAYABLE_VIEWER_KINDS` is still exported from
      `viewer/mod.rs` after the move (via `pub use`), so any external
      consumer that referenced it through `crate::viewer::…` keeps
      working. Internal call sites in `mux/scrollback_filter.rs` are
      updated to the new path.
- [ ] `scroll.rs` becomes inaccessible in a CLI-only build (no `mux`,
      no `gui`). Nothing in the CLI-only surface references it, so no
      compile error.

## Error Handling

### Unsupported subcommand in non-mux build

When the binary is built without `mux` (CLI-only) and the user runs
`emterm mux …`, print:

```
emterm: `mux` is not available in this build.
Install a build that includes the `mux` feature (`emterm` or
`emterm-mux`) to use `emterm mux`.
```

and exit 2.

### GUI launch in mux-only build

When the binary is built with `mux` but not `gui` and the user runs
`emterm` with no subcommand (or with a flag that would normally start
the windowed terminal), fall through to the existing
`#[cfg(not(feature = "gui"))]` arm in `main()`:

```
emterm: this build provides only CLI subcommands.
Usage: emterm <markdown|json|yaml|image> <file> [options]
Run `emterm <subcommand> --help` for details.
```

and exit 2. Note: `mux` is reached BEFORE this arm via the dedicated
`if sub == "mux"` dispatch, so a mux-only binary can still run
`emterm mux …`.

## Success Criteria

- [ ] `cargo check --no-default-features --features mux` succeeds with
      no compile errors and no warnings beyond pre-existing ones.
- [ ] `cargo build --release --no-default-features --features mux`
      produces a working binary that responds to `emterm mux --daemon`
      and `emterm mux attach`.
- [ ] `cargo build --release` (default) produces a binary
      indistinguishable from the pre-task GUI build in observed
      behavior.
- [ ] `cargo build --release --no-default-features` produces a binary
      indistinguishable from the pre-task CLI-only build.
- [ ] `make mux-dpkg` produces `emterm-mux_<ver>_<arch>.deb` with
      `Depends: libc6`.
- [ ] `make dpkg` and `make cli-dpkg` produce unchanged deb packages.
- [ ] All `cargo test` invocations in the test scenarios pass.

## Open Questions

> **Note**: No TBD items. Resolved during clarification:
> - Feature name: `mux` (single feature; PTY is bundled).
> - `gui` depends on `mux`.
> - New deb `emterm-mux` for the CLI+mux build.
> - `mux::tmux_import` stays `feature = "gui"`-gated because of its
>   `settings_store::save_patch_to` dependency.

## References

- `tmp/issues-windows-mux-2026-06-22.md` section 5 ("CLI ビルドで
  mux daemon が起動しない") — root cause and recommended fix A.
- `doc/tasks/cli-only-build/SPEC.md` — design of the original `gui`
  feature split.
- `doc/tasks/mux-feature-cleanup/SPEC.md` — current shape of the mux
  module after pane / split / zoom / copy-mode removal.
- `src-tauri/Cargo.toml` `[features]` — the existing `gui` feature
  definition this task restructures.
- `src-tauri/src/lib.rs` — the existing module gate layout.
- `src-tauri/src/main.rs` lines 75–97 — the `emterm mux …` dispatch
  whose cfg is flipped to `feature = "mux"`.
