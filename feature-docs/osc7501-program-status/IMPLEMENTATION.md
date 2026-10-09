# Implementation Plan: osc7501-program-status

## Overview
eMterm receives OSC 7501 (Program Status Protocol draft 0.3) on plain tabs and
mux panes, keeps a per-terminal record table, and composes its aggregate with
the existing OSC 777 agent status so that the tab badge, mux sidebar,
notifications, the `{agent_status}` status bar variable and the mux agent API
all see one composite pane state, with a new `error` state.

## Technology Stack
- **Language**: Rust — the `src-tauri` crate (plain-tab ingestion, GUI model,
  display, notifications, mux daemon) and the `crates/mux_ipc` crate (wire and
  handoff formats). `crates/term_core` is used through its existing app-param
  mapping and OSC responder hooks and is not modified.
- **Key libraries**: base64 — decoding `title` / `msg`; already a dependency
  of the always-built (non-gui) part of `src-tauri`.
- **New dependencies**: none. The project license (MIT) is unaffected.

## Layer Structure

| Layer | Components | Build | May depend on |
|---|---|---|---|
| Protocol core | SC-1 Program Status core, SC-2 agent state and composition | always built (non-gui) | nothing project-internal; SC-1 and SC-2 do not depend on each other |
| Wire | SC-3 control messages, handoff document | `mux_ipc` crate | nothing in `src-tauri` |
| Ingestion — plain tab | callbacks, tab, output pipeline | gui | Protocol core |
| Ingestion — mux daemon | agent-status feed scanner, SC-5 pane record, daemon tasks, agent API handlers, hot upgrade | always built | Protocol core, Wire |
| GUI model | SC-4 agent-status model, App apply paths | gui | Protocol core, Wire |
| Presentation | tab badge, mux sidebar, notifications, status bar provider, `emterm mux wait` | gui (the wait CLI is always built) | GUI model, Protocol core, Wire |

SC-1 and SC-2 meet only through the state word: a consumer takes the word of
an SC-1 summary state and converts it with SC-2's five-word conversion.

Data flow:
1. Plain tab: PTY output → term_core (7501 mapped to an internal action
   code; the tab's OSC responder sees the terminator) → SC-1 parse → the
   tab's SC-1 table → pending summary list → SC-4 plain-tab summary
   operation → composite → presentation.
2. Mux pane: PTY output → daemon feed scanner (one ordered feed with OSC 777
   reports and live OSC 133 marks) → SC-1 parse → SC-5 pane record (revision
   bump) → SC-3 update message with the summary item → App → SC-4
   daemon-update-with-summary operation → composite → presentation.
3. Query: OSC 7501 `?` → the attached GUI's responder → same-terminator
   answer through the existing device-response route.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| SC-1 Program Status core — `src-tauri/src/program_status.rs` (owner: task0001) | Record-state vocabulary, body parser, record table, lifecycle operations, summary, title sanitization, export/import | See "SC-1 contract" below | task0003, task0004, task0007 |
| SC-2 Agent state and composition — `src-tauri/src/agent_status.rs` (owner: task0002) | Core `error` state, five-word conversion, composition of the OSC 777 state with the OSC 7501 aggregate | See "SC-2 contract" below | task0003, task0004, task0005 |
| SC-3 Wire state and summary item — `crates/mux_ipc/src/protocol.rs` (owner: task0002) | Wire `error` state; optional OSC 7501 summary item on the agent-status update message | See "SC-3 contract" below | task0004 |
| SC-4 GUI agent-status model — `src-tauri/src/agent_status_model.rs` (owner: task0002) | Composite storage per key, plain-tab summary operation, daemon update with summary, names, seven-level aggregation, counts | See "SC-4 contract" below | task0003, task0005 |
| SC-5 Daemon pane agent-status record — `src-tauri/src/mux/session/pane/handles.rs` (owner: task0004) | Holds the pane's SC-1 table with the OSC 777 state and revision; composite accessor | See "SC-5 contract" below | task0007 |

### SC-1 contract
- **State vocabulary**: five stored states with words `idle`, `working`,
  `done`, `blocked`, `error`, each with its D2 rank. `clear` is a report
  action, never a stored state.
- **parse(body, terminator)** — pre: `body` is the text after `7501;` of a
  terminated sequence; `terminator` is BEL or ST. Post: returns exactly one
  of Query (iff the body is exactly `?`), Report (fully validated, ready to
  apply), or Ignored (covers both "discard" and "ignore" of FR3). Pure: no
  side effects, no logging at warn or above.
- **Table** — has an empty default value and can be cloned.
  - apply(Report): post: the table follows FR4 / FR6 (replacement, subtree
    or full clear, 256-record cap with least-recently-updated eviction).
    Every applied report counts as accepted, whether or not stored data
    changed.
  - prompt start: post: `working`, `blocked`, `idle` records removed;
    returns whether anything was removed.
  - reset: post: every record removed; returns whether any existed.
  - summary: post: absent for an empty table; otherwise the deciding
    record's state (highest by D2, ties to the most recently updated), its
    title after D9 sanitization (absent when missing or empty after
    sanitization) and its effective app (own app, else the nearest ancestor
    record's app; the root is every id's ancestor).
  - export: post: every record as plain values (id text with the empty text
    denoting the root, state word, optional kind word, optional progress,
    optional app, optional decoded title, optional decoded msg), least
    recently updated first.
  - import(list in export form): post: a table with the same update order;
    each record re-validated against the FR3 / FR5 rules (an invalid record
    is dropped); at most the 256 most recently updated valid records kept.
- **Title sanitization for names**: the D9 rule, exposed for every caller
  that turns a title into a name.
- Always built; uses only always-built crates (NFR1).

### SC-2 contract
- The core agent state gains `error` with word `error`; its display form is
  the word.
- OSC 777 parsing is unchanged: it still accepts only `idle`, `working`,
  `blocked`, `done`, and the existing four-state collection keeps exactly
  those four members.
- **five-word conversion(word)** — post: maps `idle`, `working`, `blocked`,
  `done`, `error` to the core state; every other input maps to nothing.
- **compose(OSC 777 state, OSC 7501 aggregate)** — both optional. Post: the
  higher of the two by the D2 in-pane rank; one side absent → the other
  side; both absent → no state.

### SC-3 contract
- The wire agent state gains `error`, appended after the existing values;
  its text form is `error`; the existing four values encode exactly as
  before.
- The agent-status update message gains one trailing optional item, the
  OSC 7501 summary: wire state, optional title, optional app. Pre (sender):
  the title is already D9-sanitized and the app passed the SC-1 app rule.
  Post: the item is absent when the pane has no records; it round-trips
  through the control-message encoding.

### SC-4 contract
- Each entry stores the OSC 777 part (state, name) and the OSC 7501 summary
  (core state, sanitized title, app) separately. Everything the model
  reports for an entry — status, aggregate, counts, any-reported-state,
  unseen tracking, transitions — uses the SC-2 composite.
- **plain-tab summary operation(key, optional summary)** — post: stores the
  summary, advances the model-minted revision, recomputes the composite and
  applies the existing unseen / transition rules.
- **daemon update with summary(key, revision, OSC 777 state, name, optional
  summary, replay-derived flag)** — post: stores both parts verbatim with the
  daemon revision. The existing daemon-update operation keeps its signature
  and stores the summary as absent.
- Transitions fire only when the composite changes on a non-replay update;
  their name follows D5.
- Cross-pane aggregation uses the FR11 seven-level order (D2); counts gain an
  `error` bucket.
- An entry whose only input is OSC 777 behaves exactly as before.

### SC-5 contract
- The pane's shared agent-status record holds its SC-1 table (empty by
  default) under the same lock as the OSC 777 state and the revision.
