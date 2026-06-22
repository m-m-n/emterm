# Feature: logging.rs RUST_LOG Process Env Isolation

## Overview

`src-tauri/src/logging.rs::init()` currently writes a default `RUST_LOG` filter string into the process environment table via `std::env::set_var`, then reads it back through `env_logger::Builder::from_env`. The write leaks into every child PTY process spawned later (pwsh, bash, fnm, etc.) and surfaces as unwanted `env_logger` output — most visibly fnm's `INFO  fnm::version_files` lines on every PowerShell startup. This feature removes the process-env write entirely: the logger is initialized with an in-process filter using `Builder::parse_filters`, so child processes only see `RUST_LOG` when the user explicitly set it themselves.

## Objectives

- Drop the `unsafe { std::env::set_var("RUST_LOG", ...) }` block from `logging::init()` so the process environment table is not mutated at startup.
- Preserve every observable property of the existing logger: same default filter, same `[LEVEL][NATIVE-POC]` format, same `emterm.log` warn/error persistence in release builds, same `INIT.call_once` guard.
- Extract the filter-string resolution into a pure helper that is unit-testable.

## User Stories

### US1: child processes no longer inherit a leaked RUST_LOG

As a Windows user with fnm's `--use-on-cd` hook in `$PROFILE`, I want pwsh to start without fnm's `INFO fnm::version_files` lines, so my prompt is clean.

**Acceptance Criteria:**
- [ ] Launching the Windows GUI build of eMterm, opening a pwsh tab, observes no fnm INFO lines at startup.
- [ ] The same holds for any other child shell whose env_logger-based tooling would otherwise pick up the leaked default.

### US2: explicit RUST_LOG still propagates as the user expects

As a developer debugging eMterm and a downstream tool together, I want `RUST_LOG=debug emterm` to take effect for both the parent and any spawned child, so I can correlate logs.

**Acceptance Criteria:**
- [ ] Launching `RUST_LOG=debug emterm` continues to produce debug-level eMterm output.
- [ ] A child shell spawned by that process observes `RUST_LOG=debug` (because the user set it themselves; eMterm does not strip it).

### US3: no regression in eMterm's own logging

As any user, I want eMterm's log output format, log file persistence, and once-init semantics to be unchanged.

**Acceptance Criteria:**
- [ ] Every log line still uses the `[LEVEL][NATIVE-POC] {message}` shape.
- [ ] Release builds still write `warn` / `error` records to `~/.local/share/net.laser5.app.emterm/logs/emterm.log` (Linux path).
- [ ] `init()` remains safe to call multiple times (no double-init, no panic on a second call).

## Technical Requirements

### Functional Requirements

- **FR1 — Drop process-env write:** `logging::init()` MUST NOT call `std::env::set_var("RUST_LOG", ...)`. The `unsafe` block currently wrapping that call MUST be removed (no replacement unsafe is allowed; the new code path is entirely safe).

