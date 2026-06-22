# Implementation Plan: logging.rs RUST_LOG Process Env Isolation

## Overview

Replace `logging::init()`'s `unsafe { std::env::set_var("RUST_LOG", ...) }` startup write with an in-process filter set via `env_logger::Builder::parse_filters`, so the process env table stays clean and child PTY processes (pwsh / fnm / etc.) do not inherit a leaked default `RUST_LOG=info,...`. Extract the filter-string resolution into a pure `resolved_filters` helper and cover it with unit tests.

## Objectives

- Land FR1–FR5 plus NFR1 / NFR4 in a single localized patch (one file).
- Preserve every observable property of the existing logger (FR4 / NFR2).
- Keep the `unsafe` count in `logging.rs` strictly decreasing (NFR1).

## Prerequisites

### Development Environment

- Rust toolchain pinned by `rust-toolchain.toml` (rustfmt style_edition = 2024).
- `env_logger` crate already in `src-tauri/Cargo.toml`; no version change.

### Dependencies

- None added. The patch uses already-imported APIs (`env_logger::Builder::new`, `Builder::parse_filters`, `std::env::var`).

## Architecture Overview

### Technology Stack

- **Language**: Rust (edition 2024).
- **Framework**: native (logger is process-wide global state, set up at main entry).
- **Key Libraries**: `env_logger` (build the logger), `log` (the facade callers go through; unchanged).

### Design Approach

Single file change. Replace one block, add one pure helper, add tests. No new module, no new dependency, no public API change.

### Component Interaction

```
main() -> logging::init()
            INIT.call_once {
              filters = resolved_filters( std::env::var("RUST_LOG").ok().as_deref() )   [pure]
              Builder::new()
                .parse_filters(&filters)
                .format(|...|)           [unchanged closure]
                .try_init()
            }
```

No other module touched. PTY spawn (`src-tauri/src/pty/mod.rs`) and the env_remove list stay as-is — the child no longer needs `RUST_LOG` removal because nothing writes it.

## Implementation Phases

### Phase 1: Rewrite `init()` + add `resolved_filters` + tests

**Goal**: Land FR1 / FR2 / FR3 / FR4 / FR5 / NFR1 / NFR4 in one patch. Single file, no new files, no module reorganization.

**Files to Create**:

- (none)

**Files to Modify**:

- `src-tauri/src/logging.rs`
  - Add private `const DEFAULT_FILTER: &str = "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn"`.
  - Add private pure helper `resolved_filters(env_value: Option<&str>) -> String`.
  - Rewrite the body of `init()`'s `INIT.call_once { ... }` to: read `RUST_LOG`, call `resolved_filters`, build with `Builder::new() + parse_filters`, retain the existing format closure and `try_init()`.
  - Remove the `unsafe { std::env::set_var(...) }` block and the surrounding rationale comment about "Set the env-var only if...".
  - Update the `init()` doc comment to say "in-process filter; the process env table is not modified".
  - Add four unit tests to the existing `#[cfg(test)] mod tests`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `DEFAULT_FILTER` const | Single source of truth for the default filter string | n/a (compile-time constant) | Module-private `&str` equal to `"info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn"` |
| `resolved_filters(env_value)` | Decide the final filter string | `env_value` is any `Option<&str>` | Returns `DEFAULT_FILTER.to_string()` if `env_value` is `None` or `Some("")`; otherwise returns `env_value.unwrap().to_string()` |
| Rewritten `init()` body | Set the global logger with the chosen filter, preserving format / persistence behavior | First call: `INIT.call_once` block executes. Subsequent calls: short-circuit | env-table NOT mutated; logger initialized with `parse_filters(&resolved)`; format closure attached |

**Processing Flow** (diagram-convertible):

1. `init()` is called.
2. `INIT.call_once` opens.
3. Read `std::env::var("RUST_LOG")`:
   - `Ok(s)` → pass `Some(s.as_str())` to `resolved_filters`.
   - `Err(_)` → pass `None`.
4. `resolved_filters` returns a `String` (DEFAULT_FILTER if value is None or empty; otherwise the value as-is).
5. Build `env_logger::Builder::new()`, call `parse_filters(&filters)`.
6. Attach the existing `format` closure (writes `[LEVEL][NATIVE-POC] message` and pipes warn/error to `write_to_log_file` in release builds).
7. Call `builder.try_init()`, ignore the result.

**Implementation Steps** (5–7 max):

1. **Add `DEFAULT_FILTER` constant** at module level (above or near `init()`). Single source so the helper, init body, and tests share one literal.
2. **Add `resolved_filters` helper** with doc comment + the contract from FR3. Pure; takes `Option<&str>`, returns `String`.
3. **Rewrite the inside of `INIT.call_once`**: remove the `unsafe { std::env::set_var(...) }` block, replace the `Builder::from_env(...)` call with `Builder::new()` + `parse_filters(&resolved_filters(std::env::var("RUST_LOG").ok().as_deref()))`. Keep the `.format(...)` closure verbatim. Keep the `let _ = builder.try_init();` line verbatim.
4. **Update the `init()` doc comment** to reflect the new behavior (drop the "Set the env-var only if..." paragraph; add "the process env table is not modified").
5. **Add four unit tests** to the existing `#[cfg(test)] mod tests`: `resolved_filters_none_returns_default`, `resolved_filters_empty_returns_default`, `resolved_filters_passes_user_value`, `resolved_filters_passes_module_scoped_value`.
6. **Run `cargo check` + `cargo test --lib`** to confirm no breakage. Run `cargo fmt --check src-tauri/src/logging.rs` to confirm formatting.

