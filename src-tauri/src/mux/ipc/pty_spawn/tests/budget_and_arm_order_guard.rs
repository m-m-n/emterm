//! pty-spawn-parallel-budget-flake task0001 (FR6, NFR1, NFR3, NFR4): the
//! source-scan guard of the five tests whose verdict must not depend on machine
//! load.
//!
//! Three budget tests judge their production calls by thread CPU time through a
//! `CpuMeter`; two reattach tests arm the second P2 stop before the first
//! release. This guard reads the source text of the target test files, embedded
//! at compile time (no runtime file access and no path resolution, so Linux and
//! Windows judge the same text and a CR LF checkout is judged like an LF one),
//! and fails when one of the five bodies returns to the wall-clock budget or to
//! the release-before-arm order.
//!
//! Token contract (IMPLEMENTATION.md C2):
//!
//! - a budget test body contains `CpuMeter::new`, `.measure(` and
//!   `.judge(BUDGET)`, and contains neither `Instant::now` nor `.elapsed()`;
//! - an arm-order test body contains at least two `p2.arm()`, and its first
//!   `release_tx.send(` comes after its second `p2.arm()`;
//! - whole-line `//` comments inside a body are ignored, so a comment may
//!   describe the order or the old form in words without affecting the verdict.
//!
//! A body is found by the exact text `fn` + space + the test name + `(` and ends
//! at the end of that function; text of any other function is never part of it.
//! The judging functions are pure functions of a source text and a function
//! name. The guard calls no target test and no production code, and it never
//! scans its own file.

use std::fmt;

/// The budget test sources, embedded at compile time.
const ESCAPE_STATE_CARRY_SOURCE: &str = include_str!("escape_state_carry.rs");
const POST_STRIP_CUT_CSI_SOURCE: &str = include_str!("post_strip_cut_csi.rs");
const STRIP_OPEN_STRING_BODY_CLOSURE_SOURCE: &str =
    include_str!("strip_open_string_body_closure.rs");
/// The source of `pty_spawn/tests.rs`, which holds the two arm-order tests.
const PTY_SPAWN_TESTS_SOURCE: &str = include_str!("../tests.rs");

/// The tokens a budget test body must hold.
const REQUIRED_BUDGET_TOKENS: [&str; 3] = ["CpuMeter::new", ".measure(", ".judge(BUDGET)"];
/// The wall-clock tokens a budget test body must not hold.
const FORBIDDEN_BUDGET_TOKENS: [&str; 2] = ["Instant::now", ".elapsed()"];
/// The arm call the arm-order tests count.
const ARM_TOKEN: &str = "p2.arm()";
/// The release call whose first occurrence must follow the second arm.
const RELEASE_TOKEN: &str = "release_tx.send(";

/// One guarded test: the file it lives in, that file's embedded text and its name.
struct GuardedTest {
    file: &'static str,
    source: &'static str,
    name: &'static str,
}

/// The three budget tests (FR1-FR3).
const BUDGET_TESTS: [GuardedTest; 3] = [
    GuardedTest {
        file: "tests/escape_state_carry.rs",
        source: ESCAPE_STATE_CARRY_SOURCE,
        name: "escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget",
    },
    GuardedTest {
        file: "tests/post_strip_cut_csi.rs",
        source: POST_STRIP_CUT_CSI_SOURCE,
        name: "post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget",
    },
    GuardedTest {
        file: "tests/strip_open_string_body_closure.rs",
        source: STRIP_OPEN_STRING_BODY_CLOSURE_SOURCE,
        name: "open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal",
    },
];

/// The two arm-order tests (FR4, FR5).
const ARM_ORDER_TESTS: [GuardedTest; 2] = [
    GuardedTest {
        file: "tests.rs",
        source: PTY_SPAWN_TESTS_SOURCE,
        name: "visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once",
    },
    GuardedTest {
        file: "tests.rs",
        source: PTY_SPAWN_TESTS_SOURCE,
        name: "on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once",
    },
];

