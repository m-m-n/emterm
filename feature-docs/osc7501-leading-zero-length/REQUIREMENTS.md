---
title: "osc7501-leading-zero-length"
created_date: 2026-10-11
status: draft
---

# osc7501-leading-zero-length - Requirements

## 1. Overview

### 1.1 Background

OSC 7501 has a whole-sequence limit of 4096 bytes (THREAT-MODEL TM-1,
osc7501-program-status SPEC FR3). Two defects were reproduced:

- Repro 1 (plain tab): the 4115-byte sequence `ESC ]` + 4096 zeros +
  `7501;state=error` BEL (covered by AC1).
- Repro 2 (mux pane): after `OSC 7501;state=working` BEL,
  `OSC 07501;state=clear` BEL (covered by AC2).

### 1.2 Purpose

- Make the whole-sequence 4096-byte limit for OSC 7501 (THREAT-MODEL TM-1,
  osc7501-program-status SPEC FR3) work as specified, counting the bytes of
  the sequence as received.
- Give the plain-tab terminal parser, mux daemon ingestion and mux history
  stripping the same rule for recognizing the OSC number and body, so that
  plain tabs and mux panes end with the same OSC 7501 state.

### 1.3 Scope

In scope:

- term_core: received byte count of each OSC string passed to the host, and
  saturating OSC number accumulation.
- program_status: the 4096-byte length check on the received byte count.
- mux daemon `AgentStatusFeedScanner`: OSC 7501 and OSC 133 recognition by
  the shared rule.
- mux history stripping (`osc_body_identity` / `client_parity_scan`),
  `osc_identify::recover_osc` and the mux coalesce gate
  (`payload_has_device_query`): agreement with the other paths.
- Renaming one existing test and updating
  `test-docs/osc7501-program-status/task0004.tests.yaml`.

Out of scope:

- OSC 777 agent-status ingestion. It stays canonical-only
  (`777;emterm;agent-status;`), per predecessor FR18.
- Screen, visual or interaction changes (design step skipped).

## 2. Business Requirements

### 2.1 Business Objectives

- The 4096-byte whole-sequence limit for OSC 7501 counts the bytes of the
  sequence as received.
- The plain-tab terminal parser, mux daemon ingestion and mux history
  stripping use the same rule for recognizing the OSC number and body, and
  plain tabs and mux panes end with the same OSC 7501 state.

## 3. Functional Requirements

### 3.1 Requirement List

| ID | Title | Status |
|----|-------|--------|
| FR1 | Sequence length is the received byte count | resolved |
| FR2 | Terminal parser passes the received byte count | resolved |
| FR3 | Mux ingestion recognizes OSC 7501 by the shared rule | resolved |
| FR4 | The three paths agree | resolved |
| FR5 | Canonical spelling is unchanged | resolved |
| FR6 | OSC 133 recognition in mux ingestion | resolved |
| FR7 | OSC number overflow saturates | resolved |
| FR8 | Query answer does not depend on length | resolved |
| FR9 | Updating existing tests and records | resolved |

### 3.2 Requirement Details

#### FR1: Sequence length is the received byte count

The 4096-byte check on an OSC 7501 report counts the sequence as received:

- `ESC ]` (2 bytes)
- every byte in the OSC string: the number's actual digits with leading
  zeros, the first `;`, non-digit bytes before the first `;`, and the body
  before U+FFFD replacement
- the terminator (BEL = 1, ST = 2)

The plain-tab and mux paths both use this definition.

#### FR2: Terminal parser passes the received byte count

term_core passes the host the received byte count of each OSC string. Bytes
beyond `MAX_OSC_LEN` (16 MiB), which the buffer drops, still count (the
count saturates), so a sequence over 4096 bytes is never accepted. term_core
does not embed the number 7501.

#### FR3: Mux ingestion recognizes OSC 7501 by the shared rule

The mux daemon `AgentStatusFeedScanner` recognizes OSC 7501 by the same rule
as term_core and `osc_identify::recover_osc`:

