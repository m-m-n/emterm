# Feature: notification-markup-fail-closed

## Overview

The Linux notification path in `NotifyRustSink::send` currently decides whether to
escape notification text from the result of `get_capabilities()`, and skips escaping
when that call fails (fail-open). This feature inverts the decision to fail-closed:
text is passed through unescaped only when the capability query succeeds and
explicitly reports that the server does not support `body-markup`. Everything else,
including a failed capability query, is escaped.

Requirements source: `feature-docs/notification-markup-fail-closed/REQUIREMENTS.md`.

## Objectives

- Close the notification-phishing window in which `GetCapabilities` fails while the
  immediately following `Notify` succeeds, letting OSC 9-derived `<a href>` /
  `<img src>` reach a body-markup-capable server (GNOME Shell / dunst).
- Close PR #35 review round1 finding `eade9e7f97a29a29` (severity medium / category
  security / confidence 100, cross-model agreement between Claude and Codex).
- Move the default of the escape decision to the safe side, based on the asymmetry of
  failure cost: over-escaping degrades display only, showing a literal `&lt;`.

## Scope

**In scope**

- The capability decision branch of `NotifyRustSink::send` under `#[cfg(unix)]`.
- Doc comments in that code that currently describe fail-open semantics.
- Unit tests pinning the new decision.
- Recording fail-closed as normative in this SPEC.

**Out of scope**

- A `summary(title)`-specific escaping measure (separate task).
- Making the notification path asynchronous.

## User Stories

### US1: Escaped output when the capability query fails
As a user on Linux whose notification server supports body-markup, I want notification
text to be escaped when eMterm cannot determine server capabilities, so that OSC
9-derived `<a href>` / `<img src>` cannot be rendered as markup in a desktop
notification.

**Acceptance Criteria:**
- [ ] A unit test pins that `escape_for_send` applies escaping to both title and body
      when `get_capabilities()` returns `Err(_)`.
- [ ] The escape decision covers `title(summary)` and `body` from a single evaluation.

### US2: Unchanged behaviour when capabilities are known
As a user on Linux whose notification server does not support body-markup, I want
notification text to keep passing through unescaped, so that `&` is not displayed raw
on a plain-text server.

**Acceptance Criteria:**
- [ ] `get_capabilities()` returning `Ok` with a list that omits `body-markup` results
      in unescaped pass-through (existing test expectations maintained/updated for the
      new specification).
- [ ] `get_capabilities()` returning `Ok` with a list containing `body-markup` results
      in escaping, with no regression of existing behaviour.

## Technical Requirements

### Functional Requirements

- **FR1 - Invert the capability decision to fail-closed:** Change the escape decision in
  `NotifyRustSink::send` to "pass through unescaped only when `get_capabilities()`
  succeeds and the returned list explicitly does not contain `body-markup`; escape in
  every other case (including `Err(_)`, i.e. query failure)". This replaces the current
  `body_markup_confirmed` (`src-tauri/src/callbacks.rs:220`, which treats `Err` as
  unconfirmed and skips escaping).