/// The outcome of judging one named function in a source text.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// The body keeps the token contract.
    Accepted,
    /// The source has no such function; carries the function's name.
    Missing(String),
    /// The body breaks the token contract; carries the forbidden or absent token.
    Rejected(String),
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Accepted => write!(f, "accepted"),
            Verdict::Missing(name) => write!(
                f,
                "the function `{name}` is missing from the source (or its closing brace was not found)"
            ),
            Verdict::Rejected(reason) => write!(f, "{reason}"),
        }
    }
}

// ── The body finder ───────────────────────────────────────────────────────

/// Whether the match of `fn name(` at byte `at` of `source` starts a function: the
/// text before it on its line is not a `//` comment and `fn` does not end another
/// word.
fn starts_a_function(source: &str, at: usize) -> bool {
    let line_start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let in_a_comment = source[line_start..at].trim_start().starts_with("//");
    let ends_a_word = source[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    !in_a_comment && !ends_a_word
}

/// The text of the function `name` in `source`, from the line holding `fn` +
/// space + `name` + `(` to the line holding only its closing brace at the
/// indentation of that first line. Lines are joined with LF, so CR LF and LF line
/// ends give the same text. `None` when no such function (or no such closing
/// line) exists; text of any later function is never included.
fn function_body(source: &str, name: &str) -> Option<String> {
    let needle = format!("fn {name}(");
    let at = source
        .match_indices(&needle)
        .map(|(at, _)| at)
        .find(|&at| starts_a_function(source, at))?;
    let line_start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let mut lines = source[line_start..].lines();
    let first = lines.next()?;
    let indent: String = first.chars().take_while(|c| c.is_whitespace()).collect();
    let closing = format!("{indent}}}");
    let mut body = vec![first];
    for line in lines {
        body.push(line);
        if line == closing {
            return Some(body.join("\n"));
        }
    }
    None
}

/// `body` without its whole-line `//` comments (doc comments included).
fn code_of(body: &str) -> String {
    body.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── The judgments ─────────────────────────────────────────────────────────

/// Judges one budget test body (IMPLEMENTATION.md C2, first bullet).
fn judge_budget_test(source: &str, name: &str) -> Verdict {
    let Some(body) = function_body(source, name) else {
        return Verdict::Missing(name.to_string());
    };
    let code = code_of(&body);
    for token in FORBIDDEN_BUDGET_TOKENS {
        if code.contains(token) {
            return Verdict::Rejected(format!("holds the forbidden wall-clock token `{token}`"));
        }
    }
    for token in REQUIRED_BUDGET_TOKENS {
        if !code.contains(token) {
            return Verdict::Rejected(format!("lacks the required token `{token}`"));
        }
    }
    Verdict::Accepted
}

/// Judges one arm-order test body (IMPLEMENTATION.md C2, second bullet).
fn judge_arm_order_test(source: &str, name: &str) -> Verdict {
    let Some(body) = function_body(source, name) else {
        return Verdict::Missing(name.to_string());
    };
    let code = code_of(&body);
    let arms: Vec<usize> = code.match_indices(ARM_TOKEN).map(|(at, _)| at).collect();
    let Some(second_arm) = arms.get(1) else {
        return Verdict::Rejected(format!(
            "holds fewer than two `{ARM_TOKEN}` ({} found)",
            arms.len()
        ));
    };
    let Some(first_release) = code.find(RELEASE_TOKEN) else {
        return Verdict::Rejected(format!("holds no `{RELEASE_TOKEN}`"));
    };
    if first_release < *second_arm {
        return Verdict::Rejected(format!(
            "the first `{RELEASE_TOKEN}` comes before the second `{ARM_TOKEN}`"
        ));
    }
    Verdict::Accepted
}

// ── The real-source guard (AC-7) ──────────────────────────────────────────

/// AC-7 (FR6, NFR3): the three budget tests and the two arm-order tests, read
/// from the real files embedded at compile time, are each found and accepted.
/// A failure names the test, its file and the reason.
#[test]
fn the_five_target_tests_keep_the_load_independent_forms() {
    let mut failures = Vec::new();
    for test in &BUDGET_TESTS {
        let verdict = judge_budget_test(test.source, test.name);
        if verdict != Verdict::Accepted {
            failures.push(format!("{} ({}): {verdict}", test.name, test.file));
        }
    }
    for test in &ARM_ORDER_TESTS {
        let verdict = judge_arm_order_test(test.source, test.name);
        if verdict != Verdict::Accepted {
            failures.push(format!("{} ({}): {verdict}", test.name, test.file));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of the five target tests left the load-independent form:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ── The judgments' own tests (AC-4, AC-5, AC-6) ───────────────────────────

/// A test function named `name` whose body is `lines`, at the top level.
fn function(name: &str, lines: &[&str]) -> String {
    let mut text = format!("#[test]\nfn {name}() {{\n");
    for line in lines {
        text.push_str("    ");
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("}\n");
    text
}

/// Functions joined the way a source file holds them.
fn source_of(functions: &[String]) -> String {
    functions.join("\n")
}

/// The wall-clock form of a budget body.
const WALL_CLOCK_LINES: [&str; 3] = [
    "let start = std::time::Instant::now();",
    "run();",
    "assert!(start.elapsed() < BUDGET, \"took {:?}\", start.elapsed());",
];

/// The `CpuMeter` form of a budget body.
const METER_LINES: [&str; 4] = [
    "let mut meter = CpuMeter::new();",
    "let out = meter.measure(|| run());",
    "let verdict = meter.judge(BUDGET);",
    "assert!(verdict.is_within(), \"{verdict}\");",
];

/// An arm-order body in the accepted order: the second arm, then the release.
const ARM_BEFORE_RELEASE_LINES: [&str; 5] = [
    "let (arrived_rx, release_tx) = output_capture.p2.arm();",
    "reader_reaches_the_first_stop();",
    "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
    "release_tx.send(()).unwrap();",
    "release_tx_2.send(()).unwrap();",
];

/// An arm-order body in the old order: the release, then the second arm.
const RELEASE_BEFORE_ARM_LINES: [&str; 5] = [
    "let (arrived_rx, release_tx) = output_capture.p2.arm();",
    "reader_reaches_the_first_stop();",
    "release_tx.send(()).unwrap();",
    "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
    "release_tx_2.send(()).unwrap();",
];

fn budget_verdict_of(lines: &[&str]) -> Verdict {
    judge_budget_test(&function("target", lines), "target")
}

fn arm_order_verdict_of(lines: &[&str]) -> Verdict {
    judge_arm_order_test(&function("target", lines), "target")
}

/// The reason of a rejected verdict; panics on any other verdict.
#[track_caller]
fn reason_of(verdict: Verdict) -> String {
    match verdict {
        Verdict::Rejected(reason) => reason,
        other => panic!("expected a rejected verdict, got {other:?}"),
    }
}

// AC-4

#[test]
fn the_budget_judgment_rejects_a_wall_clock_body_without_a_meter() {
    let reason = reason_of(budget_verdict_of(&WALL_CLOCK_LINES));
    assert!(reason.contains("Instant::now"), "{reason}");
}

#[test]
fn the_budget_judgment_rejects_a_wall_clock_check_that_holds_only_the_elapsed_token() {
    let mut lines = METER_LINES.to_vec();
    lines.push("assert!(start.elapsed() < BUDGET);");
    let reason = reason_of(budget_verdict_of(&lines));
    assert!(reason.contains(".elapsed()"), "{reason}");
}

#[test]
fn the_budget_judgment_rejects_a_meter_body_that_keeps_the_wall_clock_start() {
    let mut lines = METER_LINES.to_vec();
    lines.insert(0, "let start = std::time::Instant::now();");
    let reason = reason_of(budget_verdict_of(&lines));
    assert!(reason.contains("Instant::now"), "{reason}");
}

#[test]
fn the_budget_judgment_rejects_a_meter_body_without_a_measure_call() {
    let lines = [
        "let mut meter = CpuMeter::new();",
        "let out = run();",
        "let verdict = meter.judge(BUDGET);",
    ];
    let reason = reason_of(budget_verdict_of(&lines));
    assert!(reason.contains(".measure("), "{reason}");
}

#[test]
fn the_budget_judgment_rejects_a_body_that_lacks_any_one_meter_token() {
    for token in REQUIRED_BUDGET_TOKENS {
        let lines: Vec<&str> = METER_LINES
            .iter()
            .copied()
            .filter(|line| !line.contains(token))
            .collect();
        let reason = reason_of(budget_verdict_of(&lines));
        assert!(reason.contains(token), "without {token}: {reason}");
    }
}

#[test]
fn the_budget_judgment_accepts_a_body_with_all_three_meter_tokens_and_no_wall_clock_token() {
    assert_eq!(budget_verdict_of(&METER_LINES), Verdict::Accepted);
}

#[test]
fn the_budget_judgment_reports_a_source_without_the_named_function_as_missing_by_name() {
    let source = source_of(&[function("another", &METER_LINES)]);
    let verdict = judge_budget_test(&source, "target");
    assert_eq!(verdict, Verdict::Missing("target".to_string()));
    assert!(verdict.to_string().contains("`target`"), "{verdict}");
}

// AC-5

#[test]
fn the_arm_order_judgment_rejects_a_release_before_the_second_arm() {
    let reason = reason_of(arm_order_verdict_of(&RELEASE_BEFORE_ARM_LINES));
    assert!(reason.contains("before the second"), "{reason}");
}

#[test]
fn the_arm_order_judgment_rejects_a_body_with_one_arm() {
    let lines = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "release_tx.send(()).unwrap();",
    ];
    let reason = reason_of(arm_order_verdict_of(&lines));
    assert!(reason.contains("fewer than two"), "{reason}");
}

#[test]
fn the_arm_order_judgment_rejects_a_body_with_no_release() {
    let lines = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
        "drop(release_tx);",
    ];
    let reason = reason_of(arm_order_verdict_of(&lines));
    assert!(reason.contains("release_tx.send("), "{reason}");
}

#[test]
fn the_arm_order_judgment_accepts_the_second_arm_before_the_first_release() {
    assert_eq!(
        arm_order_verdict_of(&ARM_BEFORE_RELEASE_LINES),
        Verdict::Accepted
    );
}

#[test]
fn the_arm_order_judgment_reports_a_source_without_the_named_function_as_missing_by_name() {
    let source = source_of(&[function("another", &ARM_BEFORE_RELEASE_LINES)]);
    let verdict = judge_arm_order_test(&source, "target");
    assert_eq!(verdict, Verdict::Missing("target".to_string()));
    assert!(verdict.to_string().contains("`target`"), "{verdict}");
}

#[test]
fn the_arm_order_judgment_does_not_take_the_second_release_for_the_first() {
    // Only `release_tx_2.send(` comes before the second arm: the first release is
    // still after it.
    let accepted = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "release_tx_2.send(()).unwrap();",
        "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
        "release_tx.send(()).unwrap();",
    ];
    assert_eq!(arm_order_verdict_of(&accepted), Verdict::Accepted);
    // Without any `release_tx.send(` the second release does not stand in for it.
    let rejected = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
        "release_tx_2.send(()).unwrap();",
    ];
    let reason = reason_of(arm_order_verdict_of(&rejected));
    assert!(reason.contains("release_tx.send("), "{reason}");
}

// AC-6

#[test]
fn a_forbidden_token_only_in_the_following_function_does_not_reject_the_named_budget_function() {
    let source = source_of(&[
        function("target", &METER_LINES),
        function("next", &WALL_CLOCK_LINES),
    ]);
    assert_eq!(judge_budget_test(&source, "target"), Verdict::Accepted);
}

#[test]
fn a_required_token_only_in_the_following_function_does_not_satisfy_the_named_budget_function() {
    let source = source_of(&[
        function("target", &["let out = run();"]),
        function("next", &METER_LINES),
    ]);
    let reason = reason_of(judge_budget_test(&source, "target"));
    assert!(reason.contains("CpuMeter::new"), "{reason}");
}

#[test]
fn a_release_only_in_the_following_function_neither_rejects_nor_satisfies_the_named_arm_order_function()
 {
    let accepted = source_of(&[
        function("target", &ARM_BEFORE_RELEASE_LINES),
        function("next", &RELEASE_BEFORE_ARM_LINES),
    ]);
    assert_eq!(judge_arm_order_test(&accepted, "target"), Verdict::Accepted);

    let no_release = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
    ];
    let rejected = source_of(&[
        function("target", &no_release),
        function("next", &ARM_BEFORE_RELEASE_LINES),
    ]);
    let reason = reason_of(judge_arm_order_test(&rejected, "target"));
    assert!(reason.contains("release_tx.send("), "{reason}");
}

#[test]
fn a_second_arm_only_in_the_following_function_does_not_satisfy_the_named_arm_order_function() {
    let one_arm = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "release_tx.send(()).unwrap();",
    ];
    let source = source_of(&[
        function("target", &one_arm),
        function("next", &ARM_BEFORE_RELEASE_LINES),
    ]);
    let reason = reason_of(judge_arm_order_test(&source, "target"));
    assert!(reason.contains("fewer than two"), "{reason}");
}

