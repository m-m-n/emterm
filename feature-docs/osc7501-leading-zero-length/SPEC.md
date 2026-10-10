# Feature: osc7501-leading-zero-length

## Overview

The 4096-byte whole-sequence limit for OSC 7501 counts the bytes of the
sequence as received, including leading zeros in the OSC number, non-digit
bytes before the first `;` and the body before U+FFFD replacement. The
plain-tab terminal parser, mux daemon ingestion and mux history stripping
recognize the OSC number and body by one shared rule, so plain tabs and mux
panes end with the same OSC 7501 state. Requirements:
`feature-docs/osc7501-leading-zero-length/REQUIREMENTS.md`.

## Objectives

- Make the whole-sequence 4096-byte limit for OSC 7501 (THREAT-MODEL TM-1,
  osc7501-program-status SPEC FR3) work as specified, counting the bytes of
  the sequence as received.
- Give the plain-tab terminal parser, mux daemon ingestion and mux history
  stripping the same rule for recognizing the OSC number and body, so that
  plain tabs and mux panes end with the same OSC 7501 state.

## Acceptance Criteria

- [ ] AC1 (FR1, FR2, repro 1): In a plain tab, the 4115-byte sequence
  `ESC ]` + 4096 zeros + `7501;state=error` BEL registers no record.
- [ ] AC2 (FR3, FR4, repro 2): In a mux pane, after `OSC 7501;state=working`
  BEL, `OSC 07501;state=clear` BEL removes the working record. This is the
  same result as a plain tab.
- [ ] AC3 (FR1, FR3): A report whose received byte count is exactly 4096,
  leading zeros included, is accepted and one of 4097 is rejected, on both
  plain tab and mux and with both BEL and ST.
- [ ] AC4 (FR1, FR2): A body containing invalid UTF-8 is measured by its
  byte count before replacement. A report at 4096 bytes or less is accepted
  even when U+FFFD replacement would make it look longer than 4096, with the
  same result on plain tab and mux.
- [ ] AC5 (FR4): Over the same input corpus (leading zeros, non-digit bytes
  before `;`, invalid UTF-8, boundary lengths, both terminators, numbers
  near 7501, out-of-range numbers), the plain-tab record-table state, the
  mux-pane record-table state and the history-stripping verdict all agree.
- [ ] AC6 (FR6): In a mux pane, a live main-screen `OSC 0133;A` removes
  working, blocked and idle records and keeps done and error, the same as a
  plain tab. `OSC 133;D` followed by `OSC 0133;A` does not fire the OSC 777
  inferred clear (the 777 state stays). `133;D` followed by `133;A` still
  fires it as before. `OSC 0777;emterm;agent-status;...` is not ingested by
  the daemon, as today.
- [ ] AC7 (FR7): term_core dispatches `OSC 65535;x` with number 65535, and
  `65536;x` and `655367501;state=error` with the saturated 65535. Neither
  reaches any code: no OSC 7501 record and no callback for a registered
  code. `6553652;...` is not dispatched as OSC 52. Debug-build tests do not
  panic. osc_identify and mux ingestion also treat these inputs as
  non-7501.
- [ ] AC8 (FR8): A query of leading zeros + `7501;?` whose received byte
  count exceeds 4096 gets exactly one answer with its own terminator. The
  mux coalesce gate (`payload_has_device_query`) and `client_parity_scan`
  treat `07501;?` as a query.
- [ ] AC9 (FR5, FR9): Every existing test for the canonical spelling
  passes. The renamed test and test-docs task0004 AC-1 are updated, and the
  name resolves in the `cargo test --lib -- --list` output. A regression
  test that catches each reproduced defect exists.

## Technical Requirements

### Functional Requirements

- **FR1:** Sequence length is the received byte count. The 4096-byte check
  on an OSC 7501 report counts the sequence as received: `ESC ]` (2 bytes),
  every byte in the OSC string (the number's actual digits with leading
  zeros, the first `;`, non-digit bytes before the first `;`, and the body
  before U+FFFD replacement), and the terminator (BEL = 1, ST = 2). The
  plain-tab and mux paths both use this definition.
- **FR2:** Terminal parser passes the received byte count. term_core passes
  the host the received byte count of each OSC string. Bytes beyond
  `MAX_OSC_LEN` (16 MiB), which the buffer drops, still count (the count
  saturates), so a sequence over 4096 bytes is never accepted. term_core
  does not embed the number 7501.
- **FR3:** Mux ingestion recognizes OSC 7501 by the shared rule. The mux
  daemon `AgentStatusFeedScanner` recognizes OSC 7501 by the same rule as
  term_core and `osc_identify::recover_osc`. Digits before the first `;`
  accumulate in base 10, so leading zeros do not change the value.
  Non-digit bytes before the first `;` are part of the body. The first `;`
  is removed. A value above the u16 range is not a number. A body whose
  number is 7501 is ingested as OSC 7501, and its sequence length is the
  received byte count (FR1).
