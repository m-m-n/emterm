# Implementation Plan: Windows Esc Key via Win32 Input Mode

## Overview

Replace the single-byte `0x1b` encoding of the Escape key on Windows with a Win32 Input Mode VT key event sequence (key-down + key-up pair) so that ConPTY — opened by portable-pty 0.8 with `PSEUDOCONSOLE_WIN32_INPUT_MODE` — delivers a proper Escape key event to TUI applications. Also audit other keys that may suffer from the same bare-`0x1b` ambiguity and fix the ones demonstrably broken.

## Objectives

- Land FR1 / FR2 / FR4 / FR5 (Esc encoder + modifier support + non-Windows parity + unit tests).
- Land FR3 (audit other keys, fix only the broken ones, document the audit).
- Preserve `encode()`'s public signature and all call sites.

## Prerequisites

### Development Environment

- Rust toolchain pinned by `rust-toolchain.toml` (rustfmt style_edition = 2024).
- Windows cross-build chain: `cargo-xwin` + `x86_64-pc-windows-msvc` target (`make setup`).
- A Windows host (or VM / remote machine) for manual verification of FR3's vim test.

### Dependencies

- `portable-pty` 0.8.1 — already a direct dependency. No version change.
- No new crate dependencies introduced.
- Existing `encode_backspace_win32()` and its tests (`src-tauri/src/pty/input.rs:141-173`, `:243-267`) serve as the implementation template.

## Architecture Overview

### Technology Stack

- **Language**: Rust (edition 2024 per `rust-toolchain.toml`).
- **Framework**: native (no GUI framework involved for this change; `winit` only feeds key events into the encoder).
- **Key Libraries**: `portable-pty` 0.8.1 for ConPTY interaction; standard library `format!` for sequence construction.

### Design Approach

The encoder `pty::input::encode(key, mods) -> Vec<u8>` already uses a Windows-only early-return pattern for Backspace. The new code follows the same pattern for Escape and any additional keys discovered in Phase 2. Linux / macOS code paths remain in the existing match arm. All Windows-only helpers live behind `#[cfg(windows)]` guards.

### Component Interaction

```
winit KeyboardInput
  -> window_host translates to (Key, Modifiers)
  -> pty::input::encode(key, mods)
     -> (Windows) early-return shims for Backspace, Escape, (Phase 2 additions)
     -> (other paths) existing match
  -> Tab::write_input(bytes)
  -> PtySession writer -> ConPTY in WIN32_INPUT_MODE
  -> child shell / TUI receives KEY_EVENT_RECORD
```

No new components. The change is confined to one source file.

## Implementation Phases

### Phase 1: encode_escape_win32 + early-return shim + unit tests

**Goal**: Windows builds emit a Win32 Input Mode VT key event pair for Escape with full modifier propagation, and the change is covered by unit tests on both Windows and non-Windows targets.

**Files to Create**:

- (none — all changes live in an existing file)

**Files to Modify**:

- `src-tauri/src/pty/input.rs` — add `encode_escape_win32`, add the Windows early-return shim for Escape inside `encode()`, add unit tests, ensure the existing `enter_tab_backspace_escape` test still holds on non-Windows.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `encode_escape_win32(mods)` | Build the Win32 Input Mode key-down + key-up byte pair for Escape with `Cs` reflecting modifiers | `mods` is well-formed (any combination of ctrl/shift/alt) | Returns the ASCII pair `ESC[27;1;27;1;{Cs};1_ESC[27;1;27;0;{Cs};1_` (≈32 bytes; 32-34 depending on the digit count of `Cs`) with `Cs` correctly OR'd from modifier bits |
| `encode()` early-return shim | Route Windows Escape through `encode_escape_win32` before the Alt-prefix logic | Caller passed `Key::Escape` on a Windows build | Caller receives the Win32 Input Mode byte pair instead of the bare `\x1b` |

**Processing Flow** (diagram-convertible):

1. `encode(key, mods)` is called.
2. On Windows:
   - If `key == Backspace` -> return `encode_backspace_win32(mods)` (existing).
   - If `key == Escape` -> return `encode_escape_win32(mods)` (new).
   - Otherwise -> fall through.
3. Apply Alt-prefix logic (unchanged).
4. Match on `key`:
   - Escape -> `push(0x1b)` (now reachable only on non-Windows).
   - Other arms unchanged.
5. Return `out`.

**Implementation Steps** (5–7 max):

