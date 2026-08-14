# Implementation Plan: OSC 9 Notification Log Redaction

## Overview

Both notification log sites in the GUI-gated callbacks module stop interpolating OSC 9
notification text and instead emit a fixed, allow-listed metadata rendering produced by one
shared, module-private redaction component. Notification behaviour, the rate limiter and the
body-markup escape pipeline are untouched — only log record content changes.

## Technology Stack

- **Language / crate**: Rust, the `emterm` crate (`src-tauri/`), inside the existing
  GUI-feature-gated `callbacks` module.
- **Key libraries**: none added. The keyed hash uses the language standard library's
  randomly-seeded hash-state facility, already present in the dependency graph (NFR4).
- **New dependencies and their licenses**: none. No third-party crate is added, so no license
  compatibility question arises; `project.license` stays `MIT` and the standard library
  (dual MIT / Apache-2.0) is already a dependency of the crate. Nothing in this feature
  changes the project's license position.

## Layer Structure

No new layer and no new module. Two layers inside the existing callbacks module:

| Layer | Responsibility | May depend on |
|---|---|---|
| Redaction layer (new, module-private) | Pure rendering of allow-listed metadata for a (title, body) pair, and derivation of the diagnostic ID | Nothing inside the module |
| Notification path (existing) | OSC 9 handling, rate-limit decision, sink dispatch, and the three log records | The redaction layer |

Allowed dependency direction is one-way: notification path → redaction layer. The redaction
layer never reads notification state, performs no I/O, never calls the escape pipeline and is
never called from the escape pipeline.

## Shared Components

This feature decomposes into a single task, so nothing here is literally cross-task. The two
contracts below are pinned at feature level anyway because both log sites bind to them, the
review phase checks the implementation against them, and a future rework task touching either
site must implement against the same contract rather than re-deriving it.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Redaction renderer | Render the allow-listed metadata for one (title, body) pair | **Pre**: both inputs are the values as received at the call site, before any escaping transformation is applied to them. **Post**: returns an owned rendering that contains exactly, in the fixed order and field naming of "Redacted record format" below, the title length, the body length and the diagnostic ID; contains no character sequence copied from either input; performs no I/O; is total (every input pair, including empty strings, yields a rendering — no failure branch); leaves both inputs unmodified. | task0001 (both log sites) |
| Diagnostic ID | Derive one short correlation value from a (title, body) pair | **Pre**: same as above. **Post**: 16 lowercase hexadecimal characters, fixed width, never truncated or padded; equal for equal input pairs within one process run **regardless of the calling thread**; different pairs differ with negligible collision probability; derived under a hash key created exactly once per process run that is never written to any log record at any level, so the value is neither comparable across process runs nor invertible from log contents. | Redaction renderer only |

## Conventions

### Redacted record format

| Position | Field | Value | Unit |
|---|---|---|---|
| 1 | title length | length of the title as received at the call site | bytes (UTF-8 encoded length) |
| 2 | body length | length of the body as received at the call site | bytes (UTF-8 encoded length) |
| 3 | diagnostic ID | the value defined by the Diagnostic ID contract | 16 lowercase hex characters |

Rendering rules: the three fields appear in the order above, each as a `name=value` pair
separated by a single space, and each field name carries its unit so no log reader has to
infer it. The rendering is produced in exactly one place (the redaction renderer) so the two
sites cannot drift in what they consider non-sensitive (FR6).

### Log site prefixes

Each site prepends its own stable, content-independent literal and nothing else. No site may
bind the title or the body into its record in any form.

| Site | Level | Prefix | Requirement |
|---|---|---|---|
| Rate-limit suppression (OSC 9 handler) | warn (unchanged) | the existing rate-limit marker constant, value unchanged, followed by a colon and a space | FR1, FR4, NFR3 |
| Dispatch success (notification sink) | debug (unchanged) | the existing "notify-rust dispatched" literal, followed by a colon and a space | FR5 |
| Dispatch error (notification sink) | warn (unchanged) | unchanged in full — the record keeps its current form and content (the notify-rust error value only) | FR7 |

### Logging policy for this path

- The project's `[LEVEL] <message>` convention is produced by the logger itself; this feature
  changes nothing about it (NFR3).
- Nothing derived from notification content other than the three allow-listed fields may
  appear in any record on this path: no prefixes, suffixes, truncations, character samples,
  per-character counts, partial hashes of a single field, or any other content-derived
  rendering (FR2).
- The per-run hash key is never logged at any level, and is never exposed outside the module.

### Error handling

The redaction layer introduces no failure mode: the renderer is total and never fails or
panics, so no fallback branch, no result type and no degraded record exists to design. The
existing dispatch-error branch keeps its current handling unchanged (FR7).

## Cross-task Design Decisions

### D1 — Length unit is bytes (resolves SPEC assumption A4)

Lengths are reported as the UTF-8 byte length of the string as received, and the field name
states the unit explicitly so a reader can never mistake it for a character count.

Rationale: it is the length of the value the parse actually produced, it is unambiguous for
any input, and it needs no scanning decision (grapheme vs scalar value) that could itself
become a drift point between the two sites. Both units satisfy FR2; pinning one here prevents
the sites from diverging. Affects: task0001, TS2.

### D2 — The rendering is captured before the escape gate (resolves SPEC assumption A3)

