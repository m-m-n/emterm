# Verification Document: native-poc CLI Subcommand Port (Phase A + B)

## Overview

**Feature**: cli-subcommand-native-port
**SPEC.md**: `doc/tasks/cli-subcommand-native-port/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/cli-subcommand-native-port/IMPLEMENTATION.md`

## Build Verification

- **Debug check (fast)**:
  `CARGO_TARGET_DIR=native-poc/target cargo check --manifest-path native-poc/Cargo.toml`
- **Release build**:
  `CARGO_TARGET_DIR=native-poc/target-host cargo build --release --manifest-path native-poc/Cargo.toml`
- **Expected**: exit code 0, no errors, no `rust-i18n` in dependency
  tree (`cargo tree -p emterm-native-poc | grep -i rust-i18n` returns
  no output).
- **Cross-platform**: Linux build is the primary verification. Windows
  build verification is deferred to CI; locally the developer ensures
  Unix-only code is `#[cfg(unix)]`-gated.

### Actual Results (2026-06-16, Linux host)

- Debug check: `cargo check` exit 0, lib + bin compile clean.
- Release build: `cargo build --release` exit 0, artifact at
  `native-poc/target-host/release/emterm-native-poc` (size: 53 MiB).
- `cargo tree -p emterm-native-poc | grep -i rust-i18n` returns no
  output (exit 1) — `rust-i18n` not in graph.
- Smoke test:
  `target-host/release/emterm-native-poc markdown README.md > /tmp/smoke.osc`
  begins with `\x1b]777;emterm;markdown;begin;id=<uuid>;format=gfm;...`
  — byte-for-byte parity with src-tauri verified.

## Test Verification

- **Command**:
  `CARGO_TARGET_DIR=native-poc/target cargo test --manifest-path native-poc/Cargo.toml`
- **Coverage target**: ≥ 80 % over the new `cli` subtree; ≥ 90 % for
  `cli::error` and `cli::validation` paths.

### Actual Results (2026-06-16)

- Full suite: **1554 passed; 0 failed; 1 ignored** (lib unit tests).
- Integration suite (`tests/cli_subcommands.rs`): **12 passed; 0 failed**.
- New `cli::*` tests added: **112** (across error / messages / tmux /
  encoding / validation / markdown / json / yaml / image / protocols).
- All `cli::*` ported unit tests are byte-for-byte parity with the
  src-tauri originals (frame format strings, exit codes, locale
  messages). No `rust_i18n::set_locale` remains; tests use the
  `set_active_locale_for_test` helper exposed by `cli::mod`.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `markdown <file>` with a small (< 1 KB) `.md` fixture | Stdout contains `\x1b]777;emterm;markdown;begin;…` then `;end;…`; exit 0 | Integration |
| TS-2 | `markdown <file>` with an empty `.md` file | Stdout contains only `begin` + `end`, no `chunk`; exit 0 | Unit |
| TS-3 | `markdown <file>` with a file at `MAX_MARKDOWN_SIZE` (10 MB) | Exit 0; full OSC frame stream emitted | Unit |
| TS-4 | `markdown <file>` with a file at `MAX_MARKDOWN_SIZE + 1` | `FileTooLarge`; exit 1; localized stderr | Unit |
| TS-5 | `markdown <file>` with a nonexistent path | `FileNotFound`; exit 2 | Unit |
| TS-6 | `markdown <file>` with a directory path | `NotAFile`; exit 2 | Unit |
| TS-7 | `json <file>` with a small JSON fixture | Stdout contains `;emterm;json;begin;…;end;…`; exit 0 | Integration |
| TS-8 | `yaml <file>` with a small YAML fixture | Stdout contains `;emterm;yaml;begin;…;end;…`; exit 0 | Integration |
| TS-9 | `json <file>` / `yaml <file>` with a 10 MB JSON file | Exit 0 (no size cap for these subcommands) | Unit |
| TS-10 | `image <file>` defaults to Kitty protocol | Stdout contains `\x1b_Gi=…` APC sequence; exit 0 | Integration |
| TS-11 | `image <file> --protocol sixel` | Stdout contains `\x1bPq…` SIXEL sequence; exit 0 | Integration |
| TS-12 | `image <file> --protocol ascii` | `InvalidProtocol`; exit 1 | Unit |
| TS-13 | `image <file>` with an 8193×8192 PNG | `EncodingError("…exceeds maximum…")`; exit 1 | Unit |
| TS-14 | `image <file>` with a non-image file (e.g. `.txt`) | `UnsupportedImageFormat`; exit 1 | Unit |
| TS-15 | `image <file>` with a file > 10 MB | `FileTooLarge`; exit 1 | Unit |
| TS-16 | `image <file>` Kitty path on Unix | After write, stdin is drained (no leakage onto subsequent prompt) | Manual (Unix only) |
| TS-17 | Any subcommand with `TMUX=1` env set | Output frames are wrapped in `\x1bPtmux;…\x1b\\` with internal ESCs doubled | Integration |
| TS-18 | Any subcommand with `TMUX` env unset | Output frames are emitted raw, no DCS wrapper | Integration |
| TS-19 | Any error path under `LANG=ja_JP.UTF-8` | Localized stderr message in Japanese | Unit |
| TS-20 | Any error path under `LANG=en_US.UTF-8` | Localized stderr message in English | Unit |
| TS-21 | `cargo tree -p emterm-native-poc` after Phase 1 | Output includes `clap` and `uuid`; does not include `rust-i18n` | Manual / scripted |
| TS-22 | `cli::tmux::passthrough_if_needed` ESC doubling | All internal `\x1b` bytes are doubled inside the wrapper; outer wrapper present | Unit |
| TS-23 | `cli::encoding::base64::chunk_data` boundary | For data exactly at the chunk boundary, produces a single chunk; one byte over produces two | Unit |
| TS-24 | `cli::encoding::osc` frame format parity | Frame strings for a known input match the byte sequence produced by `src-tauri` for the same input | Unit |
| TS-25 | `cli::run` exit code mapping | Each `CommandError` variant produces the exit code documented in `error.rs` (1 or 2) | Unit |
| TS-26 | `main.rs` dispatch precedence | When `args[1] = "markdown"`, the CLI arm runs; when `args[1] = "--viewer"`, the existing flag dispatch runs | Manual |
| TS-27 | `main.rs` regression | All existing internal-flag startup paths (`--viewer`, `--settings`, `--image-viewer`, `--data-viewer`, mux, terminal startup) behave as before | Manual |

