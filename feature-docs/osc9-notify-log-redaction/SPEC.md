# Feature: osc9-notify-log-redaction

## Overview

Two log sites on the OSC 9 notification path interpolate parsed notification text
directly into log records: the rate-limit branch of `NativeCallbacks::handle_notify`
(`src-tauri/src/callbacks.rs:496`) and the dispatch-success branch of
`NotifyRustSink::send` (`src-tauri/src/callbacks.rs:175`). This feature redacts both
sites so the OSC 9 title and body never reach
`~/.local/share/net.laser5.app.emterm/logs/emterm.log`, replacing the interpolated text
with a bounded set of non-sensitive metadata (lengths plus a per-run keyed diagnostic
ID). Notification behaviour itself is untouched — only log record content changes.

Requirements source: `feature-docs/osc9-notify-log-redaction/REQUIREMENTS.md`.

## Objectives

- Stop persisting attacker-influenceable OSC 9 notification text (title and body) into
  `~/.local/share/net.laser5.app.emterm/logs/emterm.log`, closing an
  information-disclosure path that outlives the intended desktop notification.
- Keep OSC 9 rate-limit suppression and notification dispatch diagnosable from
  emterm.log using non-sensitive metadata only, so "which notification was suppressed /
  dispatched" stays answerable without the raw text.

## Scope

**In scope**

- The rate-limit branch's log record in `NativeCallbacks::handle_notify` (FR1).
- The dispatch-success branch's log record in `NotifyRustSink::send` (FR5).
- One shared redaction/formatting helper used by both sites (FR6).
- A keyed diagnostic ID derived with a per-process-run key (FR3).
- Unit tests pinning the above.

**Out of scope**

- Notification behaviour: `parse_osc9`, `NotificationRateLimiter` semantics,
  `pending_notifications`, the D-Bus/toast payload (NFR1).
- The escape pipeline from the notification-markup-fail-closed SPEC
  (`escape_for_send` / `body_markup_absence_confirmed` / `escape_body_markup`) (NFR2).
- The error branch of `NotifyRustSink::send` (`src-tauri/src/callbacks.rs:176`), which
  keeps its current form (FR7).
- E2E coverage (A5).

## User Stories

### US1: No notification text in the log file
As an eMterm user whose machine holds emterm.log, I want OSC 9 notification text to
never be written to the log, so that attacker-influenceable content does not outlive the
desktop notification it was meant for.

**Acceptance Criteria:**
- [ ] No log record produced by the OSC 9 notification path (rate-limit branch or
      dispatch-success branch) contains any substring of the notification title or body.
- [ ] The metadata a redacted record may carry is limited to the marker, the title
      length, the body length and the diagnostic ID.

### US2: Suppression stays diagnosable
As someone investigating eMterm from emterm.log, I want the rate-limit suppression to
still be identifiable and correlatable, so that "which notification was suppressed" is
answerable without the raw text.

**Acceptance Criteria:**
- [ ] The rate-limit record still identifies the event via the `LOG_NOTIFY_RATE_LIMIT`
      marker at warn level, and carries title length, body length and a diagnostic ID.
- [ ] Two suppressions of the same (title, body) pair within one process run carry the
      same diagnostic ID; two different pairs carry different IDs.

## Technical Requirements

### Functional Requirements

- **FR1 - Redact the rate-limit warn line:** The rate-limit branch of
  `NativeCallbacks::handle_notify` (`src-tauri/src/callbacks.rs:496`, currently
  `log::warn!("{LOG_NOTIFY_RATE_LIMIT}: '{title}' / '{body}'")`) emits no substring of
  the parsed OSC 9 title or body. It emits the stable `LOG_NOTIFY_RATE_LIMIT` marker
  plus non-sensitive metadata only.
- **FR2 - Non-sensitive metadata set:** The metadata a redacted line may carry is limited
  to: the `LOG_NOTIFY_RATE_LIMIT` marker (unchanged constant value), the title length,
  the body length, and a keyed diagnostic ID (FR3). Raw text, prefixes, suffixes,
  truncations, character samples and any other content-derived rendering of the title or
  body are excluded.
- **FR3 - Keyed diagnostic ID:** A short diagnostic ID is derived from the (title, body)
  pair with a keyed hash whose key is generated per process run. The same pair produces
  the same ID within one run (so repeated suppressions of one notification correlate in
  the log), and the ID is not usable to recover or confirm the original text, nor
  comparable across process runs.
- **FR4 - Suppression remains traceable:** The rate-limit event is still logged at warn
  level, so it survives the release build's warn-and-above file-recording filter
  (`src-tauri/src/logging.rs:191`) and the suppression remains observable in emterm.log
  after the change. The change does not silence the event.
