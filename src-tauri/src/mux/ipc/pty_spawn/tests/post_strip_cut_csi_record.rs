//! mux-cut-csi-post-strip-closure task0002 (FR5, FR6): the decision record.
//!
//! `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` states the
//! verdict on review finding `4c0ad9058a983648`, the regression tests that
//! cover it, and the three cut-free strip concatenation residuals with their
//! occurrence conditions and the reason each is outside this fix. This test
//! reads the record and pins its fixed headings and labels; the wording
//! between them stays free.
//!
//! It does not check that the cited tests exist in the source: another task
//! creates them, and name resolution is checked in verify against the
//! `--lib -- --list` output (VERIFICATION.md TS-6).

const STABLE_ID: &str = "4c0ad9058a983648";

/// R1-R7 of IMPLEMENTATION.md "Regression test identifiers", as written there.
const REGRESSION_TESTS: [&str; 7] = [
    "mux::ipc::pty_spawn::tests::round4_cut_csi::round4_fr4_the_closing_is_written_only_when_the_emitted_stream_ends_inside_a_csi",
    "mux::ipc::pty_spawn::tests::round4_cut_csi::post_strip_a_cut_after_a_stripped_construct_replays_like_the_raw_stream",
    "mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output",
    "mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_an_overflow_flush_followed_by_a_cut_closes_an_open_csi",
    "mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_reader_restore_matches_the_raw_stream_reference",
    "mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_closing_follows_the_stripped_output",
    "mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget",
];

const ROUND1_RECORD: &str = "feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml";
const DECISIONS_RECORD: &str = "feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md";

/// The text of the section that starts at the line `heading` and runs to the
/// next heading of the same or a higher level.
fn section_of<'a>(doc: &'a str, heading: &str) -> &'a str {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut offset = 0usize;
    let mut start = None;
    for line in doc.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == heading {
            start = Some(offset);
            break;
        }
        offset += line.len();
    }
    let start = start.unwrap_or_else(|| panic!("DECISIONS.md has no heading {heading:?}"));
    let body = &doc[start..];
    let mut end = body.len();
    let mut at = 0usize;
    for (index, line) in body.split_inclusive('\n').enumerate() {
        if index > 0 {
            let line = line.trim_end_matches(['\r', '\n']);
            let hashes = line.chars().take_while(|c| *c == '#').count();
            if hashes > 0 && hashes <= level && line[hashes..].starts_with(' ') {
                end = at;
                break;
            }
        }
        at += line.len();
    }
    &body[..end]
}

/// The cells of one markdown table row, trimmed.
fn cells_of(row: &str) -> Vec<&str> {
    row.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// The text after `label` on the first line of `section` that starts with it.
fn labelled_text<'a>(section: &'a str, label: &str) -> &'a str {
    let line = section
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("no line starts with {label:?} in:\n{section}"));
    line[label.len()..].trim()
}

fn read_record() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the decision record {} is unreadable: {e}", path.display()))
}

/// AC-1 to AC-5 (FR5, FR6, SPEC AC-6): the record holds one decision-table row
/// for `4c0ad9058a983648` (requirement, verdict, cause and fix, R1-R7 paths),
/// three residual subsections each with an occurrence condition and an
/// out-of-scope reason, the behavior-changing-tests statement and the two
/// predecessor records named as not modified.
#[test]
fn post_strip_the_decision_record_states_the_verdict_and_the_residuals() {
    let doc = read_record();

    // AC-1: the decision table.
    let table = section_of(&doc, "## Decision table");
    let header = table
        .lines()
        .find(|line| line.starts_with('|'))
        .expect("the decision table has a header row");
    assert_eq!(
        cells_of(header),
        [
            "stable_id",
            "requirement",
            "verdict",
            "rationale",
            "regression test"
        ],
        "the decision table's columns"
    );
    let rows: Vec<&str> = table
        .lines()
        .filter(|line| line.starts_with('|') && line.contains(STABLE_ID))
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "exactly one decision-table row for {STABLE_ID}: {rows:?}"
    );
    let cells = cells_of(rows[0]);
    assert_eq!(cells.len(), 5, "the row has five cells: {cells:?}");
    assert_eq!(cells[0], STABLE_ID, "stable_id cell");
    assert_eq!(cells[1], "FR1, FR2, FR3", "requirement cell");
    assert_eq!(cells[2], "resolved", "verdict cell");
    let rationale = cells[3];
    for needle in [
        // The cause: the boundary scan's pre-strip CSI state.
        "boundary scan",
        "pre-strip",
        "cut",
        "carry",
        // The fix: IMPLEMENTATION.md D1 on the three decisions.
        "IMPLEMENTATION.md D1",
        "in-call cut",
        "fallback closing",
        "overflow flush",
    ] {
        assert!(
            rationale.contains(needle),
            "the rationale names {needle:?}: {rationale}"
        );
    }
    for test in REGRESSION_TESTS {
        assert!(
            cells[4].contains(test),
            "the regression-test cell lists {test}: {}",
            cells[4]
        );
    }

    // AC-2: the residuals.
    let residuals = section_of(&doc, "## Residuals (FR6)");
    let subsections: Vec<&str> = residuals
        .lines()
        .filter(|line| line.starts_with("### "))
        .collect();
    assert_eq!(
        subsections.len(),
        3,
        "three residual subsections: {subsections:?}"
    );
    for (index, heading_line) in subsections.iter().enumerate() {
        let prefix = format!("### Residual {}", index + 1);
        assert!(
            heading_line.starts_with(&prefix),
            "subsection {} starts with {prefix:?}: {heading_line:?}",
            index + 1
        );
        let section = section_of(residuals, heading_line);
        for label in ["Occurrence condition:", "Out of scope because:"] {
            assert!(
                !labelled_text(section, label).is_empty(),
                "{heading_line}: the {label:?} line carries text"
            );
        }
    }

    // AC-3: behavior-changing tests.
    let behavior_changing = section_of(&doc, "## Behavior-changing tests");
    assert!(
        behavior_changing
            .lines()
            .any(|line| line.starts_with("None.")),
        "the behavior-changing tests section states None.: {behavior_changing}"
    );
    assert!(
        behavior_changing.contains("No predecessor test-docs record was updated"),
        "the section states that no predecessor test-docs record was updated: {behavior_changing}"
    );

    // AC-4: predecessor records.
    let predecessor = section_of(&doc, "## Predecessor records");
    for record in [ROUND1_RECORD, DECISIONS_RECORD] {
        assert!(
            predecessor.contains(record),
            "the predecessor-records section names {record}: {predecessor}"
        );
    }
    assert!(
        predecessor.contains("not modified"),
        "the predecessor-records section states they are not modified: {predecessor}"
    );
}