#[test]
fn a_forbidden_token_on_a_whole_line_comment_does_not_reject_a_budget_body() {
    let mut lines = METER_LINES.to_vec();
    lines.insert(0, "// the old form: let start = Instant::now();");
    lines.push("    /// and then start.elapsed() was compared");
    assert_eq!(budget_verdict_of(&lines), Verdict::Accepted);
}

#[test]
fn a_required_token_on_a_whole_line_comment_does_not_satisfy_a_budget_body() {
    let lines = [
        "let out = run();",
        "// let mut meter = CpuMeter::new(); meter.measure(|| run()); meter.judge(BUDGET)",
    ];
    let reason = reason_of(budget_verdict_of(&lines));
    assert!(reason.contains("CpuMeter::new"), "{reason}");
}

#[test]
fn an_arm_order_token_on_a_whole_line_comment_neither_rejects_nor_satisfies_an_arm_order_body() {
    // A comment that mentions the old order in words does not reject.
    let mut described = ARM_BEFORE_RELEASE_LINES.to_vec();
    described.insert(
        3,
        "// before: release_tx.send(()) came first, then p2.arm()",
    );
    assert_eq!(arm_order_verdict_of(&described), Verdict::Accepted);

    // A commented second arm is not a second arm.
    let commented_arm = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "// let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
        "release_tx.send(()).unwrap();",
    ];
    let reason = reason_of(arm_order_verdict_of(&commented_arm));
    assert!(reason.contains("fewer than two"), "{reason}");

    // A commented release is not the first release.
    let commented_release = [
        "let (arrived_rx, release_tx) = output_capture.p2.arm();",
        "// release_tx.send(()).unwrap();",
        "let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();",
    ];
    let reason = reason_of(arm_order_verdict_of(&commented_release));
    assert!(reason.contains("release_tx.send("), "{reason}");
}