- **FR2 - Preserve existing behaviour on the `Ok` path:** When `get_capabilities()`
  succeeds, behaviour is unchanged: list contains `body-markup` → escape (as before);
  list does not contain it → pass through unescaped (as before, preserving the previous
  task's US2 guarantee that `&` is not displayed raw on a plain-text server).
- **FR3 - The fail-closed decision is a single per-send evaluation applied to both title
  and body:** Keep the single-evaluation structure of the current `escape_for_send`
  (`src-tauri/src/callbacks.rs:186`) — decision D2: one evaluation per `send` drives both
  title and body — so that on `Err(_)` both `title(summary)` and `body` are escaped.
- **FR4 - Leave the Windows notification path unchanged:** The capability decision is
  Linux-specific (`org.freedesktop.Notifications` over D-Bus), and the whole escape gate
  stays under `#[cfg(unix)]` as it is today. No capability decision and no escaping is
  added to the Windows notification path (the `.show()` call).
- **FR5 - Record fail-closed as the specification:** This SPEC states fail-closed as
  normative and supersedes FR3 of the previous task's SPEC
  (`feature-docs/notification-body-markup-escape/SPEC.md`, "pass through unescaped when
  it cannot be confirmed") at the specification level — taking the specification-change
  option rather than risk acceptance.

### Non-Functional Requirements

- **NFR1 - Feature/platform gate hygiene:** `notify-rust` is an optional dependency of
  the `gui` feature, so the change must keep following the existing
  `#[cfg(feature = "gui")]` / `#[cfg(unix)]` / `#[cfg(windows)]` gate conventions and
  keep the `--no-default-features` (CLI-only) build and the Windows build compilable.
- **NFR2 - In-code documentation consistency:** Update the doc comments that currently
  assume fail-open (`escape_for_send`'s "fail-open parity, FR1/FR3",
  `body_markup_confirmed`'s "fail-safe side (FR3): callers must not escape on
  unconfirmed", etc.) so they agree with the new fail-closed specification, renaming the
  function if needed to match the new semantics.
- **NFR3 - No change to the existing sanitize pipeline:** Leave the existing behaviour of
  `sanitize_title` (CSI stripping, control-character stripping, input cap, 100-character
  truncation), the escaping order (escape after truncation), and the notification rate
  limiter unchanged. The only change is the capability decision branch.

## Implementation Approach

### Architecture

The change is confined to the Linux branch of the notification sink; no new component is
introduced.

```
OSC 9 text
    │
    ▼
sanitize_title  (unchanged — NFR3)
    │
    ▼
NotifyRustSink::send
    │
    ├── #[cfg(unix)]  escape_for_send  ← the only changed logic (FR1/FR3)
    │        │
    │        └── get_capabilities()  → org.freedesktop.Notifications (D-Bus)
    │
    └── #[cfg(windows)]  .show()      ← unchanged (FR4)
```

**Component Diagram:**

```
callbacks.rs
  escape_for_send(title, body)   - single per-send decision (FR3), drives both fields
  body_markup_confirmed(...)     - capability predicate replaced by the fail-closed
                                   form (FR1); name/doc updated per NFR2
```

### Data Flow

```
send(title, body)
  → escape_for_send evaluates get_capabilities() once
      Ok(list) without "body-markup" → (title, body) unescaped        [FR2]
      Ok(list) with    "body-markup" → (escaped title, escaped body)  [FR2]
      Err(_)                         → (escaped title, escaped body)  [FR1/FR3]
  → notify with the resulting pair
```

### Decision Table

| `get_capabilities()` result | Escape title | Escape body | Requirement |
|---|---|---|---|
| `Ok(list)`, `body-markup` absent | No | No | FR2 |
| `Ok(list)`, `body-markup` present | Yes | Yes | FR2 |
| `Err(_)` | Yes | Yes | FR1, FR3 |

### API Design

Not applicable — no HTTP/RPC surface is added. The only external interface consulted is
`org.freedesktop.Notifications`'s `GetCapabilities` over D-Bus, via `notify-rust`.

### Database Schema

Not applicable — no persisted data.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/callbacks.rs`: hosts `escape_for_send` (line 186) and
  `body_markup_confirmed` (line 220), the sole implementation site.
- `sanitize_title` and the notification rate limiter: consumed unchanged (NFR3).

**External Dependencies:**
- `notify-rust`: optional dependency of the `gui` feature; supplies `get_capabilities()`
  and the notification send call (NFR1).

### File Structure

```
src-tauri/src/
└── callbacks.rs        # escape_for_send / capability predicate + unit tests
```

## Test Scenarios

### Unit Tests
- [ ] TS1 (FR1, FR3): with capabilities = `Err`, `escape_for_send` returns
      (escaped title, escaped body) — added as a pinning unit test.
- [ ] TS2 (FR2): with capabilities = `Ok(empty list or a list without body-markup)`,
      both fields pass through byte-identical.
- [ ] TS3 (FR2 regression): with capabilities = `Ok(["body-markup", ...])`, the existing
      3-character escaping with `&` → `&amp;` applied first is applied to both fields.

### Integration Tests
- [ ] TS4 (NFR3): the whole `--lib` suite passes —
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      (re-run with `-- --test-threads=1` if the `tabs.rs` replay tests prove flaky).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] `get_capabilities()` fails while the immediately following `Notify` succeeds — the
      window this feature targets: text is escaped (FR1).
- [ ] `Ok` with an empty capability list — treated as an explicit "no body-markup" and
      passed through unescaped (FR2, TS2).

### Build / Gate Checks
- [ ] TS5 (NFR1):
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      passes (CLI build gate hygiene).

### Performance Tests
Not applicable — no performance requirement is defined for this feature.

## Security Considerations

- **Input Validation:** OSC 9-derived notification text is treated as untrusted. When the
  server's markup capability cannot be confirmed, the text is escaped rather than passed
  through (FR1).
- **XSS / markup injection Prevention:** Escaping on the unconfirmed path prevents
  `<a href>` / `<img src>` in notification text from being rendered as markup by
  body-markup-capable servers (GNOME Shell / dunst), which is the phishing vector
  reported as finding `eade9e7f97a29a29`.
- **Failure-cost asymmetry:** Over-escaping only degrades display (a literal `&lt;`),
  while under-escaping enables in-notification phishing; the default is therefore biased
  toward escaping.
- **Authentication / Authorization / CSRF / SQL Injection:** Not applicable — the feature
  adds no authenticated surface, no data store, and no web request handling.

## Error Handling

| Condition | Handling | Requirement |
|---|---|---|
| `get_capabilities()` returns `Err(_)` | Treat as unconfirmed, escape both title and body, and still send the notification | FR1, FR3 |

### Error Flow

```
get_capabilities() → Err → decision = escape → escape title & body → send notification
```

## Performance Optimization

Not applicable — no performance goal, optimization strategy, or caching is specified for
this feature.

## Success Criteria

- [ ] A unit test pins that `escape_for_send` applies escaping to both title and body when
      `get_capabilities()` returns `Err(_)`.
- [ ] `Ok` with a list that omits `body-markup` passes through unescaped (existing test
      expectations maintained/updated for the new specification).
- [ ] `Ok` with a list containing `body-markup` escapes (no regression of existing
      behaviour).
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      passes.
- [ ] This SPEC states fail-closed as normative and serves as the closure basis for
      finding `eade9e7f97a29a29`.

## Assumptions

- **A1** (impact low, reversible): The two-way choice "change FR3 to fail-closed, or
  record it as accepted risk" is treated as already resolved in favour of fail-closed by
  the task text's "what we want to do" section (bias to fail-closed, change the default to
  the escaping side). Rationale: the first acceptance-criteria item asks for a decision,
  but a mandatory section of the same task description names the option explicitly.
- **A2** (impact low, reversible): The specification record for fail-closed lives in this
  feature's own SPEC; the previous task's
  `feature-docs/notification-body-markup-escape/SPEC.md` remains as history, with this
  SPEC stating by reference that it supersedes its FR3. Retroactive editing of the
  previous SPEC file itself is not part of the acceptance criteria. Rationale: feature-docs
  are per-feature snapshots, and round1.yaml's `resolution_reason` ("carried over to a
  separate task") points at new specification work in this task.
- **A3** (impact low, reversible): Escaping on the `Err(_)` path extends to
  `title(summary)` as a consequence of `escape_for_send`'s single-decision structure. The
  task's out-of-scope note "escaping of summary(title) (separate task)" refers to an
  independent title-only measure and does not exclude this consequence of the shared
  decision. Rationale: the current code (after PR #35) escapes both title and body when
  confirmed, and splitting the decision per field would complicate the specification.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement (FR1–FR5, NFR1–NFR3) is `resolved`.

## References

- Requirements document: `feature-docs/notification-markup-fail-closed/REQUIREMENTS.md`
- Superseded specification (FR3): `feature-docs/notification-body-markup-escape/SPEC.md`
- PR #35 review round1 finding: `eade9e7f97a29a29` (severity medium / category security /
  confidence 100)
- Implementation site: `src-tauri/src/callbacks.rs` (`escape_for_send`:186,
  `body_markup_confirmed`:220)