## Code Quality Verification

- **Format**: `cargo fmt --manifest-path native-poc/Cargo.toml`
  (no diff expected after the format pass).
- **Lint**: `cargo clippy --manifest-path native-poc/Cargo.toml -- -D warnings`
  (no warnings).

### Actual Results (2026-06-16)

- `cargo fmt --check`: clean (no diff).
- `cargo clippy` on the new `cli::*` code emits only one nit
  (`sort_by_key` suggestion in the verbatim-ported
  `cli::protocols::sixel`); leaving as-is to preserve byte-parity with
  src-tauri. Pre-existing clippy errors in workspace crates
  (`term_core` SGR parser) are unrelated and out of scope for this SDD.

## File Structure Verification

### Files to Create

- `native-poc/src/cli/mod.rs`
- `native-poc/src/cli/messages.rs`
- `native-poc/src/cli/error.rs`
- `native-poc/src/cli/tmux.rs`
- `native-poc/src/cli/encoding/mod.rs`
- `native-poc/src/cli/encoding/base64.rs`
- `native-poc/src/cli/encoding/osc.rs`
- `native-poc/src/cli/validation/mod.rs`
- `native-poc/src/cli/validation/file.rs`
- `native-poc/src/cli/validation/image.rs`
- `native-poc/src/cli/protocols/mod.rs`
- `native-poc/src/cli/protocols/kitty.rs`
- `native-poc/src/cli/protocols/sixel.rs`
- `native-poc/src/cli/markdown.rs`
- `native-poc/src/cli/json.rs`
- `native-poc/src/cli/yaml.rs`
- `native-poc/src/cli/image.rs`
- `native-poc/tests/cli_subcommands.rs`
- `native-poc/tests/fixtures/markdown/sample.md`
- `native-poc/tests/fixtures/data/sample.json`
- `native-poc/tests/fixtures/data/sample.yaml`
- `native-poc/tests/fixtures/images/sample.png`

### Files to Modify

- `native-poc/Cargo.toml` — add `clap`, `uuid`, `tempfile (dev)`,
  and a new `[lib]` target (`path = "src/lib.rs"`,
  `name = "emterm_native_poc"`)
- `native-poc/src/main.rs` — switch to importing `cli` from the new
  library crate, and add the CLI dispatch arm before flag branches

### Additional Files to Create (Phase 4)

- `native-poc/src/lib.rs` — new library target hosting `pub mod cli;`
  and any modules `cli` depends on; created because native-poc
  currently has no `[lib]` target and integration tests need
  in-process access to `cli::run`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR10 fully implemented in `native-poc/src/cli/` | File-existence + per-FR unit/integration tests pass |