- Composite accessor — post: SC-2 compose of the OSC 777 state and the
  five-word conversion of the table's summary state.
- The table is readable for export and installable at pane construction,
  together with the existing OSC 777 state and revision.

## Conventions
- Rejected or ignored OSC 7501 input is never logged at warn level or
  above: it is untrusted and could flood the log.
- Tests live in the `--lib` target, in the sibling `tests.rs` file of the
  module under test (or the existing test module for that file).
- NFR2: code is written from the SPEC text only; no external
  implementation's code is consulted or copied.
- NFR3: only platform-neutral code; no new platform-gated paths.
- Names: the module and its items use "program status" (the protocol's
  name); state words are the lowercase protocol words.

## Cross-task Design Decisions

### D1: Store layout
OSC 777 entries stay where they are (SC-4 entries, the daemon pane record).
Each plain tab owns its SC-1 table; each mux pane's table lives in the SC-5
record in the daemon; the GUI model holds only the summary. Rationale:
FR9 keeps the OSC 777 stores unchanged and the daemon must keep records
while no GUI is attached (FR14). Affects: task0002, task0003, task0004,
task0007.

### D2: Ranking
- In-pane composition (SC-2 compose) and the SC-1 aggregate use one
  read-flag-free order: blocked > working > error > done > idle. Rationale:
  the daemon has no read flag, and the daemon's wait and the GUI must agree
  on a pane's composite; a child `done` must not mask a `working` root.
  This is a reversible assumption, kept in the single SC-2 rank so it can
  change in one place.
- Cross-pane GUI aggregation uses the FR11 order: blocked > unseen error >
  unseen done > working > seen error > seen done > idle.
Affects: task0001, task0002, task0004, task0005.

### D3: Error state and wire compatibility
New wire enum values and message fields are appended last; the positional
control-message encoding lets an older reader ignore trailing data, and a
mixed-version mismatch falls into the existing malformed-payload warning
path (A8). The OSC 777 grammar and the `emterm agent-status` CLI do not
change. Affects: task0002, task0004.