- **FR5 - Redact the success-path debug line:** The dispatch-success branch of
  `NotifyRustSink::send` (`src-tauri/src/callbacks.rs:175`, currently
  `log::debug!("notify-rust dispatched: {title}")`) is redacted under the same policy as
  FR1: the interpolated title is replaced by non-sensitive metadata (length plus a keyed
  diagnostic ID per FR2/FR3). The line's level stays debug.
- **FR6 - Shared redaction helper:** FR1 and FR5 use one shared redaction/formatting
  helper inside `src-tauri/src/callbacks.rs` rather than two independently formatted
  strings, so the two sites cannot drift in what they consider non-sensitive.
- **FR7 - No other notification log site leaks text:** The error branch of
  `NotifyRustSink::send` (`src-tauri/src/callbacks.rs:176`,
  `log::warn!("notify-rust failed: {e}")`) logs only the notify-rust error value and
  keeps its current form; the change introduces no new site that interpolates OSC 9 title
  or body into a log record.

### Non-Functional Requirements

- **NFR1 - Behavioural invariance:** Notification behaviour is unchanged: `parse_osc9`
  output, `NotificationRateLimiter` dedupe semantics (1 s window, (title, body) key),
  `pending_notifications` buffering and the D-Bus/toast payload delivered by
  `NotifyRustSink::send` all keep their current values. Only log record content changes.
- **NFR2 - Escape pipeline untouched:** The escape pipeline introduced by the
  notification-markup-fail-closed SPEC (`escape_for_send` /
  `body_markup_absence_confirmed` / `escape_body_markup`) is not modified.
- **NFR3 - Log convention and marker stability:** Log lines keep the project's
  `[LEVEL] <message>` convention, and the `LOG_NOTIFY_RATE_LIMIT` constant keeps its
  current value (`"LOG_NOTIFY_RATE_LIMIT"`) so existing log greps on the marker keep
  matching.
- **NFR4 - No new dependency:** No new third-party dependency is added for the keyed ID;
  the hash comes from the crates already in the dependency graph (std is sufficient).
- **NFR5 - Negligible per-notification cost:** Per-notification cost stays negligible —
  the redaction runs once per OSC 9 notification event, on a path that already allocates
  two `String`s.
- **NFR6 - Feature-gate containment:** The change stays inside the GUI-gated callbacks
  module and does not affect the `--no-default-features` (CLI-only) build surface.

## Implementation Approach

### Architecture

The change is confined to the logging statements of two branches in `callbacks.rs`; no
new component and no new dependency is introduced.

```
OSC 9 sequence
    │
    ▼
parse_osc9                       (unchanged — NFR1)
    │  (title, body)
    ▼
NativeCallbacks::handle_notify
    │
    ├── NotificationRateLimiter  (unchanged — NFR1)
    │        │
    │        └── suppressed → redaction helper → warn record   ← changed (FR1)
    │
    └── dispatch
             │
             ▼
        NotifyRustSink::send
             ├── escape_for_send (unchanged — NFR2)
             ├── success → redaction helper → debug record     ← changed (FR5)
             └── error   → warn "notify-rust failed: {e}"      ← unchanged (FR7)
```

**Component Diagram:**

```
callbacks.rs
  <redaction helper>            - single shared formatter for both sites (FR6)
                                  input : (title, body)
                                  output: non-sensitive metadata rendering (FR2)
  <keyed diagnostic ID>         - keyed hash over (title, body), per-run key (FR3)
  handle_notify (rate-limit)    - LOG_NOTIFY_RATE_LIMIT + metadata, warn  (FR1, FR4)
  NotifyRustSink::send (ok)     - metadata only, debug                    (FR5)
  NotifyRustSink::send (err)    - unchanged                               (FR7)
```

### Data Flow

```
(title, body)
  → keyed hash with per-run key → diagnostic ID          [FR3]
  → lengths of title and body                            [FR2]
  → shared redaction helper renders the metadata         [FR6]
      rate-limit site  → log::warn!  with LOG_NOTIFY_RATE_LIMIT + metadata   [FR1, FR4]
      success site     → log::debug! with metadata                           [FR5]
```

### Redaction Decision Table

| Candidate content | Allowed in a redacted record | Requirement |
|---|---|---|
| `LOG_NOTIFY_RATE_LIMIT` marker (value unchanged) | Yes (rate-limit site) | FR1, FR2, NFR3 |
| Title length | Yes | FR2 |
| Body length | Yes | FR2 |
| Keyed diagnostic ID | Yes | FR2, FR3 |
| Raw title / body text | No | FR1, FR2 |
| Prefix / suffix / truncation of either | No | FR2 |
| Character samples or any other content-derived rendering | No | FR2 |