- Digits before the first `;` accumulate in base 10, so leading zeros do not
  change the value.
- Non-digit bytes before the first `;` are part of the body.
- The first `;` is removed.
- A value above the u16 range is not a number.

A body whose number is 7501 is ingested as OSC 7501, and its sequence length
is the received byte count (FR1).

#### FR4: The three paths agree

For the same OSC byte sequence, the terminal parser (plain tab), mux daemon
ingestion and mux history stripping (`osc_body_identity` /
`client_parity_scan`) agree on whether it is OSC 7501, what the body is, and
how long the sequence is. For the same input, a plain tab's OSC 7501 record
table and a mux pane's record table end in the same state.

#### FR5: Canonical spelling is unchanged

For OSC 7501 with the canonical `ESC ] 7501 ;` spelling, parsing, rejection,
query answering, feed order, replay stripping and the mux coalesce gate
(`payload_has_device_query`) behave as before.

#### FR6: OSC 133 recognition in mux ingestion

- The daemon `AgentStatusFeedScanner` recognizes OSC 133 by the shared rule
  in FR3 (leading zeros and non-digit bytes before the first `;` allowed),
  and the mark kind is read from the reconstructed body.
- As today, only a mark whose terminator falls in a live main-screen span is
  emitted.
- An OSC 133 mark in the canonical spelling (body starts with `133;` and the
  first segment after it is a single kind byte) goes to both the OSC 777
  D->A inferred-clear latch and the OSC 7501 prompt-start, as today.
- A mark recognized only by the shared rule applies just the OSC 7501
  prompt-start (working, blocked and idle records removed; done and error
  kept) and does not reach the OSC 777 latch.
- OSC 777 agent-status ingestion stays canonical-only
  (`777;emterm;agent-status;`), per predecessor FR18.

#### FR7: OSC number overflow saturates

term_core's OSC number accumulation saturates on both the multiply and the
add. A number above the u16 range stays at 65535 and matches no OSC code
(native or registered through `register_osc_app_param`). It neither wraps
nor panics, in release or debug builds. This applies to every OSC number.

#### FR8: Query answer does not depend on length

A query whose body is exactly `?` is answered regardless of its received
byte count, as today. The answer is the fixed `ESC ] 7501;?` with the
query's own terminator, once per query. An unterminated query gets no
answer.

#### FR9: Updating existing tests and records

- Rename
  `mux::ipc::pty_spawn::tests::program_status_feed::ac1_only_the_canonical_prefix_is_an_osc_7501`,
  whose expectation is reversed, and give it the new expectation
  (leading-zero 7501 is ingested).
- In AC-1 of `test-docs/osc7501-program-status/task0004.tests.yaml`, update
  the entry to the new name and add a supersede note naming this SPEC's
  FR3/FR4 as a YAML comment. Leave `red_reason` unchanged
  (`.claude/rules/test-docs-records.md`).
- Each updated name must appear in the `cargo test --lib -- --list` output.

## 4. Non-Functional Requirements

- **NFR1:** program_status and osc_identify do not depend on the `gui`
  feature and still build with `--no-default-features`.
- **NFR2:** term_core embeds no application protocol number such as 7501
  (keeps osc7501-program-status A2).
- **NFR3:** Arithmetic on untrusted input never panics (counts and numbers
  saturate). The mux scanner's bounded carry
  (`AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP` = 8 KiB, TM-2) and
  `osc_body_identity`'s allocation-free property (round3 FR8) are kept.
  Bodies of 1 MiB and numbers of 1 million digits still finish within the
  existing time bounds.
- **NFR4:** Rejected input is not logged, as today.
- **NFR5:** Works on both Linux and Windows.

## 5. UI/UX Requirements

Not applicable. The change is limited to the parser and the mux
ingestion/stripping paths; there is no screen, visual or interaction change.

## 6. Constraints