1. **Add `encode_escape_win32(mods)` helper** — Construct the down+up pair using the same modifier-bit assignment as `encode_backspace_win32` (Shift = 0x10, LCtrl = 0x08, LAlt = 0x02). Include a doc comment explaining the WIN32_INPUT_MODE rationale and pointing to Microsoft Terminal spec #4999.
2. **Insert the Escape early-return shim in `encode()`** — Immediately after the existing Backspace shim, gated by `#[cfg(windows)]`.
3. **Add Rust unit tests** — One test per scenario in the Test Scenarios table (TS-1 … TS-6). Mirror the structure and naming of the existing `backspace_emits_win32_input_mode_pair` and `backspace_win32_includes_modifier_bits` tests.
4. **Tighten the existing `enter_tab_backspace_escape` test** — Add a `#[cfg(not(windows))]` guard around the Escape assertion (analogous to how Backspace is already guarded). Confirms non-Windows parity (FR4).
5. **Run quick check + tests on Linux host** — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` and `cargo test --lib`. The Windows-gated tests do not run, but the non-Windows side stays green.
6. **Run Windows cross-build** — `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml` to confirm the Windows path compiles. (Cross-build does not execute Windows tests; those run during manual verification in Phase 3 / sdd.6-verify.)

**Dependencies**: None. Blocks Phase 2 and the manual-verification work in sdd.6-verify.

**Testing Approach**:

- Unit: TS-1 (Esc no-mod sequence), TS-2 (Ctrl Cs=8), TS-3 (Alt Cs=2), TS-4 (Shift Cs=16), TS-5 (Ctrl+Shift Cs=24), TS-6 (non-Windows still `b"\x1b"`).
- Integration: none.
- E2E: none (no E2E framework in this project).
- Manual: deferred to sdd.6-verify; see TS-7 / TS-8.

**Acceptance Criteria**:

- [ ] `encode_escape_win32` exists, signature `fn encode_escape_win32(mods: Modifiers) -> Vec<u8>`.
- [ ] `encode()` returns the Win32 Input Mode byte pair for Escape on Windows builds.
- [ ] All TS-1 … TS-6 unit tests pass on their respective targets.
- [ ] Existing tests in `pty/input.rs` are not weakened (they may be reshaped with `#[cfg]` guards but assertions remain).
- [ ] Linux `cargo test --lib` is green.
- [ ] Windows cross-build compiles cleanly.

**Estimated Effort**: small.

---

### Phase 2: WIN32_INPUT_MODE other-key audit (FR3)

**Goal**: Determine which other keys in `encode()` may be misinterpreted under WIN32_INPUT_MODE, fix the ones that are broken in practice, and record the audit outcome in IMPLEMENTATION.md.

**Files to Create**:

- (none expected; if a new helper is needed it lives in `pty/input.rs`)

**Files to Modify**:

- `src-tauri/src/pty/input.rs` — only if the audit identifies a broken key.
- `doc/tasks/windows-esc-win32-input-mode/IMPLEMENTATION.md` — append an "Audit Notes" subsection documenting the candidates examined and the verdict for each.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Audit checklist | Enumerate every `encode()` arm that emits a leading `0x1b` byte and classify it as "full CSI/SS3 (safe)" vs "bare/ambiguous (verify)" | Phase 1 is merged so the baseline behavior is known | Each arm has a recorded verdict; broken arms are paired with a follow-up encoder helper |
| Optional new helper(s) | If a broken key is found, emit its Win32 Input Mode key event sequence in the same shape as `encode_escape_win32` / `encode_backspace_win32` | Audit verdict says "broken in practice" | Broken key now behaves like Backspace/Escape on Windows |

**Processing Flow** (diagram-convertible):

1. Enumerate candidates from `pty/input.rs::encode`:
   - Bare-leading-`0x1b`: Escape (already handled by Phase 1).
   - Alt-prefix path: any `Char(c)` with `mods.alt == true` becomes `0x1b` + UTF-8 of `c`. Candidate for ambiguity.
   - Multi-byte CSI/SS3 sequences (arrows, Home/End/PageUp/PageDown, Delete/Insert, F1-F12, Shift+Tab): these emit a complete sequence in a single `write_input`. Expected to be safe.
2. Manual exercise on Windows for each candidate:
   - Alt+letter chord: type `Alt+b`, `Alt+f`, `Alt+x` (xdotool / on-keyboard) in pwsh + PSReadLine. Confirm whether PSReadLine recognizes them or whether they degrade to Esc-then-letter.
   - Arrow / nav / F-keys: type each in pwsh / vim and confirm behavior matches an Esc-fixed terminal.
3. For each candidate:
   - "Safe in practice" -> record in Audit Notes.
   - "Broken in practice" -> add a new `encode_<key>_win32` helper following the Backspace/Escape pattern. Add unit tests. Add a manual verification scenario to VERIFICATION.md.