### Log Site Table

| Site | Location | Level | Content after the change | Requirement |
|---|---|---|---|---|
| Rate-limit suppression | `handle_notify`, `callbacks.rs:496` | warn (unchanged) | marker + lengths + diagnostic ID | FR1, FR2, FR4 |
| Dispatch success | `NotifyRustSink::send`, `callbacks.rs:175` | debug (unchanged) | lengths + diagnostic ID | FR5, FR2 |
| Dispatch error | `NotifyRustSink::send`, `callbacks.rs:176` | warn (unchanged) | notify-rust error value only (unchanged) | FR7 |

### Rationale: why the success-path debug line is redacted too

The task's acceptance criteria allow either applying the same policy to the
dispatch-success `log::debug!` line or recording a rationale for keeping it. This SPEC
takes the first option (`redact-both`, decided in batch mode by Codex consultation — see
A1). The rationale, as reflected in the requirements:

- The debug line interpolates the same attacker-influenceable title, so leaving it
  unredacted would keep an information-disclosure path open in any build or environment
  where debug records are persisted.
- FR6 requires one shared helper; redacting only one of the two sites would reintroduce
  the drift the shared helper exists to prevent.
- The level stays debug (FR5), so nothing about the release build's warn-and-above file
  filter changes as a result of this decision.

FR5 is the single point to revisit if this decision is reversed.

### API Design

Not applicable — no HTTP/RPC surface is added or changed.

### Database Schema

Not applicable — no persisted data model; the only persistence involved is the log file
itself, whose records this feature narrows.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/callbacks.rs`: hosts both log sites (`handle_notify`'s rate-limit branch
  at line 496, `NotifyRustSink::send` at lines 148-175) and will host the shared
  redaction helper (FR6).
- `src-tauri/src/logging.rs:191`: the release build's warn-and-above file-recording
  filter that FR4 depends on.
- `escape_for_send` / `body_markup_absence_confirmed` / `escape_body_markup`: consumed
  unchanged (NFR2).

**External Dependencies:**
- None added. The keyed hash comes from crates already in the dependency graph — std is
  sufficient (NFR4).

### File Structure

```
src-tauri/src/
├── callbacks.rs          # both log sites + the shared redaction helper + keyed ID
└── callbacks/
    └── tests.rs          # unit tests for the helper and the diagnostic ID