- **FR4:** The three paths agree. For the same OSC byte sequence, the
  terminal parser (plain tab), mux daemon ingestion and mux history
  stripping (`osc_body_identity` / `client_parity_scan`) agree on whether it
  is OSC 7501, what the body is, and how long the sequence is. For the same
  input, a plain tab's OSC 7501 record table and a mux pane's record table
  end in the same state.
- **FR5:** Canonical spelling is unchanged. For OSC 7501 with the canonical
  `ESC ] 7501 ;` spelling, parsing, rejection, query answering, feed order,
  replay stripping and the mux coalesce gate (`payload_has_device_query`)
  behave as before.
- **FR6:** OSC 133 recognition in mux ingestion. The daemon
  `AgentStatusFeedScanner` recognizes OSC 133 by the shared rule in FR3
  (leading zeros and non-digit bytes before the first `;` allowed), and the
  mark kind is read from the reconstructed body. As today, only a mark whose
  terminator falls in a live main-screen span is emitted. An OSC 133 mark in
  the canonical spelling (body starts with `133;` and the first segment
  after it is a single kind byte) goes to both the OSC 777 D->A
  inferred-clear latch and the OSC 7501 prompt-start, as today. A mark
  recognized only by the shared rule applies just the OSC 7501 prompt-start
  (working, blocked and idle records removed; done and error kept) and does
  not reach the OSC 777 latch. OSC 777 agent-status ingestion stays
  canonical-only (`777;emterm;agent-status;`), per predecessor FR18.
- **FR7:** OSC number overflow saturates. term_core's OSC number
  accumulation saturates on both the multiply and the add. A number above
  the u16 range stays at 65535 and matches no OSC code (native or
  registered through `register_osc_app_param`). It neither wraps nor
  panics, in release or debug builds. This applies to every OSC number.
- **FR8:** Query answer does not depend on length. A query whose body is
  exactly `?` is answered regardless of its received byte count, as today.
  The answer is the fixed `ESC ] 7501;?` with the query's own terminator,
  once per query. An unterminated query gets no answer.
- **FR9:** Updating existing tests and records. Rename
  `mux::ipc::pty_spawn::tests::program_status_feed::ac1_only_the_canonical_prefix_is_an_osc_7501`,
  whose expectation is reversed, and give it the new expectation
  (leading-zero 7501 is ingested). In AC-1 of
  `test-docs/osc7501-program-status/task0004.tests.yaml`, update the entry
  to the new name and add a supersede note naming this SPEC's FR3/FR4 as a
  YAML comment. Leave `red_reason` unchanged
  (`.claude/rules/test-docs-records.md`). Each updated name must appear in
  the `cargo test --lib -- --list` output.

### Non-Functional Requirements

- **NFR1 - Feature gate:** program_status and osc_identify do not depend on
  the `gui` feature and still build with `--no-default-features`.
- **NFR2 - Protocol independence:** term_core embeds no application protocol
  number such as 7501 (keeps osc7501-program-status A2).
- **NFR3 - Robustness:** Arithmetic on untrusted input never panics (counts
  and numbers saturate). The mux scanner's bounded carry
  (`AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP` = 8 KiB, TM-2) and
  `osc_body_identity`'s allocation-free property (round3 FR8) are kept.
  Bodies of 1 MiB and numbers of 1 million digits still finish within the
  existing time bounds.
- **NFR4 - Logging:** Rejected input is not logged, as today.
- **NFR5 - Platform:** Works on both Linux and Windows.

## Implementation Approach

### Components

| Component | Requirements |
|---|---|
| term_core OSC parser (received byte count, number accumulation) | FR2, FR7, NFR2 |
| program_status (4096-byte length check, query classification) | FR1, FR8, NFR1 |
| mux daemon `AgentStatusFeedScanner` | FR3, FR6, NFR3 |
| `MuxPane::record_live_osc133_mark` (OSC 777 D->A latch, OSC 7501 prompt-start) | FR6 |
| `osc_identify::recover_osc`, `osc_body_identity`, `client_parity_scan` | FR3, FR4, NFR1, NFR3 |
| `payload_has_device_query` (mux coalesce gate) | FR5, FR8 |
| `mux::ipc::pty_spawn::tests::program_status_feed`, `test-docs/osc7501-program-status/task0004.tests.yaml` | FR9 |

### Data Flow

```
Plain tab:  PTY bytes -> term_core (OSC number + body + received byte count)
                      -> 4096-byte check (FR1) -> plain-tab record table
Mux pane:   PTY bytes -> daemon AgentStatusFeedScanner (shared rule, FR3)
                      -> 4096-byte check (FR1) -> mux-pane record table
History:    mux history bytes -> osc_body_identity / client_parity_scan
                              -> stripping verdict
```

All three paths apply the shared recognition rule (FR3) and the received
byte count (FR1), and agree on the result (FR4).

### Constants

| Name | Value |
|---|---|
| OSC 7501 whole-sequence limit | 4096 bytes |
| `MAX_OSC_LEN` | 16 MiB |
| `AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP` | 8 KiB |
| Saturated OSC number | 65535 |