4. Update `IMPLEMENTATION.md` Audit Notes subsection with the verdict for every candidate, even safe ones.

**Implementation Steps** (5–7 max):

1. **Build the candidate list** — Walk `encode()`'s arms and the Alt-prefix branch; group each by whether it emits a single `0x1b` byte or a longer sequence.
2. **Author manual reproduction recipes** — Short instructions per candidate (which key chord to press in which shell), so the audit can be executed in one Windows session.
3. **Execute the audit on Windows** — User runs the recipes and reports verdicts (this is where Phase 2 may stop if everything else works).
4. **For each broken candidate** — add the encoder helper (mirroring Phase 1's helper), the unit tests, and a manual verification scenario.
5. **Document the audit** — append the verdict table to IMPLEMENTATION.md (new subsection "Audit Notes (FR3)").

**Dependencies**: Requires Phase 1 merged so the Esc fix doesn't confound audit results. Blocks the sdd.6-verify manual checks for Phase 2.

**Testing Approach**:

- Unit: one test per newly added encoder helper, if any.
- Integration: none.
- E2E: none.
- Manual: TS-9 (Alt+letter chord behaves correctly), TS-10 (arrow / nav / F-keys behave correctly). These are verified by the user on a Windows host.

**Acceptance Criteria**:

- [ ] Audit Notes subsection added to IMPLEMENTATION.md listing every `encode()` candidate examined with its verdict.
- [ ] Any broken candidate ships with a helper + unit tests + manual scenario in VERIFICATION.md.
- [ ] If no broken candidate is found, the audit explicitly states so and the section is signed off.

**Estimated Effort**: small (audit-only path) to medium (if multiple keys turn out broken).

---

## Complete File Structure

```
src-tauri/
  src/
    pty/
      input.rs              # encode_escape_win32 added + early-return shim + tests (Phase 1)
                            # potentially additional encode_*_win32 helpers + tests (Phase 2)
doc/
  tasks/
    windows-esc-win32-input-mode/
      要件定義書.md
      SPEC.md
      IMPLEMENTATION.md     # this document
      VERIFICATION.md
      sdd.yaml
      tasks.yaml
      (Phase 2 audit notes appended to IMPLEMENTATION.md)
```

No new modules, no new crates, no build-script touches.

## Testing Strategy

- **Unit**: encoder-level. Every byte pattern asserted explicitly. Target coverage for the touched code path is 100% (the function is short and pure).
- **Integration**: none. The encoder is exercised by GUI code that has no automated end-to-end harness in this repo.
- **E2E**: none (`test/README.md` confirms no E2E framework in the project).
- **Manual**: Windows host verification of vim / less / Alt+letter chord. Tracked in VERIFICATION.md's "Manual Testing" section.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| portable-pty | 0.8.1 | Existing dependency. ConPTY backend with WIN32_INPUT_MODE always on. No version change. |

No new dependencies. No build-script changes.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Microsoft Terminal's WIN32_INPUT_MODE Esc handling differs from the assumed `VK_ESCAPE=27, Sc=1` mapping | Low | Medium | Mirror the proven Backspace pattern. Verify on Windows during sdd.6-verify; adjust Vk / Sc / Uc if vim still misbehaves. |
| Alt+letter chord ends up needing the same Win32 Input Mode treatment, expanding Phase 2 scope | Medium | Low | The audit is the explicit gate; we only add code if breakage reproduces. |
| Future portable-pty upgrade disables WIN32_INPUT_MODE | Low | Medium | The Win32 Input Mode VT key event is still recognized by ConPTY in many configurations. If a regression occurs, gate the new helpers behind a feature flag or auto-detect. |
| Esc-heavy input (e.g. mash Esc 20 times) produces noticeable lag because each key now writes ~35 bytes instead of 1 | Very Low | Low | Same byte count as Backspace, which has shipped without complaints. |

## Open Questions

- [ ] FR3 audit: which exact set of additional keys (if any) require Win32 Input Mode encoding? Resolved during Phase 2 by user verification.

## Success Metrics

- [ ] Functional completeness: all FR1 / FR2 / FR3 / FR4 / FR5 marked verified in VERIFICATION_RESULT.md.
- [ ] Quality metrics: `cargo test --lib` green on Linux; Windows cross-build green; no rustfmt diffs.
- [ ] Performance metrics: per-keystroke encode latency unchanged at the scale of a `format!` call (no benchmark required).

---

## Audit Notes (FR3)

The table below lists every `encode()` arm that emits a leading `0x1b` byte under WIN32_INPUT_MODE. Rows marked `safe (mechanical)` are determined from inspecting the encoder alone: any branch that emits a complete CSI (`ESC [ ...`) or SS3 (`ESC O ...`) sequence in a single `write_input` call is interpreted by ConPTY's Win32 Input Mode parser as a VT escape sequence and forwarded intact — this is the explicit design of the mode. Rows marked `requires Windows runtime confirmation` need TS-9 / TS-10 manual reproduction on a Windows host because the failure depends on real ConPTY timing / PSReadLine policy, not on a property visible from source. Rows marked `fixed (Phase 1)` / `fixed (Phase 1 follow-up)` are guaranteed safe by code paths added in this feature.

| Candidate (encode() arm) | Sequence emitted today | Expected behavior on Windows | Verdict | Follow-up |
|--------------------------|------------------------|------------------------------|---------|-----------|
| `Escape` (no modifier) | `ESC[27;1;27;Kd;Cs;1_` pair via `encode_escape_win32` | Escape key event in vim / TUI | fixed (Phase 1) | none |
| `Escape` + modifiers | same pair with `Cs` reflecting Ctrl/Alt/Shift | Escape key event with modifier bits | fixed (Phase 1) | none |
| `Backspace` (any modifier) | `ESC[8;14;8;Kd;Cs;1_` pair via `encode_backspace_win32` | single-character delete in PSReadLine | safe (already fixed pre-feature) | none |
| `Char('[')` with Ctrl (== vim's `i_CTRL-[`) | previously `0x1b` via `ctrl_byte`; now `encode_escape_win32(mods)` | Escape key event with optional Ctrl bit in `Cs` | fixed (Phase 1 follow-up) | none |
| `Char(c)` with `mods.alt == true` (Alt+letter) | `\x1b` + UTF-8 of `c` (two bytes in one write) | Alt+letter chord in PSReadLine (e.g. Alt+b = back-word) | requires Windows runtime confirmation | TS-9 on real host; if PSReadLine splits into Esc-then-letter, add `encode_alt_letter_win32` |
| `Up`/`Down`/`Left`/`Right` | `\x1b[A` / `\x1b[B` / `\x1b[C` / `\x1b[D` (CSI) | cursor move | safe (mechanical) — complete CSI in one write | none |
| `Home`/`End` | `\x1b[H` / `\x1b[F` (CSI) | line start / end | safe (mechanical) | none |
| `PageUp`/`PageDown` | `\x1b[5~` / `\x1b[6~` (CSI) | scroll page | safe (mechanical) | none |
| `Delete`/`Insert` | `\x1b[3~` / `\x1b[2~` (CSI) | edit ops | safe (mechanical) | none |
| `F(1)`..`F(4)` | `\x1bOP`..`\x1bOS` (SS3) | function key in app | safe (mechanical) — complete SS3 in one write | none |
| `F(5)`..`F(12)` | `\x1b[15~`..`\x1b[24~` (CSI) | function key in app | safe (mechanical) | none |
| `Tab` with `mods.shift` | `\x1b[Z` (CSI back-tab) | reverse completion / mode switch | safe (mechanical) | none |
| `Char(c)` with `mods.ctrl == true` + Alt prefix (i.e. Alt+Ctrl+letter) | `\x1b` + control byte | Alt-prefixed Ctrl chord | requires Windows runtime confirmation | TS-9 also covers this; rare in practice |

### Reproduction baseline

- Reproduction host: TBD (recorded at TS-9 / TS-10 execution time)
- Windows build: TBD
- Shell: pwsh (PowerShell 5/7) primary, optional cross-check on cmd
- PSReadLine version: TBD

### Legend

- **safe (mechanical)** — verdict reachable from source inspection alone. The branch writes a complete CSI/SS3 sequence in one `write_input` call; ConPTY's WIN32_INPUT_MODE parser is documented to forward such complete sequences intact. No Windows runtime evidence required.
- **fixed (Phase 1)** / **fixed (Phase 1 follow-up)** — branch has an explicit `encode_<key>_win32` path landed in this feature.
- **requires Windows runtime confirmation** — verdict depends on PSReadLine's behavior or ConPTY timing that source inspection cannot determine; tracked by TS-9 / TS-10 manual checks. If confirmed broken, add an encoder helper following the Backspace / Escape / Ctrl+[ pattern.

### Not investigated (out of FR3 scope)

- IME-composed key sequences (handled in a separate layer, never reach `encode()`).
- Bracketed-paste content (already opaque under both modes).
- Application-mode toggles via `DECCKM` (none currently emitted by `encode()`).