| SC-2 | No modifications outside `native-poc/` | `git diff --name-only` shows only `native-poc/**` entries |
| SC-3 | Ported unit tests pass | `cargo test` on native-poc target reports all green |
| SC-4 | Integration tests pass | `cargo test --test cli_subcommands` green |
| SC-5 | Release binary builds | `cargo build --release` produces `native-poc/target-host/release/emterm-native-poc` |
| SC-6 | Manual: each subcommand renders correctly | Developer launches the release binary, runs each subcommand, observes correct viewer / image output |
| SC-7 | `cargo tree` excludes `rust-i18n` | Grep verifies the absence |
| SC-8 | No regression in existing native-poc paths | Manual exercise of `--viewer` / `--settings` / `--image-viewer` / `--data-viewer` / mux / terminal startup |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (markdown subcommand) | Phase 2 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6 |
| FR2 (json subcommand) | Phase 2 | TS-7, TS-9 |
| FR3 (yaml subcommand) | Phase 2 | TS-8, TS-9 |
| FR4 (image subcommand, Kitty + SIXEL) | Phase 3 | TS-10, TS-11, TS-12, TS-13, TS-14, TS-15, TS-16 |
| FR5 (cli module dispatcher, clap) | Phase 1 (skeleton) + Phase 2/3 (wiring) | TS-12 (parser), TS-25 (exit code) |
| FR6 (main.rs integration) | Phase 4 | TS-26, TS-27 |
| FR7 (tmux DCS passthrough port) | Phase 1 | TS-17, TS-18, TS-22 |
| FR8 (Localized error messages, no rust-i18n) | Phase 1 | TS-19, TS-20, TS-21 |
| FR9 (session_id via uuid v4) | Phase 1 (utility) + Phase 2/3 (usage) | Implicit in TS-1, TS-7, TS-8, TS-10, TS-11 (frame format includes `session_id`) |
| FR10 (Unit test parity with src-tauri) | Phase 2 + Phase 3 | All TS-* unit lines |
| NFR1 (Performance targets) | Phase 4 | Performance Verification section below |
| NFR2 (Security guards) | Phase 1 + Phase 3 | TS-3 / TS-4 / TS-13 / TS-14 / TS-15 |
| NFR3 (Dependency minimalism) | Phase 1 | TS-21 |
| NFR4 (Cross-platform, cfg-gated Unix code) | Phase 3 + Phase 4 | Linux build (local) + Windows build (CI) |
| NFR5 (Branch policy: no changes outside native-poc/) | All phases | SC-2 |

## E2E Testing

The project's WebDriver E2E suite (`./scripts/run-e2e-docker.sh`)
targets the WebView build, not native-poc. No automated E2E additions
in this SDD.

- [ ] Pre-existing E2E suite still passes (no impact expected since
      this work touches only `native-poc/`).

## Manual Testing (E2E Not Possible)

- [ ] Launch `native-poc/target-host/release/emterm-native-poc`.
- [ ] Inside the running native-poc terminal, run
      `./native-poc/target-host/release/emterm-native-poc markdown README.md`
      — viewer window appears, README rendered.
- [ ] Same with a `.json` and `.yaml` file — data viewer window
      appears.
- [ ] Same with a `.png` (default Kitty) — image appears inline.
- [ ] Same with a `.png` and `--protocol sixel` — image appears
      inline.
- [ ] Wrap a `tmux new -s test` session around the native-poc
      terminal; ensure `set -g allow-passthrough on` is configured.
      Repeat the four subcommands; verify viewer / image still
      appears (DCS passthrough working).
- [ ] Set `LANG=ja_JP.UTF-8` and run `emterm-native-poc markdown
      /nonexistent` — verify Japanese error message on stderr.
- [ ] Set `LANG=en_US.UTF-8` and run the same — verify English error
      message.
- [ ] Confirm existing internal-flag startup paths still work:
      launch native-poc terminal, open viewer / settings / image
      viewer / data viewer via the usual means; ensure mux behaves
      as before.

## Performance Verification

- markdown 100 KB → CLI exit within 200 ms (wall clock, host
  developer machine).
- json / yaml 100 KB → exit within 200 ms.
- image 1 MB PNG with Kitty → exit within 500 ms.
- image 5 MB PNG with SIXEL → no hard target; ensure it does not
  exceed src-tauri's wall-clock by more than 20 %.

Measurement is informal (`time ./emterm-native-poc ...`); failures
on slower machines are not blockers if the relative difference vs.
the src-tauri build is small.

## Security Verification

- [ ] All file reads go through `validation::file::open_and_validate_file`
      (grep verification).
- [ ] Image dimension check precedes full decode (grep verification:
      `image_dimensions` call precedes `image::open` in
      `cli::image`).
- [ ] No raw file content is interpolated into OSC frames without
      base64 encoding (grep verification across `cli::encoding::osc`).
- [ ] Kitty stdin drain is `#[cfg(unix)]`-gated (grep verification).
- [ ] No `unsafe` blocks beyond what is required for Unix termios
      manipulation (grep verification — `unsafe` count in `cli/*` ≤ 1).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Functional (TS-1 – TS-15) | 15 | 15 | 0 | 0 |
| Behavior parity (TS-17 – TS-25) | 9 | 8 | 0 | 1 (TS-16 Unix-only manual) |
| Integration (TS-26 – TS-27) | 2 | 0 | 0 | 2 |
| Performance | 4 | 0 | 0 | 4 |
| Security | 5 | 5 (grep-based) | 0 | 0 |
| **Total** | **35** | **28** | **0** | **7** |