**Dependencies**: None. Blocks Phase 2 (manual verification).

**Testing Approach**:

- Unit: TS-1 (None → default), TS-2 (Some("") → default), TS-3 (Some("debug") → "debug"), TS-4 (Some("wgpu_core=info,naga=trace") → unchanged).
- Integration: none.
- E2E: none (no E2E framework in this project).
- Manual: deferred to Phase 2 / `sdd.6-verify` (TS-5 through TS-8).

**Acceptance Criteria**:

- [ ] `logging.rs` no longer contains `std::env::set_var`.
- [ ] `logging.rs` has exactly one fewer `unsafe` block than before (net -1).
- [ ] `resolved_filters` exists with the contract from FR3 and is private.
- [ ] All four unit tests pass.
- [ ] `cargo check` (default and `--no-default-features`) passes.
- [ ] `cargo fmt --check src-tauri/src/logging.rs` clean.
- [ ] Doc comment on `init()` is updated.

**Estimated Effort**: small.

---

### Phase 2: Manual verification (Linux + Windows)

**Goal**: Confirm the originally reported bug is resolved and no regression in eMterm's own logger output.

**Files to Modify**:

- `doc/tasks/logging-rust-log-isolation/VERIFICATION_RESULT.md` (created by `sdd.6-verify`).

**Implementation Steps**:

1. **TS-5 (Linux)**: launch eMterm, check `/proc/$(pidof emterm)/environ` for absence of `RUST_LOG`. Open a tab, check the spawned shell's env (`env | grep RUST_LOG` or `echo $RUST_LOG`). Both should be absent.
2. **TS-6 (Windows)**: on the host where the fnm INFO leak was originally reported, install the new build, open a pwsh tab with the original `$PROFILE` containing the `fnm env --use-on-cd` hook. Confirm none of the `INFO  fnm::version_files` lines appear at startup.
3. **TS-7**: launch `RUST_LOG=debug emterm`, confirm eMterm emits debug-level lines, and that a spawned child shell reports `RUST_LOG=debug` (proving the explicit-set value still propagates — eMterm doesn't strip the user's intent).
4. **TS-8**: in the same debug session, trigger a known `log::warn!` site, confirm format is `[WARN][NATIVE-POC] ...` and (in a release build) that the same line lands in `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.
5. **Record results in `VERIFICATION_RESULT.md`**: pass/fail per TS-5..TS-8 with the observed evidence.

**Dependencies**: Requires Phase 1 merged and the relevant build (Linux GUI for TS-5/TS-7/TS-8; Windows GUI for TS-6).

**Acceptance Criteria**:

- [ ] TS-5 passes on Linux.
- [ ] TS-6 passes on Windows (fnm INFO lines gone).
- [ ] TS-7 passes on Linux.
- [ ] TS-8 passes on Linux (release build for the log-file half).

**Estimated Effort**: small.

---

## Complete File Structure

```
src-tauri/
  src/
    logging.rs              # resolved_filters + DEFAULT_FILTER + rewritten init() body + tests (Phase 1)
doc/
  tasks/
    logging-rust-log-isolation/
      要件定義書.md
      SPEC.md
      IMPLEMENTATION.md     # this document
      VERIFICATION.md
      sdd.yaml
      tasks.yaml
      VERIFICATION_RESULT.md (created during sdd.6-verify)
```

No new source files. No new modules. No new crates.

## Testing Strategy

- **Unit**: 100% coverage of the new `resolved_filters` helper (it has exactly three input classes — None, Some(""), Some(non-empty) — and all are covered).
- **Integration**: none. `init()`'s side effects (global logger registration) are not unit-testable across `INIT.call_once`; integration testing would require a sub-process per case, which is disproportionate for a refactor this small.
- **E2E**: none (no E2E framework in the project).
- **Manual**: Linux process-env inspection (TS-5), Windows pwsh+fnm reproduction (TS-6), explicit-RUST_LOG propagation (TS-7), format/log-file persistence (TS-8).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| env_logger | (existing) | Build the logger. Uses already-available `Builder::new`, `Builder::parse_filters`, `Builder::format`. No version change. |

No new dependencies.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `parse_filters` semantics differ subtly from `from_env(default_filter_or)` (e.g. handling of empty string) | Low | Low | The unit tests pin `resolved_filters` behavior. If runtime behavior differs, TS-8 catches it (eMterm's own log lines change). |
| Some other source file reads `std::env::var("RUST_LOG")` and relied on the old "self-poisoning" behavior | Very Low | Medium | Grep verifies during Phase 1 (the implementation step). Today only `logging.rs:178-200` references it. |
| Future Rust edition / clippy lint flags the new code differently | Low | Low | No new attributes added; the change is to remove an `unsafe` block, which is monotonically less likely to lint than before. |

## Open Questions

None — the SPEC's Open Questions section is empty and the verify-plan step did not add any.

## Success Metrics

- [ ] FR1–FR5 implemented, all four unit tests pass.
- [ ] `cargo check` (default + `--no-default-features`) green; `cargo test --lib` green; `cargo fmt --check` clean.
- [ ] Manual TS-5 / TS-7 / TS-8 pass on Linux; TS-6 passes on Windows.
- [ ] `logging.rs` `unsafe` count decreased by exactly 1.