### Dependencies

**Internal Dependencies:**
- osc7501-program-status: OSC 7501 parsing, record table, query answer,
  feed scanner and replay stripping this feature changes.

**External Dependencies:**
- None.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths:

- `feature-docs/osc7501-leading-zero-length/**`
- `test-docs/osc7501-leading-zero-length/**`

`feature-docs/osc7501-leading-zero-length/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/osc7501-leading-zero-length/**` covers
`test-docs/osc7501-leading-zero-length/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/osc7501-leading-zero-length/` directory at all; the declared
`test-docs/osc7501-leading-zero-length/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

FR9 also updates `test-docs/osc7501-program-status/task0004.tests.yaml`.

## Test Scenarios

### Unit Tests
- [ ] TS1 (FR1, FR8): program_status unit tests - Length check from the
  received byte count (4096 accepted, 4097 rejected, leading zeros counted,
  pre-replacement byte count), and an over-long `?` query is a Query.
- [ ] TS2 (FR2, FR7): term_core parser tests - Received byte count of an
  OSC string (leading zeros, invalid UTF-8, over 16 MiB), and number
  saturation at 65535 / 65536 / 655367501 / 6553652 (no panic).
- [ ] TS4 (FR3, FR6): `AgentStatusFeedScanner` tests - Items for
  leading-zero and non-digit-head 7501, 133 marks under the shared rule,
  live-span condition, canonical and non-canonical 133 marks told apart,
  0777 agent-status not ingested, and mixed-stream byte order.
- [ ] TS7 (FR5, FR8): `payload_has_device_query` and `client_parity_scan` -
  Leading-zero queries are queries and out-of-range numbers are
  non-queries.

### Integration Tests
- [ ] TS3 (FR1, FR2, FR8): plain-tab tests through callbacks /
  output_pipeline - Repro 1, and the answer to an over-long query.
- [ ] TS5 (FR3, FR6): daemon / pane tests - Repro 2. `0133;A` applies only
  the 7501 prompt-start and does not fire the 777 latch, while canonical
  `133;D`->`133;A` fires it as before.
- [ ] TS6 (FR4, FR7): parity corpus test - Over one corpus, the term_core
  path (plain-tab table), the daemon scanner (mux table) and
  `osc_body_identity` / `recover_osc` agree.
- [ ] TS8 (FR9): rename the existing test and update test-docs task0004 -
  The new name resolves in the `cargo test --lib -- --list` output.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Received byte count exactly 4096 (accepted) and 4097 (rejected),
  leading zeros included, with BEL and ST (AC3).
- [ ] Invalid UTF-8 body: measured before U+FFFD replacement (AC4).
- [ ] OSC string over `MAX_OSC_LEN` (16 MiB): the count saturates and the
  sequence is rejected (FR2).
- [ ] OSC numbers 65535, 65536, 655367501, 6553652: above-range values
  saturate at 65535 and match no code (AC7).
- [ ] Over-long leading-zero `7501;?` query: exactly one answer; an
  unterminated query gets no answer (FR8, AC8).
- [ ] Non-canonical OSC 133 mark (`0133;A`): 7501 prompt-start only, no 777
  latch (AC6).

### Performance Tests
- [ ] Bodies of 1 MiB and numbers of 1 million digits finish within the
  existing time bounds (NFR3).

## Security Considerations

- **Input Validation:** The 4096-byte limit is applied to the received byte
  count (FR1, FR2). Counts and OSC numbers saturate; arithmetic on untrusted
  input never panics (FR7, NFR3). The mux scanner's carry stays bounded at
  8 KiB and `osc_body_identity` stays allocation-free (NFR3).

## Error Handling

- A report over 4096 received bytes is rejected (FR1).
- Rejected input is not logged, as today (NFR4).

## Assumptions

- A1: Leading-zero numbers are accepted as OSC 7501 on every path, not
  rejected.
- A2: term_core does not embed 7501 (keeps osc7501-program-status A2).
- A3: The last three lines of the task description (push/PR, Codex
  consultation, Notion record) are run instructions and are not included in
  the requirements.
- A4: "Do not change the 777 path" means a non-canonical OSC 133 mark does
  not reach the OSC 777 D->A latch (`MuxPane::record_live_osc133_mark`
  currently does both the latch and the 7501 prompt-start).
- A5: No code registers 65535 with `register_osc_app_param`.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Every existing test for the canonical spelling passes (FR5)
- [ ] `--no-default-features` build succeeds (NFR1)

## Open Questions

> **Note**: Unresolved requirements are tracked in workflow.yaml as
> `status: tbd`. Resolve them before the plan phase.

None.

## References

- Requirements: `feature-docs/osc7501-leading-zero-length/REQUIREMENTS.md`
- osc7501-program-status SPEC FR3, FR18 and A2:
  `feature-docs/osc7501-program-status/SPEC.md`
- THREAT-MODEL TM-1, TM-2
- `test-docs/osc7501-program-status/task0004.tests.yaml`
- `.claude/rules/test-docs-records.md`