### D4: Plain-tab ingestion path
App param 7501 maps to an internal action code; the tab's OSC responder
receives the terminator and both answers queries and records reports. The
tab keeps one ordered 7501 feed (reports, prompt-mark candidates, resets)
that the output pipeline reconciles against live main-screen marks. Within
one pump, App applies a tab's OSC 777 inputs before its 7501 summary
changes. Accepted caveat: intermediate transitions may differ from strict
byte order only when both protocols conflict inside one pump; the final
state is the same. The mux path keeps true byte order. Affects: task0003,
task0005.

### D5: Name selection
When the SC-1 summary state ranks at or above the OSC 777 state by D2 (ties
go to OSC 7501), the name is the summary's title, else its app, else the
OSC 777 name, else none. Otherwise the name is the OSC 777 name, else none.
"None" falls back to the existing default name. Affects: task0002.

### D6: Query routing and replay
Only an attached GUI answers queries (A3); the daemon never answers. OSC 7501
reports and queries are stripped from the daemon ring and snapshots;
suppressed-output parity delivers 7501 queries to the attached GUI the way it
delivers color queries. The GUI discards 7501 state parsed from mux inner
content. Affects: task0003, task0004, task0006.

### D7: Daemon revision rules
Every accepted report bumps the pane revision once. Prompt start and reset
bump only when the table changed. Queries never bump. Each bump re-evaluates
waiters and broadcasts one non-replay update carrying the SC-3 summary
item. Snapshot, reattach and window-switch resync messages also carry the
summary item. Affects: task0004, task0006.

### D8: Handoff
The handoff document carries each pane's records as plain values defined in
`mux_ipc` (state as its word) in update order, under the next schema version;
restore goes through SC-1 import, which re-validates and applies the cap.
Affects: task0001, task0007.

### D9: Title sanitization for names
Remove control characters (C0, DEL, C1) and invisible formatting
characters — at least U+00AD, U+061C, U+200B–U+200F, U+2028–U+202E,
U+2060–U+2064, U+2066–U+2069 and U+FEFF — then truncate to 80 Unicode scalar
values. The rule uses only always-built crates. The daemon sends only
sanitized titles; the GUI uses them as received. Affects: task0001, task0002,
task0004.

### D10: Missing-foundation rule
Tasks run in parallel in separate worktrees. When a consumer's worktree
lacks a shared component, the consumer adds the minimum of it at the owner's
path with the owner's names — enough to compile and to pass its own
Acceptance Criteria — and writes no tests for owner-only behavior. The
owner's version supersedes it through parent-side adoption at merge. Wiring
owners: plain-tab ingestion (task0003), daemon ingestion and wait (task0004),
GUI apply of the wire summary (task0002), status bar (task0005), handoff
(task0007). Affects: task0003, task0004, task0007.

### D11: File-overlap map

| File | Tasks |
|---|---|
| `src-tauri/src/app/agent_status.rs` | task0002, task0003, task0005 |
| `src-tauri/src/app/tests/agent_status.rs` | task0002, task0003, task0005 |
| `src-tauri/src/app/mod.rs` | task0003, task0005 |
| `src-tauri/src/agent_status_model.rs` | task0002 (owner), task0003 (seam) |
| `src-tauri/src/agent_status.rs` | task0002 (owner), task0003 and task0004 (seams) |
| `crates/mux_ipc/src/protocol.rs` | task0002 (owner), task0004 (seam) |
| `src-tauri/src/mux/daemon/tasks.rs` | task0002, task0004 |
| `src-tauri/src/program_status.rs` and `src-tauri/src/lib.rs` | task0001 (owner), task0003, task0004 and task0007 (seams) |
| `src-tauri/src/mux/session/pane/handles.rs` | task0004 (owner), task0007 (seam) |
| `src-tauri/src/mux/session/pane/mod.rs` | task0004, task0007 |

Each task changes only the parts of a shared file its own plan names.

### D12: Planning inputs
File paths were confirmed by targeted source reads at planning time; line
positions are not pinned. The internal action code chosen for 7501 was
unused at planning time; the implementing task confirms it is still free.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Merge conflicts on the shared App and model files | High | Medium | D11 map, D10 seams with the owner's names, parent-side adoption |
| A core installed by the off-thread swap loses the 7501 mapping or responder | Medium | High | task0003 AC-1 covers the swap path |
| The D2 in-pane order differs from user expectation | Medium | Low | One rank in SC-2; recorded as a reversible assumption |
| Mixed GUI / daemon versions drop update messages | Low | Medium | Append-last encoding and the A8 malformed-payload path (D3) |
| A handoff schema change breaks restoring older documents | Low | High | Per-version decode shapes; task0007 AC-2 |
| Replay re-answers a query or re-applies stale records | Medium | Medium | D6 stripping and inner-content discard (TM-5) |

## Open Questions
- [ ] README and the settings template hint do not list `{agent_status}`
  (task0005 leaves them unchanged).
- [ ] D2 in-pane order (blocked > working > error > done > idle) is an
  assumption; FR10 / FR11 do not fix the order without a read flag.
- [ ] D4 within-pump ordering caveat on plain tabs.
- [ ] FR19 is an exclusion statement and has no implementing task.
- [ ] NFR2, NFR3 and NFR4 have no automated test (review and manual checks).