### 6.1 Technical Constraints

- term_core does not embed 7501 (NFR2).
- program_status and osc_identify build without the `gui` feature (NFR1).

### 6.2 Declared Change Set

Feature-specific paths are not listed by hand here; create-plan derives them
from every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them):

- `feature-docs/osc7501-leading-zero-length/**`
- `test-docs/osc7501-leading-zero-length/**`

`feature-docs/osc7501-leading-zero-length/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and any design
artifacts the design step produces. Their generators are the phase documents
and `references/phase-state.md` (cited only; rules not restated).

`test-docs/osc7501-leading-zero-length/**` covers `{T}.tests.yaml` (path form
`test-docs/osc7501-leading-zero-length/{T}.tests.yaml`). Its generator is
`implement-phase.md` (cited only; rules not restated).

**Semantics**:

- Default members are part of the declaration unless the SPEC author
  explicitly removes them. Removal is a deliberate narrowing, not an
  omission.
- The declaration is a superset assertion: the actual change set must be
  CONTAINED IN the declared set. A declared path that is never generated is
  not a violation.

The change also updates
`test-docs/osc7501-program-status/task0004.tests.yaml` (FR9).

## 7. Success Criteria

### 7.1 Acceptance Criteria

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

## 8. Test Scenarios

- [ ] TS1 (FR1, FR8): program_status unit tests. Length check from the
  received byte count (4096 accepted, 4097 rejected, leading zeros counted,
  pre-replacement byte count), and an over-long `?` query is a Query.
- [ ] TS2 (FR2, FR7): term_core parser tests. Received byte count of an OSC
  string (leading zeros, invalid UTF-8, over 16 MiB), and number saturation
  at 65535 / 65536 / 655367501 / 6553652 (no panic).
- [ ] TS3 (FR1, FR2, FR8): plain-tab tests through callbacks /
  output_pipeline. Repro 1, and the answer to an over-long query.
- [ ] TS4 (FR3, FR6): `AgentStatusFeedScanner` tests. Items for leading-zero
  and non-digit-head 7501, 133 marks under the shared rule, live-span
  condition, canonical and non-canonical 133 marks told apart, 0777
  agent-status not ingested, and mixed-stream byte order.
- [ ] TS5 (FR3, FR6): daemon / pane tests. Repro 2. `0133;A` applies only the
  7501 prompt-start and does not fire the 777 latch, while canonical
  `133;D`->`133;A` fires it as before.
- [ ] TS6 (FR4, FR7): parity corpus test. Over one corpus, the term_core
  path (plain-tab table), the daemon scanner (mux table) and
  `osc_body_identity` / `recover_osc` agree.
- [ ] TS7 (FR5, FR8): `payload_has_device_query` and `client_parity_scan`
  treat leading-zero queries as queries and out-of-range numbers as
  non-queries.
- [ ] TS8 (FR9): rename the existing test and update test-docs task0004.

## 9. Confirmed Items

### 9.1 Confirmed

- [x] A1: Leading-zero numbers are accepted as OSC 7501 on every path, not
  rejected.
- [x] A2: term_core does not embed 7501 (keeps osc7501-program-status A2).
- [x] A3: The last three lines of the task description (push/PR, Codex
  consultation, Notion record) are run instructions and are not included in
  the requirements.
- [x] A4: "Do not change the 777 path" means a non-canonical OSC 133 mark
  does not reach the OSC 777 D->A latch (`MuxPane::record_live_osc133_mark`
  currently does both the latch and the 7501 prompt-start).
- [x] A5: No code registers 65535 with `register_osc_app_param`.

### 9.2 Open Items

None.

## 10. References

- osc7501-program-status SPEC FR3, FR18 and A2:
  `feature-docs/osc7501-program-status/SPEC.md`
- THREAT-MODEL TM-1, TM-2
- `test-docs/osc7501-program-status/task0004.tests.yaml`
- `.claude/rules/test-docs-records.md`