#[test]
fn a_name_that_is_a_prefix_of_another_name_is_not_matched_to_that_function() {
    // Only `target_extended` exists: `target` is missing, not matched to it.
    let source = source_of(&[function("target_extended", &METER_LINES)]);
    assert_eq!(
        judge_budget_test(&source, "target"),
        Verdict::Missing("target".to_string())
    );
    assert_eq!(
        judge_arm_order_test(
            &source_of(&[function("target_extended", &ARM_BEFORE_RELEASE_LINES)]),
            "target"
        ),
        Verdict::Missing("target".to_string())
    );

    // The longer name comes first: the body judged is still the exact name's.
    let source = source_of(&[
        function("target_extended", &WALL_CLOCK_LINES),
        function("target", &METER_LINES),
    ]);
    assert_eq!(judge_budget_test(&source, "target"), Verdict::Accepted);
    let source = source_of(&[
        function("target_extended", &METER_LINES),
        function("target", &WALL_CLOCK_LINES),
    ]);
    assert!(matches!(
        judge_budget_test(&source, "target"),
        Verdict::Rejected(_)
    ));
}

#[test]
fn a_source_with_crlf_line_ends_gets_the_verdict_of_the_same_source_with_lf_line_ends() {
    let budget_samples = [
        source_of(&[
            function("target", &METER_LINES),
            function("next", &WALL_CLOCK_LINES),
        ]),
        source_of(&[
            function("target", &WALL_CLOCK_LINES),
            function("next", &METER_LINES),
        ]),
        source_of(&[function("other", &METER_LINES)]),
    ];
    for lf in budget_samples {
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(
            judge_budget_test(&crlf, "target"),
            judge_budget_test(&lf, "target"),
            "budget judgment of {lf:?}"
        );
    }
    let arm_samples = [
        source_of(&[
            function("target", &ARM_BEFORE_RELEASE_LINES),
            function("next", &RELEASE_BEFORE_ARM_LINES),
        ]),
        source_of(&[function("target", &RELEASE_BEFORE_ARM_LINES)]),
        source_of(&[function("other", &ARM_BEFORE_RELEASE_LINES)]),
    ];
    for lf in arm_samples {
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(
            judge_arm_order_test(&crlf, "target"),
            judge_arm_order_test(&lf, "target"),
            "arm-order judgment of {lf:?}"
        );
    }
    // The accepted samples really are accepted in both forms.
    let accepted = source_of(&[function("target", &METER_LINES)]);
    assert_eq!(
        judge_budget_test(&accepted.replace('\n', "\r\n"), "target"),
        Verdict::Accepted
    );
}

