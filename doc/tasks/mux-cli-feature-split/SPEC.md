# Feature: Mux in CLI Build

## Overview

Make the eMterm mux subsystem (daemon / bridge / CLI / PTY) part of the
CLI build so `cargo build --release --no-default-features` (`emterm-cli`
deb) ships a binary that can run `emterm mux --daemon` on headless SSH
hosts. The mux subsystem has no GUI dependency, so it is promoted to
"always built" instead of being gated behind any feature flag.

No new cargo features, no new make targets, no new deb packages: the
existing `make build` / `make cli-build` / `make dpkg` / `make cli-dpkg`
keep their previous shape, and the same `emterm` and `emterm-cli` debs
are produced as before. Only the CLI binary's capability surface grows.

## Motivation

eMterm's CLI build was originally meant for SSH hosts where the GUI
deps (winit / wgpu / wry / GTK / WebKitGTK) cannot be installed. The
mux daemon is the very feature that makes SSH usage worthwhile (attach
from local GUI eMterm over SSH, persistent multi-pane sessions), so
excluding mux from the CLI build defeated its purpose. The fix is to
recognize that mux has no GUI dependency and just build it
unconditionally.

## User Story

### US1: Run mux daemon on a headless SSH host

As an eMterm user, I install the `emterm-cli` deb on a remote Linux
server that has only `libc6` (no webkit / gtk), run
`emterm mux --daemon` there, and attach to it from my local GUI eMterm
over SSH.

**Acceptance Criteria:**
- [ ] `dpkg -i emterm-cli_<ver>_<arch>.deb` succeeds on a host with no
  webkit / gtk libraries installed.
- [ ] `emterm mux --daemon` starts a daemon on the host (or
  `emterm mux` auto-spawns one when starting a session).
- [ ] `emterm mux attach` from another shell on the same host bridges
  into the daemon.

### US2: GUI build unchanged

As an eMterm developer, I want `make build` and `make dpkg` to produce
the same GUI behavior as before, so nothing breaks for existing users.

**Acceptance Criteria:**
- [ ] `cargo build --release` with default features still produces a
  GUI binary that opens the windowed terminal, child WebView windows,
  and runs `emterm mux --daemon` if invoked.
- [ ] No new optional features are introduced.

## Functional Requirements

### FR1: Remove the `mux` cargo feature

The `mux` cargo feature introduced by the prior `feat(mux): split mux
into standalone cargo feature` commit is removed. The mux subsystem is
not gated behind any feature flag.

### FR2: Promote mux deps to "always built"

In `src-tauri/Cargo.toml`, the following crates are moved out of the
`gui = [...]` feature list and out of the `optional = true` dep entries
into the unconditional `[dependencies]` section:

- `tokio` (with the existing feature set)
- `tokio-util` (with `codec`)
- `futures`
- `chrono`
- `anyhow`
- `hostname`
- `vt100`
- `portable-pty`
- `term_core` (path dep)
- `mux_ipc` (path dep)

The `gui` feature retains only its actual GUI dependencies
(winit / wgpu / wry / swash / etc.).

### FR3: Ungate mux modules in `lib.rs`

The modules `mux`, `pty`, `scroll`, `wakeup`, and `self_exec` lose
their `#[cfg(feature = "mux")]` gate (they had been gated behind the
short-lived `mux` feature) and are declared unconditionally.

### FR4: Keep `mux::tmux_import` gated on `gui`

`mux::tmux_import::import_tmux_conf_if_needed()` writes to
`crate::settings_store`, which is GUI-only. The `pub mod tmux_import`
declaration in `src-tauri/src/mux/mod.rs` stays
`#[cfg(feature = "gui")]`, and the two call sites
(`main.rs:run_gui` and `mux::cli::execute_mux`) keep their
`#[cfg(feature = "gui")]` guards. Mux-on-SSH workflows hand-manage
`settings.json` directly.

### FR5: Drop the mux dispatch cfg gate in `main.rs`

`main.rs` no longer guards the `if sub == "mux"` arm with
`#[cfg(feature = "mux")]`. Both GUI and CLI builds dispatch
`emterm mux ...` to `emterm::mux::cli::run` directly. The previous
`"not available in this build"` fallback branch is removed.

### FR6: `viewer_kinds.rs` SSOT stays

The `REPLAYABLE_VIEWER_KINDS` constant continues to live in
`src-tauri/src/viewer_kinds.rs` (always built) with a
`pub use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS` re-export in
`viewer/mod.rs` (GUI-only) so existing call sites compile unchanged.
The mux scrollback filter
(`crate::mux::scrollback_filter::strip_replayable_rich_content`) uses
`crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS` directly.

### FR7: `mux::prefix` tests stay GUI-independent

The test rewrites in `src-tauri/src/mux/prefix.rs` that swap
`crate::settings::parse_mux_action_chord` for
`crate::mux::prefix::parse_prefix_key` are retained, so
`cargo test --no-default-features --lib` exercises the prefix logic
without enabling the GUI feature.

### FR8: Build commands unchanged

- `make build` / `cargo build --release` → GUI binary (no change).
- `make cli-build` / `cargo build --release --no-default-features` →
  CLI binary that includes mux (was previously without mux).
- `make dpkg` → `emterm` deb (no change).
- `make cli-dpkg` (`EMTERM_CLI_ONLY=1 bash scripts/build-dpkg.sh`) →
  `emterm-cli` deb (Depends: libc6) with mux included.

No new make targets, no new env-var branches, no `emterm-mux` deb.

### FR9: `emterm-cli` deb Description updated

The `emterm-cli` deb's Description body lists the mux entry points
alongside the existing CLI subcommands.

## Non-Functional Requirements

### NFR1: GUI build bit-identical

The default GUI build (`cargo build --release`) compiles the same set
of crates as before because the deps that moved out of the optional
list were already part of the `gui` feature's transitive closure.

### NFR2: CLI build lighter than GUI

`cargo build --release --no-default-features` does not link winit,
wgpu, wry, GTK, WebKitGTK, swash, zeno, fontdb, ab_glyph, resvg,
rodio, term_images, regex, unicode-width, or unicode-segmentation.
The resulting binary depends only on libc6.

### NFR3: No new tests required

The only new code is two `pub use` re-exports (`viewer_kinds`) and
test-side parser direct calls (`mux::prefix`). Existing
`drift_route_dispatch_kinds_match_replayable_viewer_kinds_ssot` and the
`mux::prefix` test module continue to cover the affected surface.

## Out of Scope

- Adding a `daemon` positional subcommand to `mux::cli::run`. The
  `--daemon` flag is the canonical way to start the foreground daemon;
  user-facing docs reference `emterm mux --daemon`.
- Declaring `Conflicts:` / `Replaces:` between the `emterm` and
  `emterm-cli` debs. The two debs target disjoint host profiles
  (desktop with GUI deps vs. headless SSH) and are not expected to
  coexist on the same machine, so the pre-existing absence of
  `Conflicts` / `Replaces` is kept as-is.