At the dispatch-success site, the rendering is derived from the values the sink received on
entry, captured **before** the escape gate shadows them with their escaped forms. The
rate-limit site already holds the raw parsed values.

Rationale: the two sites would otherwise compute IDs over different strings for one and the
same notification (raw vs. body-markup-escaped), so a notification that is dispatched once and
then suppressed on a repeat would appear in the log under two unrelated IDs — destroying the
correlation the ID exists to provide (FR3). Capturing before the gate makes the debug record
of the first dispatch and the warn record of the later suppression share one ID. This changes
nothing about the escape pipeline itself (NFR2): it only fixes which values the renderer is
handed and when. Affects: task0001, TS10.

Consequence worth knowing: the sink is the single egress point for every notification producer
in the app, not only OSC 9, so the redacted success record now covers all of them. That is a
strict improvement in the same direction as FR5/FR7 and changes no behaviour.

### D3 — The hash key is process-global and initialized exactly once (resolves SPEC assumption A2)

"Keyed" means one randomly-seeded hash state created lazily on first use and then shared,
immutably, by every caller in the process for the rest of the run. It is not a persisted
secret, not re-created per call, and explicitly not per-thread.

Rationale: per-call keying would make every record's ID unique and defeat FR3's within-run
correlation; per-thread keying would silently break correlation between records emitted from
different tabs' threads, which is the exact situation a multi-tab session produces. The
callbacks state and the sink are both shared across threads, so the shared initialization must
be thread-safe and must be observable by all threads. Cross-run incomparability is a
consequence of the per-run seed and is accepted (SPEC A2). Affects: task0001, TS3.

### D4 — Placement, visibility and feature gating

The renderer and the ID derivation are module-private items of the callbacks module, not
exported from the crate, and unit-tested from that module's own test child module (FR6).

Two gating rules apply. First, the callbacks module is already GUI-feature-gated in the crate's
module roster, so the CLI-only build surface is unaffected without any additional per-item
gating (NFR6). Second, neither new item may carry the Unix-only gating that the escape helpers
in the same module carry: the dispatch-success site exists on every supported platform, so a
Unix-only renderer would break the Windows build. Affects: task0001, TS8.

### D5 — The metadata set is closed, and both lengths appear at both sites

The renderer emits all three allow-listed fields at both sites; no site emits a subset and no
site adds a field. FR5 names "length plus a keyed diagnostic ID" for the success site; FR2's
allow-list permits the body length at either site, and FR6's single-helper requirement makes
one identical field set the only shape that cannot drift. Adding any fourth field to this
record — however innocuous it looks — is a requirement change, not an implementation choice.
Affects: task0001, TS1, TS2.

### D6 — No automated test asserts on the final log record text

No test harness in this project captures logger output, so no automated test can assert what
either record finally contains. Requirement-level coverage therefore runs through the
renderer's own unit tests, which is precisely why FR6's shared helper is a hard requirement
rather than a style preference: each site contributes only a fixed literal prefix on top of
the renderer's output, so testing the renderer tests almost the whole record.

The residual gap — a site could still bind notification text *alongside* the renderer's output
— is covered by the diff-inspection item in VERIFICATION.md and by the review phase, not by a
test. Do not attempt to close it by introducing a logging harness; that is out of scope for
this feature. Affects: task0001, VERIFICATION.md TS11.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A naive "output contains no substring of the input" assertion is flaky, because a length digit or a hex character of the ID can coincidentally equal a single character of the title or body | High | Low (test noise, not a defect) | Assertions target whole sensitive content tokens (the URL, the token-like string, the command line) rather than per-character absence; stated in task0001's Test Notes and in TS1 |
| The per-run key ends up per-thread or per-call, silently breaking within-run correlation | Medium | Medium (FR3 unmet in exactly the multi-tab case that matters) | D3 pins single process-global initialization; note that TS3 exercises one thread only, so the initialization site is an explicit review check, not a test-covered property |
| The success-path rendering is computed after the escape gate, so one notification yields two unrelated IDs | Medium (the shadowing is easy to miss) | Medium (correlation lost) | D2 pins the capture point; TS10 pins the premise by showing raw and escaped inputs render differently |
| An attacker who can both inject OSC 9 sequences into the running process and read the log can confirm a guessed (title, body) pair by matching IDs | Low | Low | Accepted and recorded: such an attacker already observes the notification itself, so this grants no new information. The key is per-run and never logged, so an attacker with the log alone gains nothing |
| A behavioural regression slips into the notification path while editing the same function | Low | High (NFR1 breach) | The existing rate-limiter, parse and escape tests are re-run unmodified (TS4, TS5, TS12) and the full library test command gates the task (TS9) |
| Existing operational log greps break | Low | Medium | The marker constant's value is unchanged and pinned by TS7; the success site keeps its existing literal prefix |

## Open Questions

- [ ] FR5 (redact the success-path debug line) is recorded in workflow.yaml with
      `status: assumed` — the redact-both choice came from batch-mode consultation (SPEC A1),
      not from the user. If the user disagrees, FR5 and the dispatch-success row of the Log
      site prefixes table are the only points to revisit; the renderer contract stands either
      way.
- [ ] D1 picks bytes for the length unit where SPEC A4 left either unit acceptable. Reversing
      it to characters is a one-line change in the renderer plus TS2's expectations.