#[test]
fn the_body_finder_ends_at_the_closing_brace_of_the_named_function() {
    let source = source_of(&[
        function("before", &["let earlier = 1;"]),
        function(
            "target",
            &["let inside = 2;", "if inside > 1 {", "    run();", "}"],
        ),
        function("after", &["let later = 3;"]),
    ]);
    let body = function_body(&source, "target").expect("the function is found");
    assert!(body.starts_with("fn target() {"), "{body}");
    assert!(body.contains("let inside = 2;"), "{body}");
    assert!(!body.contains("earlier"), "{body}");
    assert!(!body.contains("later"), "{body}");
    assert!(!body.contains("fn after"), "{body}");
    assert!(body.ends_with("\n}"), "{body}");
}

#[test]
fn the_body_finder_ends_a_nested_function_at_the_brace_of_its_own_indentation() {
    let source = "mod inner {\n    fn target() {\n        let a = 1;\n    }\n\n    fn after() {\n        let b = 2;\n    }\n}\n";
    let body = function_body(source, "target").expect("the function is found");
    assert!(body.contains("let a = 1;"), "{body}");
    assert!(!body.contains("let b = 2;"), "{body}");
}

#[test]
fn the_body_finder_reports_a_function_without_a_closing_line_as_missing() {
    let source = "fn target() {\n    let a = 1;\n";
    assert_eq!(function_body(source, "target"), None);
}

#[test]
fn the_body_finder_does_not_match_a_function_named_only_in_a_comment() {
    let source = format!(
        "// fn target( is described here\n{}",
        function("other", &["run();"])
    );
    assert_eq!(function_body(&source, "target"), None);
}