- **FR2 — In-process filter resolution:** `logging::init()` MUST build its `env_logger::Builder` via `Builder::new()` + `parse_filters(&resolved_filters_str)`. The resolution rule is:
  - If `std::env::var("RUST_LOG")` returns `Ok(s)` with `s` non-empty, the filter string is `s` (the user's explicit value, including module-scoped forms like `wgpu_core=info`).
  - Otherwise the filter string is the existing default: `info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn`.
  - The same default applies when `RUST_LOG` is absent OR set to the empty string. (A user explicitly setting `RUST_LOG=` to suppress all output would be unusual; the existing `from_env(...default_filter_or("info"))` behavior maps empty to the default, and the new resolver matches that.)

- **FR3 — Pure helper:** A function `resolved_filters(env_value: Option<&str>) -> String` MUST exist as a private helper in `logging.rs`. Contract:
  - `resolved_filters(None) == DEFAULT_FILTER_STRING`.
  - `resolved_filters(Some("")) == DEFAULT_FILTER_STRING` (treat empty same as absent for parity with the prior `default_filter_or` path).
  - `resolved_filters(Some(s))` where `s != ""` returns `s.to_string()`.
  - Pure (no I/O, no global state read).

- **FR4 — Existing logger behavior preserved:**
  - The `INIT.call_once` guard MUST remain.
  - The `builder.format(|buf, record| ...)` writing `[{LEVEL}][NATIVE-POC] {message}` MUST be preserved verbatim.
  - The release-build branch that calls `write_to_log_file(record.level(), record.args())` for `warn` and `error` records MUST be preserved verbatim.
  - The final `builder.try_init()` (best-effort, ignoring the result) MUST be preserved.

- **FR5 — Unit tests for `resolved_filters`:** Add at least four test cases:
  - `resolved_filters_none_returns_default`
  - `resolved_filters_empty_returns_default`
  - `resolved_filters_passes_user_value`
  - `resolved_filters_passes_module_scoped_value`

### Non-Functional Requirements

- **NFR1 — No new unsafe:** The patch MUST reduce the `unsafe` count in `logging.rs` by exactly 1 (the removed block) and MUST NOT introduce any new `unsafe` block. The file's overall `unsafe` count after the change is reported in `IMPLEMENTATION.md`'s acceptance section.
- **NFR2 — No behavioral regression on log output:** Side-by-side comparison of stderr lines before and after, for the same eMterm action sequence, MUST be character-for-character identical (modulo expected timing differences).
- **NFR3 — No impact on other modules:** The change is confined to `src-tauri/src/logging.rs`. No other source file is touched.
- **NFR4 — Documentation accuracy:** The doc comment on `init()` MUST be updated to reflect "in-process filter; does not modify process env" (the existing "Set the env-var only if..." paragraph becomes stale and MUST be rewritten or removed).

## Implementation Approach

### Architecture

The change is a localized rewrite inside one function plus the addition of one private helper:

```
logging.rs
├── const DEFAULT_FILTER: &str = "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn";
├── fn resolved_filters(env_value: Option<&str>) -> String   (NEW)
├── fn init()                                                  (REWRITTEN)
│   ├── INIT.call_once { ... }
│   ├── let filters = resolved_filters(std::env::var("RUST_LOG").ok().as_deref());
│   ├── let mut builder = env_logger::Builder::new();
│   ├── builder.parse_filters(&filters);
│   ├── builder.format( ... )    (unchanged closure)
│   └── builder.try_init();
└── (existing helpers unchanged)
```

No new module, no new dependency, no public API change. `init()` keeps its `pub` signature; `resolved_filters` is private (visibility = file-private or `pub(crate)` only if a test outside the file needs it — not the case here, the unit tests live in the same file).

### Data Flow

Before:
```
init() -> [if RUST_LOG absent] write "info,..." into process env table
       -> Builder::from_env(default_filter_or("info"))
       -> [later] PtySession spawn -> child inherits process env including the just-written RUST_LOG
```

After:
```
init() -> resolved_filters( std::env::var("RUST_LOG").ok() )  (pure)
       -> Builder::new().parse_filters(<result>)
       -> [later] PtySession spawn -> child inherits original process env (RUST_LOG unchanged)
```

### API Design

```rust
// Private helper. Pure. Unit-testable.
fn resolved_filters(env_value: Option<&str>) -> String;

// Public, unchanged signature.
pub fn init();
```

### Database Schema

N/A.

### Dependencies

**Internal:**
- `src-tauri/src/logging.rs` — the only modified file.

**External:**
- `env_logger` crate — already a dependency. `Builder::new` and `parse_filters` are stable API. No version change.

### File Structure

```
src-tauri/
  src/
    logging.rs    # resolved_filters + init() rewrite + tests
```

No new files. No directory changes.

## Test Scenarios

### Unit Tests

Added to the existing `#[cfg(test)] mod tests` in `src-tauri/src/logging.rs`:

- [ ] **TS-1 — `resolved_filters_none_returns_default`**: `resolved_filters(None) == DEFAULT_FILTER`.
- [ ] **TS-2 — `resolved_filters_empty_returns_default`**: `resolved_filters(Some("")) == DEFAULT_FILTER`.
- [ ] **TS-3 — `resolved_filters_passes_user_value`**: `resolved_filters(Some("debug")) == "debug"`.
- [ ] **TS-4 — `resolved_filters_passes_module_scoped_value`**: `resolved_filters(Some("wgpu_core=info,naga=trace")) == "wgpu_core=info,naga=trace"`.

### Integration Tests

None. `init()` mutates global logger state; integration testing across `INIT.call_once` is impractical and not warranted for a refactor this small.

### E2E Tests

This project has no E2E framework (`test/README.md` confirms). Section omitted.

### Manual Testing

- [ ] **TS-5 (Linux) — process env stays clean**: launch a debug build of eMterm in a terminal, attach `strace -e trace=execve` (or check `cat /proc/$(pidof emterm)/environ | tr '\0' '\n' | grep RUST_LOG`) immediately after splash, confirm `RUST_LOG` is **not present** in the eMterm process env. Then open a tab, repeat the check inside the spawned shell — `RUST_LOG` must still be absent.
- [ ] **TS-6 (Windows) — fnm INFO leak resolved**: on a Windows host where the original bug was reproduced (`tmp/issues-windows-mux-2026-06-22.md`), rebuild eMterm with this change, launch, open a pwsh tab with a `$PROFILE` that invokes `fnm env --use-on-cd | Out-String | Invoke-Expression`. Confirm none of the `INFO fnm::version_files` lines appear at startup.
- [ ] **TS-7 — explicit RUST_LOG still propagates**: launch `RUST_LOG=debug emterm` and confirm (a) eMterm's own log emits debug records and (b) a spawned child shell reports `$env:RUST_LOG == "debug"` / `echo $RUST_LOG == "debug"`. eMterm must not strip the user-set value.
- [ ] **TS-8 — log format / persistence unchanged**: in the same debug session, trigger a known `log::warn!` site (e.g. an intentional bad action), confirm the line shape is `[WARN][NATIVE-POC] ...`. In a release build, confirm the same warn record appears in `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.

### Edge Cases

- [ ] Empty `RUST_LOG=`: behavior MUST match "no env value" (use default filter). Covered by TS-2.
- [ ] Garbage `RUST_LOG=nonsense_token`: `env_logger::Builder::parse_filters` already tolerates this (logs an internal warning and falls back to off for that target). No change required.
- [ ] Calling `init()` twice in the same process: `INIT.call_once` short-circuits the second call, no change required.

## Security Considerations

N/A. Local logger initialization. No untrusted input; the existing `unsafe` block was unsafe only because `std::env::set_var` is `unsafe fn` in newer Rust editions — removing the call removes the unsafe.

## Error Handling

`resolved_filters` cannot fail (pure string transformation). `init()` already swallows the `try_init` result; this remains.

## Performance Optimization

Negligible. One fewer process-env write per startup. No hot path affected.

## Success Criteria

- [ ] FR1–FR5 implemented and verified by the new unit tests.
- [ ] `cargo test --lib` passes on Linux.
- [ ] `cargo check` (default features) and `cargo check --no-default-features` both pass.
- [ ] `cargo fmt --check src-tauri/src/logging.rs` clean.
- [ ] Manual TS-5 / TS-7 / TS-8 on Linux pass.
- [ ] Manual TS-6 on Windows pass (the originally reported fnm INFO bug is gone).
- [ ] `unsafe` block count in `logging.rs` decreases by exactly 1.

## Open Questions

None. The approach, scope, and naming were all confirmed up front.

## Implementation Phases

### Phase 1: Refactor `init()` and add `resolved_filters`

**Goals:** Land FR1–FR5 plus NFR1 in a single self-contained patch.

**Deliverables:**
- `resolved_filters(env_value: Option<&str>) -> String` (new, with doc comment).
- `init()` rewritten to call `resolved_filters` and use `Builder::new() + parse_filters`. Doc comment on `init()` updated.
- Four unit tests (TS-1 through TS-4) added to `logging.rs::tests`.
- The `unsafe { std::env::set_var(...) }` block and its surrounding rationale comment removed.

### Phase 2: Manual verification

**Goals:** Confirm the user-visible bug is gone and no regression slipped in.

**Deliverables:**
- TS-5 / TS-7 / TS-8 confirmed on Linux (by the developer).
- TS-6 confirmed on Windows (by the user / on the Windows host where the bug originally appeared).
- Note recorded in VERIFICATION_RESULT.md.

## References

- `tmp/issues-windows-mux-2026-06-22.md` — origin report (problem 4 of 5).
- `src-tauri/src/logging.rs:171-213` — current `init()` implementation.
- `src-tauri/src/pty/mod.rs:162-179` — PTY child env construction (no longer needs RUST_LOG removal once this lands).
- env_logger crate: `Builder::new`, `Builder::parse_filters`, `Builder::format`.
- `doc/tasks/logging-rust-log-isolation/要件定義書.md` — Japanese requirements document.