```

## Test Scenarios

### Unit Tests
- [ ] TS1 (AC1): The redaction helper, given a title/body containing a URL, a token-like
      string and a command line, returns a string that contains none of those substrings.
- [ ] TS2 (AC2): The redaction helper's output contains the title length and the body
      length for a known input pair.
- [ ] TS3 (AC3): The diagnostic ID is equal for two calls with the same (title, body)
      pair and differs for a pair that differs only in body.
- [ ] TS4 (AC5, NFR1): Existing rate-limiter behaviour tests
      (`rate_limiter_dedupes_identical_pair_within_window`,
      `rate_limiter_allows_after_window_elapsed`,
      `rate_limiter_distinct_pairs_not_deduped` in
      `src-tauri/src/callbacks/tests.rs:541-569`) still pass unmodified, showing sink
      delivery is untouched.
- [ ] TS5 (AC5, NFR1): `parse_osc9` micro-tests
      (`src-tauri/src/callbacks/tests.rs:650-668`) still pass, showing title/body
      derivation is untouched.

### Integration Tests
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      passes.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

No E2E coverage is added — the resolved E2E input set is empty and the project has no
resolved E2E harness for this path (A5).

### Manual Tests
- [ ] TS6 (AC2, AC4): Trigger a duplicate OSC 9 within the 1 s window on a release build
      and confirm the emterm.log line names the suppression without the text.

### Edge Cases
- [ ] The same (title, body) pair suppressed repeatedly within one run — the records
      share one diagnostic ID so they correlate (FR3, TS3).
- [ ] Two different pairs suppressed within one run — the records carry different
      diagnostic IDs (FR3, TS3).
- [ ] The same pair seen in two different process runs — the IDs are not comparable, and
      that is accepted rather than treated as a defect (A2).
- [ ] The two log sites see different string values for one notification (raw vs.
      body-markup-escaped), so their IDs are not guaranteed to match; accepted (A3).

### Build / Gate Checks
- [ ] AC6 (NFR6):
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      passes.

### Performance Tests
Not applicable — NFR5 only requires that the once-per-notification redaction stay
negligible on a path that already allocates two `String`s; no measured performance goal
is defined.

## Security Considerations

- **Data Protection:** OSC 9 title and body are attacker-influenceable and are no longer
  persisted into emterm.log; the information-disclosure path that outlived the desktop
  notification is closed (OBJ1, FR1, FR5).
- **Output Minimisation:** What a redacted record may carry is an explicit allow-list —
  marker, title length, body length, diagnostic ID — with every content-derived rendering
  (prefixes, suffixes, truncations, character samples) excluded (FR2).
- **One-way diagnostic ID:** The ID is a keyed hash with a per-process-run key; it cannot
  be used to recover or confirm the original text and is not comparable across runs
  (FR3, A2).
- **No new leak sites:** The dispatch-error branch keeps logging only the notify-rust
  error value, and no new site interpolates notification text (FR7).
- **Input Validation / XSS / Markup injection:** Unchanged — the escape pipeline from
  notification-markup-fail-closed is not modified (NFR2).
- **Authentication / Authorization / CSRF / SQL Injection:** Not applicable — the feature
  adds no authenticated surface, no data store, and no web request handling.

## Error Handling

| Condition | Handling | Requirement |
|---|---|---|
| Notification suppressed by the rate limiter | Log the marker plus metadata at warn level; behaviour of the limiter is unchanged | FR1, FR2, FR4, NFR1 |
| `notify-rust` send fails | Log the notify-rust error value only, in its current form | FR7 |

### Error Flow

```
Rate-limit suppression → shared redaction helper → warn record (marker + metadata)
notify-rust failure    → warn record (error value only, unchanged)
```

## Performance Optimization

Not applicable — no performance goal, optimization strategy, or caching is specified for
this feature beyond NFR5's negligible-cost expectation.

## Success Criteria

- [ ] AC1: No log record produced by the OSC 9 notification path (rate-limit branch or
      dispatch-success branch) contains any substring of the notification title or body.
- [ ] AC2: The rate-limit record still identifies the event via the
      `LOG_NOTIFY_RATE_LIMIT` marker at warn level, and carries title length, body length
      and a diagnostic ID.
- [ ] AC3: Two suppressions of the same (title, body) pair within one process run carry
      the same diagnostic ID; two different pairs carry different IDs.
- [ ] AC4: The success-path debug record carries metadata only, at debug level, and the
      rationale for the redact-both decision is recorded in this SPEC (see "Rationale:
      why the success-path debug line is redacted too").
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      passes.
- [ ] AC6: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      passes.

## Assumptions

- **A1**: Recorded per `batch-policies.yaml` `record_as_assumption`: the success-path
  debug line is redacted (option `redact-both`), resolved in batch mode by Codex
  consultation rather than by the user directly. If the user disagrees, FR5 is the single
  point to revisit. (Source: `answers[notify-log.success-path-debug-line]`.)
- **A2**: "Keyed" means a per-process-run random key (e.g. std's `RandomState` seeding
  SipHash), not a persisted secret; cross-run correlation of IDs is explicitly not a
  requirement and its absence is accepted.
- **A3**: The two log sites see different string values: `handle_notify`
  (`callbacks.rs:487`) holds the raw parsed title/body, while `NotifyRustSink::send`
  (`callbacks.rs:148-175`) may hold the body-markup-escaped forms produced by
  `escape_for_send`. IDs computed at the two sites are therefore not guaranteed to match
  for the same notification. This is accepted; making them match is a plan-phase
  implementation choice (compute the ID before escaping), not a stated requirement.
- **A4**: Lengths are reported in a single, consistently documented unit (bytes or
  chars); either satisfies FR2, and picking one is a plan-phase detail.
- **A5**: No E2E coverage is added — the resolved E2E input set is empty and the project
  has no resolved E2E harness for this path; verification is unit tests plus a manual
  check.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement (FR1-FR7, NFR1-NFR6) is `resolved`.

## Design Step

Skipped. No user-visible surface changes: the feature alters two log record strings in
`src-tauri/src/callbacks.rs`; there is no UI, no WebView, no user-facing text and no
design-token involvement, so no design step is warranted.

## References

- Requirements document: `feature-docs/osc9-notify-log-redaction/REQUIREMENTS.md`
- Implementation site: `src-tauri/src/callbacks.rs` (rate-limit branch:496, send:148-175,
  success:175, error:176, title/body:487)
- Release log-level filter: `src-tauri/src/logging.rs:191`
- Existing tests: `src-tauri/src/callbacks/tests.rs` (rate limiter:541-569,
  parse_osc9:650-668)
- Escape pipeline held constant: `feature-docs/notification-markup-fail-closed/SPEC.md`
- Log file: `~/.local/share/net.laser5.app.emterm/logs/emterm.log`
