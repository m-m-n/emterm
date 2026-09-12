# Implementation Plan: selection-clear-on-enter-copy

## Overview

Two key-input-driven triggers — Enter forwarded to the PTY, and a copy through
`keybinds.copy` — are added to the six selection-clear conditions that already
exist. The clear *decision* is factored out as a side-effect-free predicate and
the clear *mutation* as a single application-state helper, so both new call
sites share one contract and the existing six sites stay untouched.

## Technology Stack

- **Language**: Rust — the existing `emterm` crate, inside the `gui` feature
  only (`--no-default-features` builds are unaffected).
- **Framework**: winit key events and the existing winit-driven event loop;
  already in use, no change to how key events are obtained.
- **Key libraries**: none added.

### Dependency licenses

This feature introduces **no new dependency**, so no license compatibility
decision is required. `project.license` stays `MIT`; no `LICENSE` change is
implied by this plan.

## Layer Structure

| Layer | Files | Responsibility | May depend on |
|-------|-------|----------------|---------------|
| Event loop | `src-tauri/src/window_host/event_loop.rs` | Receives key events, decides forwarding to the PTY, drives per-frame work | key routing, input translation, application state |
| Key routing | `src-tauri/src/window_host/key_routing.rs` | Resolves configured key bindings (including `keybinds.copy`) before a key reaches the PTY path | input translation, application state |
| Input translation | `src-tauri/src/window_host/input_translate.rs` | Side-effect-free key/byte decisions; owns the existing `shift_enter_rewrite` and the new clear-decision predicate | nothing (pure) |
| Application state | `src-tauri/src/app/` | Owns `selection` and `pending_selection_anchor`, and the existing dirty-row computation that unions the current and previous selection | nothing in `window_host` |

Dependency direction is one-way: `window_host` calls into the application
state, never the reverse. The input-translation layer stays pure — it reads no
application state and performs no mutation.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Clear-decision predicate — `should_clear_selection_on_forward`, in `window_host/input_translate.rs` | Decide whether a key event that has just been handled should clear the selection | **Inputs**: two booleans — "the event was forwarded to the PTY" and "the logical key is the named Enter key". **Output**: one boolean. **Precondition**: none. **Postcondition**: the result is true exactly when both inputs are true, and false in every other combination; the call reads and mutates no state, performs no I/O, and is safe to call any number of times. | task0001 |
| Selection-clear helper — `clear_selection`, a method on the application state in `src-tauri/src/app/mod.rs` | Drop the selection and its pending anchor together | **Precondition**: none — callable whether or not a selection is present. **Postcondition**: both `selection` and `pending_selection_anchor` are unset; no other application field is modified; nothing is written to the clipboard or to PRIMARY; calling it when both are already unset changes nothing (idempotent). | task0001 |

Both components are consumed only from within the same task, but their
contracts are pinned here because the review and verify phases check the call
sites against them, and because a later feature that adds a third trigger must
route through the same pair rather than re-implementing the mutation inline.

## Conventions

- **Naming**: the new predicate follows the existing pure-helper naming in
  `input_translate.rs` (`shift_enter_rewrite`) — a verb phrase describing the
  decision, not the caller.
- **Single clearing route**: every *new* clear site calls the application-state
  helper. No new site sets `selection` or `pending_selection_anchor`
  individually — the pairing is a property of the helper, not of each caller.
- **Existing sites are not rewritten**: the six existing clear conditions keep
  their current inline form (ASM4). Converting them to the helper is out of
  scope for this feature.
- **Error handling**: neither component has a failure path — the predicate
  returns a boolean and the helper unsets two fields. Nothing is returned that
  can fail, nothing is logged, and no new error type is introduced.
- **Logging**: none added. The change sits on the hot key-input path and has no
  failure mode worth diagnosing.
- **Test placement**: unit tests live under the library target (`--lib`); the
  binary target holds none, so tests placed there would never run.

## Cross-task Design Decisions

### D1: The feature is implemented as one task

The two call sites cannot compile without the application-state helper, and the
source-text scan assertions cannot pass without the call sites. Because tasks
run fully in parallel with no ordering mechanism, splitting the helper away from
its call sites would put a compile-time dependency across two isolated
worktrees for a change that is two branch conditions plus one helper. The whole
feature is therefore one coherent implementer session (task0001), and this
document stays correspondingly thin.

### D2: The Enter trigger is Enter-only (ASM1)

The forwarded-key branch is reached by every key that actually produced bytes
for the PTY. Clearing on all of them would break the "select, read, and keep
typing" use case, so the predicate additionally requires the logical key to be
the named Enter key. The rejected alternative — clearing on every forwarded key
with an exclusion list — was dropped because the exclusion list has no
principled boundary. Reversing this decision later is a one-condition change,
which is why the decision is isolated in the predicate rather than spread across
the call site.

### D3: The copy trigger sits inside the selection-present branch

`keybinds.copy` consumes its chord even when nothing is selected, and that
behaviour is preserved (FR4). The clear is therefore placed inside the branch
that runs only when a selection exists, immediately after the clipboard write —
so a chord press with no selection keeps having no side effect at all, and a
copy whose resolved text happens to be empty still clears (EC6).

### D4: Redraw is inherited, not added

The application already unions the current and the previous selection into the
per-frame dirty rows, so the frame after a clear repaints exactly the rows the
old highlight occupied. This feature adds no redraw request and no full-redraw
flag of its own; FR6 is satisfied by the existing machinery and NFR5 is
satisfied by not bypassing it. The known exception — a frame with the fold
layout active reports every row dirty — is pre-existing behaviour (EC4) and is
not addressed here.

### D5: Call-site placement is pinned by source-text scans

Neither call site is reachable from a unit test: the Enter site lives inside the
winit event loop, and the copy site's handler takes a concrete window host and
requires a real window. Their placement is therefore pinned by source-text scan
assertions in the same shape as the existing pointer-routing scans
(`src-tauri/src/window_host/tests.rs:1488 / 1536 / 1581 / 1629`). These
assertions substitute for integration tests; end-to-end behaviour is confirmed
manually (TS9). Scan assertions must target the smallest stable landmark that
still proves the placement, so that unrelated edits to neighbouring lines do not
break them.

### D6: No design-system work

The feature changes no UI surface, layout, or design token — the visible effect
is an existing highlight disappearing one frame earlier than before. The design
step was skipped for this reason, and no token-to-platform translation work is
planned. `doc/UI-DESIGN-GUIDELINES.yaml` and its two mirrors are not touched.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Source-text scan assertions break on unrelated nearby edits | Medium | Low | Assert on the smallest stable landmark that still proves placement (D5); keep each scan to one claim |
| The clear is placed outside the forwarded branch and fires for keys that never reached the PTY | Low | Medium | The predicate requires the forwarded flag as well as Enter; the truth table test covers the forwarded-false rows |
| Pressing Enter while the left button is held drops the pending anchor, so the fold-click decision on release does not hold and the fold toggle misfires once (EC1) | Medium | Low | Accepted as ASM5 — explicitly out of scope, recorded so review does not re-litigate it |
| A change leaks outside the `gui` feature and breaks the CLI-only build | Low | Medium | The no-default-features compile check is an acceptance criterion of task0001 |
| One of the six existing clear conditions is incidentally modified | Low | Medium | ASM4 forbids rewriting them; the declared file set excludes their files except `app/mod.rs`, and the regression scenario re-asserts them |

## Open Questions

- [ ] None. Every requirement (FR1–FR6, NFR1–NFR6) is resolved in
      `workflow.yaml`, and every one maps to at least one task and one test.
