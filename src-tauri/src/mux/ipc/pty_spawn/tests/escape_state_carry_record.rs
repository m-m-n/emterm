//! mux-strip-escape-state-carry task0002 (FR6): the decision record.
//!
//! `feature-docs/mux-strip-escape-state-carry/DECISIONS.md` states the verdict
//! on review finding `a879a02de382209f`, the regression tests that cover it,
//! the splices that remain outside the fix with their occurrence conditions
//! and the reason each is out of scope, the one test whose expectation changes
//! meaning, and the untouched predecessor records. This test reads the record
//! and pins its fixed headings and labels; the wording between them stays free.
//!
//! It does not check that the cited tests exist in the source: another task
//! creates them, and name resolution is checked in verify against the
//! `--lib -- --list` output (IMPLEMENTATION.md D5).

const STABLE_ID: &str = "a879a02de382209f";

/// R1-R11 of IMPLEMENTATION.md "Regression test identifiers", as written there.
const REGRESSION_TESTS: [&str; 11] = [
    "mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output",
    "mux::scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes",
    "mux::scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_reproduction_closes_the_csi_and_replays_no_query",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_a_cut_after_a_written_escape_writes_the_escape_closure",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_escape_closure_replays_like_the_raw_stream",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_escape_closure_has_no_effect_in_term_core",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_a_splice_made_designator_wait_closes_with_the_designator_esc",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_an_overflow_flush_ending_in_a_written_escape_carries_or_closes_it",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_reader_restore_matches_the_raw_stream_reference",
    "mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget",
];

/// The one existing test whose expectation changes meaning (IMPLEMENTATION.md
/// D4): two table rows of R2, in `BEHAVIOR_CHANGING_FILE`.
const BEHAVIOR_CHANGING_TEST: &str = REGRESSION_TESTS[1];
const BEHAVIOR_CHANGING_FILE: &str = "src-tauri/src/mux/scrollback_filter/tests.rs";

const PREDECESSOR_DECISIONS: &str = "feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md";
const PREDECESSOR_ROUND1: &str = "feature-docs/mux-cut-csi-post-strip-closure/reviews/round1.yaml";

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
        .join("../feature-docs/mux-strip-escape-state-carry/DECISIONS.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the decision record {} is unreadable: {e}", path.display()))
}

/// AC-1 to AC-4 (FR6, SPEC AC-7): the record holds the title, one
/// decision-table row for `a879a02de382209f` (requirement, verdict, cause and
/// fix, R1-R11 paths), at least two residual subsections each with an
/// occurrence condition and an out-of-scope reason (the string-introducer
/// splice and the cut-free concatenation), the behavior-changing-tests
/// section, and the two predecessor records named as not modified.
#[test]
fn escape_carry_the_decision_record_states_the_verdict_and_the_residuals() {
    let doc = read_record();

    // The title.
    assert_eq!(
        doc.lines().next().map(str::trim_end),
        Some("# Decisions: mux-strip-escape-state-carry"),
        "the record's title line"
    );

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
    assert_eq!(cells[1], "FR1, FR2, FR3, FR4", "requirement cell");
    assert_eq!(cells[2], "resolved", "verdict cell");
    let rationale = cells[3];
    for needle in [
        // The cause: the written end state narrowed to the CSI sub-state.
        "Escape",
        "CSI sub-state",
        "read split",
        // The fix: IMPLEMENTATION.md D1 on the closure decisions, D2 on the
        // Escape closure.
        "IMPLEMENTATION.md D1",
        "IMPLEMENTATION.md D2",
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
    assert!(
        subsections.len() >= 2,
        "at least two residual subsections: {subsections:?}"
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
    let residual_1 = section_of(residuals, subsections[0]);
    assert!(
        residual_1.contains("string-introducer splice"),
        "Residual 1 names the string-introducer splice: {residual_1}"
    );
    for byte in ["`]`", "`P`", "`_`"] {
        assert!(
            residual_1.contains(byte),
            "Residual 1 names the introducer byte {byte}: {residual_1}"
        );
    }
    let residual_2 = section_of(residuals, subsections[1]);
    assert!(
        residual_2.contains("cut-free concatenation"),
        "Residual 2 names the cut-free concatenation: {residual_2}"
    );
    assert!(
        residual_2.contains(PREDECESSOR_DECISIONS),
        "Residual 2 cites {PREDECESSOR_DECISIONS}: {residual_2}"
    );

    // AC-3: behavior-changing tests.
    let behavior_changing = section_of(&doc, "## Behavior-changing tests");
    for needle in [BEHAVIOR_CHANGING_TEST, BEHAVIOR_CHANGING_FILE] {
        assert!(
            behavior_changing.contains(needle),
            "the behavior-changing tests section names {needle}: {behavior_changing}"
        );
    }
    assert!(
        behavior_changing
            .lines()
            .any(|line| line.contains("No predecessor test-docs record was updated")),
        "the section states that no predecessor test-docs record was updated: {behavior_changing}"
    );

    // AC-4: predecessor records.
    let predecessor = section_of(&doc, "## Predecessor records");
    for record in [PREDECESSOR_DECISIONS, PREDECESSOR_ROUND1] {
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
