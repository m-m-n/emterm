//! Unit tests for the Program Status core. Test names carry the
//! Acceptance Criterion they prove (`ac1_` .. `ac8_`).

use super::*;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

// ── Helpers ───────────────────────────────────────────────────────────

fn b64(text: &str) -> String {
    STANDARD.encode(text)
}

fn parse_bel(body: &str) -> Parsed {
    parse(body, Terminator::Bel)
}

/// Parse with BEL and expect a Set report.
fn set_of(body: &str) -> (String, Record) {
    match parse_bel(body) {
        Parsed::Report(Report::Set { id, record }) => (id, record),
        other => panic!("expected a Set report for {body:?}, got {other:?}"),
    }
}

fn record_of(body: &str) -> Record {
    set_of(body).1
}

fn is_ignored(body: &str) -> bool {
    parse_bel(body) == Parsed::Ignored && parse(body, Terminator::St) == Parsed::Ignored
}

/// Parse with BEL and apply the report when there is one.
fn feed(table: &mut Table, body: &str) -> Parsed {
    let parsed = parse_bel(body);
    if let Parsed::Report(report) = &parsed {
        table.apply(report.clone());
    }
    parsed
}

fn table_of(bodies: &[&str]) -> Table {
    let mut table = Table::default();
    for body in bodies {
        feed(&mut table, body);
    }
    table
}

/// Ids, least recently updated first.
fn ids(table: &Table) -> Vec<String> {
    table.iter().map(|(id, _)| id.to_owned()).collect()
}

fn blank(state: ProgramState) -> Record {
    Record {
        state,
        kind: None,
        progress: None,
        app: None,
        title: None,
        msg: None,
    }
}

fn exported(id: &str, state: &str) -> ExportedRecord {
    ExportedRecord {
        id: id.to_owned(),
        state: state.to_owned(),
        kind: None,
        progress: None,
        app: None,
        title: None,
        msg: None,
    }
}

/// A body whose whole sequence (7 introducer bytes + body + terminator) is
/// exactly `total` bytes long.
fn body_of_sequence_len(total: usize, terminator: Terminator) -> String {
    let head = "state=idle:app=pad:z=";
    let body_len = total - 7 - terminator.len();
    format!("{head}{}", "z".repeat(body_len - head.len()))
}

/// Ascending by composition rank, written out independently of `rank()`.
const ASCENDING: [ProgramState; 5] = [
    ProgramState::Idle,
    ProgramState::Done,
    ProgramState::Error,
    ProgramState::Working,
    ProgramState::Blocked,
];

fn position(state: ProgramState) -> usize {
    ASCENDING.iter().position(|&s| s == state).unwrap()
}

// ── Vocabulary ────────────────────────────────────────────────────────

#[test]
fn ac1_state_words_round_trip_and_clear_is_not_a_state() {
    for state in ProgramState::ALL {
        assert_eq!(ProgramState::from_word(state.word()), Some(state));
    }
    assert_eq!(ProgramState::from_word("clear"), None);
    assert_eq!(ProgramState::from_word("Idle"), None);
    assert_eq!(ProgramState::from_word(""), None);
    assert_eq!(ProgramState::Idle.word(), "idle");
    assert_eq!(ProgramState::Working.word(), "working");
    assert_eq!(ProgramState::Done.word(), "done");
    assert_eq!(ProgramState::Blocked.word(), "blocked");
    assert_eq!(ProgramState::Error.word(), "error");
}

#[test]
fn ac5_rank_is_blocked_working_error_done_idle() {
    for pair in ASCENDING.windows(2) {
        assert!(
            pair[0].rank() < pair[1].rank(),
            "{:?} must rank below {:?}",
            pair[0],
            pair[1]
        );
    }
}

// ── AC-1: parsing ─────────────────────────────────────────────────────

#[test]
fn ac1_each_of_the_six_state_words_is_recognized() {
    for state in ProgramState::ALL {
        let (id, record) = set_of(&format!("state={}", state.word()));
        assert_eq!(id, "");
        assert_eq!(record, blank(state));
    }
    assert_eq!(
        parse_bel("state=clear"),
        Parsed::Report(Report::Clear { id: String::new() })
    );
}

#[test]
fn ac1_both_terminators_parse_the_same_body() {
    let body = "state=working:id=a/b:app=claude";
    assert_eq!(parse(body, Terminator::Bel), parse(body, Terminator::St));
    assert!(matches!(
        parse(body, Terminator::St),
        Parsed::Report(Report::Set { .. })
    ));
}

#[test]
fn ac1_malformed_pairs_are_skipped_and_the_rest_applies() {
    // no `=`, empty key, uppercase key, key with a digit, key with `_`,
    // value with a byte outside the value set (space, `%`, `;`, non-ASCII).
    let body = "junk:=x:State=done:st4te=done:my_key=1:app=a b:title=a%41:msg=a;b:\
                state=working:id=é:progress=7";
    let (id, record) = set_of(body);
    assert_eq!(id, "", "an id pair that fails the pair grammar is skipped");
    assert_eq!(record.state, ProgramState::Working);
    assert_eq!(record.progress, Some(7));
    assert_eq!(record.app, None);
    assert_eq!(record.title, None);
    assert_eq!(record.msg, None);
}

#[test]
fn ac1_a_skipped_pair_does_not_override_an_earlier_valid_one() {
    let record = record_of("state=idle:app=first:app=bad!");
    assert_eq!(record.app.as_deref(), Some("first"));
    assert_eq!(set_of("state=idle:id=a:id=b c").0, "a");
}

#[test]
fn ac1_the_pair_splits_at_its_first_equals_sign() {
    // `YQ==` is a valid value that itself contains `=`.
    let record = record_of("state=idle:title=YQ==");
    assert_eq!(record.title.as_deref(), Some("a"));
    // an `=` inside an app value keeps the pair but the app is invalid.
    assert_eq!(record_of("state=idle:app=a=b").app, None);
}

#[test]
fn ac1_unknown_keys_are_ignored() {
    assert_eq!(
        parse_bel("state=working:zzz=1:foo=bar:progress=5"),
        parse_bel("state=working:progress=5")
    );
}

#[test]
fn ac1_a_repeated_key_resolves_to_its_last_valid_occurrence() {
    assert_eq!(
        record_of("state=idle:state=working").state,
        ProgramState::Working
    );
    assert_eq!(
        record_of("state=working:state=idle").state,
        ProgramState::Idle
    );
    assert_eq!(
        record_of("state=idle:app=one:app=two").app.as_deref(),
        Some("two")
    );
    assert_eq!(set_of("state=idle:id=a:id=b").0, "b");
    // the last survivor counts even when it is the invalid one.
    assert!(is_ignored("state=idle:state=bogus"));
    assert!(is_ignored("state=idle:id=a:id="));
    assert_eq!(
        record_of("state=working:progress=10:progress=20").progress,
        Some(20)
    );
    assert_eq!(
        record_of("state=working:title=YQ==:title=Yg==")
            .title
            .as_deref(),
        Some("b")
    );
}

#[test]
fn ac1_kind_is_kept_only_for_blocked() {
    for state in ProgramState::ALL {
        for kind in [Kind::Permission, Kind::Question, Kind::Auth] {
            let record = record_of(&format!("state={}:kind={}", state.word(), kind.word()));
            let expected = (state == ProgramState::Blocked).then_some(kind);
            assert_eq!(record.kind, expected, "state={state:?} kind={kind:?}");
        }
    }
}

#[test]
fn ac1_an_unknown_kind_leaves_it_absent_without_rejecting_the_report() {
    for value in ["bogus", "Permission", "", "perm"] {
        let record = record_of(&format!("state=blocked:kind={value}"));
        assert_eq!(record.state, ProgramState::Blocked);
        assert_eq!(record.kind, None, "kind={value:?}");
    }
}

#[test]
fn ac1_progress_is_kept_only_for_working_and_blocked() {
    for state in ProgramState::ALL {
        let record = record_of(&format!("state={}:progress=42", state.word()));
        let expected = matches!(state, ProgramState::Working | ProgramState::Blocked);
        assert_eq!(record.progress, expected.then_some(42), "state={state:?}");
    }
}

#[test]
fn ac1_progress_is_an_integer_from_0_to_100() {
    for (value, expected) in [
        ("0", 0),
        ("100", 100),
        ("7", 7),
        ("007", 7),
        ("000100", 100),
    ] {
        let record = record_of(&format!("state=working:progress={value}"));
        assert_eq!(record.progress, Some(expected), "progress={value}");
    }
    for value in [
        "101",
        "1000",
        "-1",
        "5.5",
        "abc",
        "",
        "+5",
        "1e1",
        "99999999999999999999",
        "0x10",
    ] {
        let record = record_of(&format!("state=working:progress={value}"));
        assert_eq!(record.progress, None, "progress={value:?} skips the pair");
    }
}

#[test]
fn ac1_an_invalid_progress_pair_does_not_override_an_earlier_valid_one() {
    let record = record_of("state=working:progress=50:progress=101");
    assert_eq!(record.progress, Some(50));
}

#[test]
fn ac1_app_must_match_the_name_pattern_otherwise_it_is_absent() {
    let longest = "a".repeat(32);
    assert_eq!(
        record_of(&format!("state=idle:app={longest}"))
            .app
            .as_deref(),
        Some(longest.as_str())
    );
    assert_eq!(
        record_of("state=idle:app=Claude_Code-1.2+x").app.as_deref(),
        Some("Claude_Code-1.2+x")
    );
    for value in [
        "a".repeat(33),
        String::new(),
        "a,b".to_owned(),
        "a/b".to_owned(),
        "a=b".to_owned(),
    ] {
        let record = record_of(&format!("state=idle:app={value}"));
        assert_eq!(record.app, None, "app={value:?}");
        assert_eq!(
            record.state,
            ProgramState::Idle,
            "the report itself stays valid"
        );
    }
}

#[test]
fn ac1_only_the_body_question_mark_is_a_query() {
    assert_eq!(parse("?", Terminator::Bel), Parsed::Query);
    assert_eq!(parse("?", Terminator::St), Parsed::Query);
    assert_eq!(parse_bytes(b"?", Terminator::Bel), Parsed::Query);
    for body in [
        "",
        "??",
        "? ",
        " ?",
        "?=",
        "?x",
        "state=idle:?",
        "state=?",
        "\\?",
    ] {
        assert_ne!(parse_bel(body), Parsed::Query, "body {body:?}");
        assert_ne!(parse(body, Terminator::St), Parsed::Query, "body {body:?}");
    }
}

#[test]
fn ac1_a_query_is_not_a_report() {
    // `?` neither carries state nor reaches the table.
    let mut table = table_of(&["state=idle"]);
    let before = table.clone();
    assert_eq!(feed(&mut table, "?"), Parsed::Query);
    assert_eq!(table, before);
}

#[test]
fn ac1_parse_bytes_agrees_with_parse() {
    let body = "state=blocked:kind=auth:progress=3:id=a/b:app=x";
    assert_eq!(
        parse_bytes(body.as_bytes(), Terminator::St),
        parse(body, Terminator::St)
    );
    assert_eq!(
        parse_bytes(b"state=idle:app=\xff\xfe", Terminator::Bel),
        parse_bel("state=idle")
    );
}

// ── AC-2: discard and ignore ──────────────────────────────────────────

#[test]
fn ac2_a_sequence_of_exactly_4096_bytes_is_accepted_and_4097_is_discarded() {
    for terminator in [Terminator::Bel, Terminator::St] {
        let at_limit = body_of_sequence_len(4096, terminator);
        assert_eq!(7 + at_limit.len() + terminator.len(), 4096);
        match parse(&at_limit, terminator) {
            Parsed::Report(Report::Set { record, .. }) => {
                assert_eq!(record.state, ProgramState::Idle);
                assert_eq!(record.app.as_deref(), Some("pad"));
            }
            other => panic!("{terminator:?}: 4096 bytes must be accepted, got {other:?}"),
        }

        let over = body_of_sequence_len(4097, terminator);
        assert_eq!(7 + over.len() + terminator.len(), 4097);
        assert_eq!(
            parse(&over, terminator),
            Parsed::Ignored,
            "{terminator:?}: 4097 bytes must be discarded"
        );
    }
}

#[test]
fn ac2_the_terminator_counts_toward_the_sequence_length() {
    // The same body is accepted with BEL but over the limit with ST.
    let body = body_of_sequence_len(4096, Terminator::Bel);
    assert!(matches!(parse(&body, Terminator::Bel), Parsed::Report(_)));
    assert_eq!(parse(&body, Terminator::St), Parsed::Ignored);
}

#[test]
fn ac2_title_is_accepted_up_to_256_encoded_and_192_decoded_bytes() {
    let longest = "a".repeat(192);
    let encoded = b64(&longest);
    assert_eq!(encoded.len(), 256);
    let record = record_of(&format!("state=idle:title={encoded}"));
    assert_eq!(record.title.as_deref(), Some(longest.as_str()));
    // the unpadded form of a shorter title is accepted too.
    let record = record_of(&format!(
        "state=idle:title={}",
        STANDARD_NO_PAD.encode("ab")
    ));
    assert_eq!(record.title.as_deref(), Some("ab"));
}

#[test]
fn ac2_title_above_the_encoded_or_decoded_cap_discards_the_report() {
    let over = "a".repeat(193);
    for encoded in [b64(&over), STANDARD_NO_PAD.encode(&over), "A".repeat(257)] {
        assert!(
            is_ignored(&format!("state=idle:title={encoded}")),
            "{} encoded bytes",
            encoded.len()
        );
    }
}

#[test]
fn ac2_msg_is_accepted_up_to_2732_encoded_and_2048_decoded_bytes() {
    let longest = "a".repeat(2048);
    let padded = b64(&longest);
    assert_eq!(padded.len(), 2732);
    let unpadded = STANDARD_NO_PAD.encode(&longest);
    assert_eq!(unpadded.len(), 2731);
    for encoded in [padded, unpadded] {
        let record = record_of(&format!("state=idle:msg={encoded}"));
        assert_eq!(record.msg.as_deref(), Some(longest.as_str()));
    }
}

#[test]
fn ac2_msg_above_the_encoded_or_decoded_cap_discards_the_report() {
    // 2049 bytes encode to exactly 2732 characters: within the encoded cap,
    // over the decoded cap.
    let decoded_over = "a".repeat(2049);
    let encoded = b64(&decoded_over);
    assert_eq!(encoded.len(), 2732);
    assert!(is_ignored(&format!("state=idle:msg={encoded}")));

    // 2050 bytes encode to 2736 characters: over both caps.
    let both_over = "a".repeat(2050);
    for encoded in [b64(&both_over), STANDARD_NO_PAD.encode(&both_over)] {
        assert!(is_ignored(&format!("state=idle:msg={encoded}")));
    }
}

#[test]
fn ac2_invalid_base64_discards_the_report() {
    for value in [
        "ab-d", "ab_d", "ab.d", "ab,d", "A", "AAAAA", "YQ=a", "=", "====", "Y=Q=",
    ] {
        assert!(
            is_ignored(&format!("state=idle:title={value}")),
            "title={value}"
        );
        assert!(
            is_ignored(&format!("state=idle:msg={value}")),
            "msg={value}"
        );
    }
}

#[test]
fn ac2_base64_is_accepted_padded_and_unpadded() {
    for value in ["YQ==", "YQ", "YWI=", "YWI", "YWJj"] {
        let record = record_of(&format!("state=idle:title={value}"));
        assert!(record.title.is_some(), "title={value}");
    }
}

#[test]
fn ac2_a_decoded_value_that_is_not_utf8_discards_the_report() {
    for bytes in [
        vec![0xffu8, 0xfe, 0xfd],
        vec![0xe3, 0x81],
        vec![b'a', 0xc0, 0x80],
        vec![0xed, 0xa0, 0x80],
    ] {
        let encoded = STANDARD.encode(&bytes);
        assert!(
            is_ignored(&format!("state=idle:title={encoded}")),
            "{bytes:?}"
        );
        assert!(
            is_ignored(&format!("state=idle:msg={encoded}")),
            "{bytes:?}"
        );
    }
}

#[test]
fn ac2_a_control_character_in_a_decoded_value_discards_the_report() {
    for c in [
        '\u{0}', '\t', '\n', '\r', '\u{1b}', '\u{1f}', '\u{7f}', '\u{80}', '\u{85}', '\u{9f}',
    ] {
        let encoded = b64(&format!("ok{c}ok"));
        assert!(
            is_ignored(&format!("state=idle:title={encoded}")),
            "title U+{:04X}",
            c as u32
        );
        assert!(
            is_ignored(&format!("state=idle:msg={encoded}")),
            "msg U+{:04X}",
            c as u32
        );
    }
}

#[test]
fn ac2_non_control_characters_are_kept_verbatim_by_the_parser() {
    // Sanitization is for names only; the parser keeps the decoded text.
    for text in [
        "plain",
        "with space",
        "\u{a0}nbsp",
        "\u{a1}",
        "日本語",
        "😀",
        "zero\u{200b}width",
        "line\u{2028}sep",
    ] {
        let encoded = b64(text);
        let record = record_of(&format!("state=idle:title={encoded}:msg={encoded}"));
        assert_eq!(record.title.as_deref(), Some(text));
        assert_eq!(record.msg.as_deref(), Some(text));
    }
}

#[test]
fn ac2_an_empty_decoded_title_or_msg_counts_as_absent() {
    for body in ["state=idle:title=:msg=", "state=idle"] {
        let record = record_of(body);
        assert_eq!(record.title, None, "{body}");
        assert_eq!(record.msg, None, "{body}");
    }
}

#[test]
fn ac2_title_and_msg_rules_apply_to_a_clear_report_as_well() {
    assert!(is_ignored("state=clear:title=ab-d"));
    assert!(is_ignored(&format!("state=clear:msg={}", b64("a\nb"))));
    assert!(matches!(
        parse_bel("state=clear:title=YQ=="),
        Parsed::Report(Report::Clear { .. })
    ));
}

#[test]
fn ac2_a_missing_or_unknown_state_is_ignored() {
    for body in [
        "",
        "app=x",
        "id=a",
        "state=",
        "state=bogus",
        "state=IDLE",
        "state=Clear",
        "state=idle,working",
        "title=YQ==",
    ] {
        assert!(is_ignored(body), "body {body:?}");
    }
}

#[test]
fn ac2_a_valid_id_is_accepted() {
    let seg32 = "s".repeat(32);
    // 4 segments of 32, 32, 32 and 29 bytes plus 3 separators = 128 bytes.
    let total_128 = format!("{seg32}/{seg32}/{seg32}/{}", "t".repeat(29));
    assert_eq!(total_128.len(), 128);
    for id in [
        "a",
        "a/b",
        "A-z_0.9+x",
        "a/b/c/d/e/f/g/h",
        seg32.as_str(),
        total_128.as_str(),
        "..",
    ] {
        assert_eq!(set_of(&format!("state=idle:id={id}")).0, id);
    }
}

#[test]
fn ac2_an_invalid_id_is_ignored() {
    let seg33 = "s".repeat(33);
    // 4 segments of 32, 32, 32 and 30 bytes plus 3 separators = 129 bytes.
    let total_129 = format!("{0}/{0}/{0}/{1}", "s".repeat(32), "t".repeat(30));
    assert_eq!(total_129.len(), 129);
    for id in [
        "",
        "/",
        "//",
        "/a",
        "a/",
        "a//b",
        "a,b",
        "a=b",
        "a/b,c",
        seg33.as_str(),
        "a/b/c/d/e/f/g/h/i",
        total_129.as_str(),
    ] {
        assert!(is_ignored(&format!("state=idle:id={id}")), "id {id:?}");
        assert!(
            is_ignored(&format!("state=clear:id={id}")),
            "clear id {id:?}"
        );
    }
}

#[test]
fn ac2_every_discard_and_ignore_case_leaves_the_table_unchanged() {
    let mut table = table_of(&["state=working:id=keep", "state=done:id=other/child"]);
    let before = table.clone();
    let over_length = body_of_sequence_len(4097, Terminator::Bel);
    let bodies = [
        over_length.as_str(),
        "state=idle:title=ab-d",
        "state=idle:msg=ab-d",
        "state=clear:title=ab-d",
        "state=bogus:id=keep",
        "app=x:id=keep",
        "state=clear:id=a//b",
        "state=idle:id=a//b",
        "state=clear:id=",
    ];
    for body in bodies {
        assert_eq!(feed(&mut table, body), Parsed::Ignored, "{body:.40}");
        assert_eq!(table, before, "{body:.40}");
    }
}

// ── AC-3: table mutations ─────────────────────────────────────────────

#[test]
fn ac3_a_report_fully_replaces_its_id_record() {
    let full = format!(
        "state=blocked:id=x:kind=auth:progress=10:app=ap:title={}:msg={}",
        b64("t"),
        b64("m")
    );
    let mut table = table_of(&[full.as_str()]);
    assert_eq!(
        table.get("x"),
        Some(&Record {
            state: ProgramState::Blocked,
            kind: Some(Kind::Auth),
            progress: Some(10),
            app: Some("ap".to_owned()),
            title: Some("t".to_owned()),
            msg: Some("m".to_owned()),
        })
    );

    feed(&mut table, "state=working:id=x");
    assert_eq!(table.len(), 1);
    assert_eq!(table.get("x"), Some(&blank(ProgramState::Working)));
}

#[test]
fn ac3_the_root_and_each_id_are_separate_records() {
    let table = table_of(&["state=idle", "state=working:id=a", "state=done:id=a/b"]);
    assert_eq!(table.len(), 3);
    assert_eq!(table.get("").map(|r| r.state), Some(ProgramState::Idle));
    assert_eq!(table.get("a").map(|r| r.state), Some(ProgramState::Working));
    assert_eq!(table.get("a/b").map(|r| r.state), Some(ProgramState::Done));
}

#[test]
fn ac3_clear_with_an_id_removes_it_and_its_descendants_on_segment_boundaries() {
    let mut table = table_of(&[
        "state=idle",
        "state=idle:id=a",
        "state=idle:id=a/b",
        "state=idle:id=a/b/c",
        "state=idle:id=ab",
        "state=idle:id=a.b",
        "state=idle:id=a-b",
        "state=idle:id=b/a",
    ]);
    feed(&mut table, "state=clear:id=a");
    assert_eq!(ids(&table), ["", "ab", "a.b", "a-b", "b/a"]);

    // `b` has no record of its own, but `b/a` lies below it.
    feed(&mut table, "state=clear:id=b");
    assert_eq!(ids(&table), ["", "ab", "a.b", "a-b"]);

    // A longer name that merely starts with the cleared id is not below it.
    feed(&mut table, "state=clear:id=a.");
    assert_eq!(ids(&table), ["", "ab", "a.b", "a-b"]);
}

#[test]
fn ac3_clear_of_a_nested_id_keeps_its_ancestors() {
    let mut table = table_of(&[
        "state=idle:id=a",
        "state=idle:id=a/b",
        "state=idle:id=a/b/c",
        "state=idle:id=a/bc",
    ]);
    feed(&mut table, "state=clear:id=a/b");
    assert_eq!(ids(&table), ["a", "a/bc"]);
}

#[test]
fn ac3_clear_without_an_id_removes_every_record_including_the_root() {
    let mut table = table_of(&["state=idle", "state=working:id=a", "state=done:id=a/b"]);
    let parsed = feed(&mut table, "state=clear");
    assert_eq!(parsed, Parsed::Report(Report::Clear { id: String::new() }));
    assert!(table.is_empty());
}

#[test]
fn ac3_a_clear_that_removes_nothing_is_still_an_accepted_report() {
    let mut table = table_of(&["state=idle:id=keep"]);
    let before = table.clone();
    let parsed = feed(&mut table, "state=clear:id=missing");
    assert_eq!(
        parsed,
        Parsed::Report(Report::Clear {
            id: "missing".to_owned()
        })
    );
    assert_eq!(table, before);

    let mut empty = Table::default();
    assert!(matches!(feed(&mut empty, "state=clear"), Parsed::Report(_)));
    assert!(empty.is_empty());
}

fn full_table() -> Table {
    let mut table = Table::default();
    for i in 0..MAX_RECORDS {
        feed(&mut table, &format!("state=idle:id=n{i}"));
    }
    assert_eq!(table.len(), MAX_RECORDS);
    table
}

#[test]
fn ac3_the_257th_distinct_id_evicts_the_least_recently_updated_record() {
    let mut table = full_table();
    feed(&mut table, "state=working:id=extra");
    assert_eq!(table.len(), MAX_RECORDS);
    assert!(table.get("n0").is_none(), "the oldest record is evicted");
    assert!(table.get("n1").is_some());
    assert!(table.get("n255").is_some());
    assert_eq!(
        table.get("extra").map(|r| r.state),
        Some(ProgramState::Working)
    );

    feed(&mut table, "state=working:id=extra2");
    assert!(table.get("n1").is_none(), "the next oldest goes next");
    assert_eq!(table.len(), MAX_RECORDS);
}

#[test]
fn ac3_re_reporting_an_existing_id_refreshes_recency_and_evicts_nothing() {
    let mut table = full_table();
    // Same data as before: the report still counts and refreshes recency.
    feed(&mut table, "state=idle:id=n0");
    assert_eq!(table.len(), MAX_RECORDS);
    for i in 0..MAX_RECORDS {
        assert!(table.get(&format!("n{i}")).is_some(), "n{i} must remain");
    }

    feed(&mut table, "state=idle:id=extra");
    assert!(
        table.get("n0").is_some(),
        "n0 was refreshed, so it survives"
    );
    assert!(
        table.get("n1").is_none(),
        "n1 is now the least recently updated"
    );
    assert_eq!(table.len(), MAX_RECORDS);
}

#[test]
fn ac3_the_cap_applies_to_the_root_record_like_any_other() {
    let mut table = full_table();
    feed(&mut table, "state=done");
    assert!(table.get("n0").is_none());
    assert_eq!(table.get("").map(|r| r.state), Some(ProgramState::Done));
}

#[test]
fn ac3_apply_orders_records_by_last_update() {
    let mut table = table_of(&["state=idle:id=a", "state=idle:id=b", "state=idle:id=c"]);
    feed(&mut table, "state=working:id=a");
    assert_eq!(ids(&table), ["b", "c", "a"]);
}

// ── AC-4: lifecycle ───────────────────────────────────────────────────

fn lifecycle_table() -> Table {
    table_of(&[
        "state=working:id=w:progress=5",
        "state=done:id=d:app=dapp",
        "state=blocked:id=b:kind=question",
        "state=error:id=e:app=eapp",
        "state=idle:id=i",
        "state=error",
        "state=working:id=w2",
    ])
}

#[test]
fn ac4_prompt_start_removes_working_blocked_idle_and_keeps_done_and_error() {
    let mut table = lifecycle_table();
    let before = table.export();
    assert!(table.prompt_start());

    let expected: Vec<ExportedRecord> = before
        .into_iter()
        .filter(|r| r.state == "done" || r.state == "error")
        .collect();
    assert_eq!(expected.len(), 3);
    assert_eq!(
        table.export(),
        expected,
        "survivors keep their fields and relative update order"
    );
    assert_eq!(ids(&table), ["d", "e", ""]);
}

#[test]
fn ac4_prompt_start_reports_false_when_nothing_was_removed() {
    let mut table = table_of(&["state=done:id=d", "state=error:id=e"]);
    let before = table.clone();
    assert!(!table.prompt_start());
    assert_eq!(table, before);

    let mut empty = Table::default();
    assert!(!empty.prompt_start());
    assert!(empty.is_empty());
}

#[test]
fn ac4_prompt_start_removes_each_removable_state_on_its_own() {
    for state in ["working", "blocked", "idle"] {
        let mut table = table_of(&[&format!("state={state}:id=x"), "state=done:id=y"]);
        assert!(table.prompt_start(), "{state}");
        assert_eq!(ids(&table), ["y"], "{state}");
    }
}

#[test]
fn ac4_reset_removes_every_record_and_reports_whether_any_existed() {
    let mut table = lifecycle_table();
    assert!(table.reset());
    assert!(table.is_empty());
    assert_eq!(table.summary(), None);
    assert!(!table.reset(), "nothing existed the second time");
}

#[test]
fn ac4_reset_on_an_empty_table_reports_false() {
    let mut table = Table::default();
    assert!(!table.reset());
    assert_eq!(table, Table::default());
}

#[test]
fn ac4_prompt_start_keeps_the_next_eviction_order_of_survivors() {
    let mut table = table_of(&[
        "state=done:id=old",
        "state=working:id=gone",
        "state=error:id=mid",
    ]);
    table.prompt_start();
    // Two survivors plus 254 new ids fill the table exactly.
    for i in 0..(MAX_RECORDS - 2) {
        feed(&mut table, &format!("state=idle:id=n{i}"));
    }
    assert_eq!(table.len(), MAX_RECORDS);
    assert!(table.get("old").is_some());
    feed(&mut table, "state=idle:id=extra");
    assert!(
        table.get("old").is_none(),
        "the older survivor is evicted first"
    );
    assert!(table.get("mid").is_some());
}

// ── AC-5: summary ─────────────────────────────────────────────────────

#[test]
fn ac5_an_empty_table_has_no_summary() {
    assert_eq!(Table::default().summary(), None);
}

#[test]
fn ac5_the_state_is_the_highest_by_blocked_working_error_done_idle() {
    for &a in &ASCENDING {
        for &b in &ASCENDING {
            let expected = if position(a) >= position(b) { a } else { b };
            // root and a descendant, in both insertion orders.
            let one = table_of(&[
                &format!("state={}", a.word()),
                &format!("state={}:id=x/y", b.word()),
            ]);
            let two = table_of(&[
                &format!("state={}:id=x/y", b.word()),
                &format!("state={}", a.word()),
            ]);
            assert_eq!(
                one.summary().map(|s| s.state),
                Some(expected),
                "{a:?} vs {b:?}"
            );
            assert_eq!(
                two.summary().map(|s| s.state),
                Some(expected),
                "{b:?} vs {a:?}"
            );
        }
    }
}

#[test]
fn ac5_a_done_child_does_not_mask_a_working_root_and_error_ranks_above_done() {
    let table = table_of(&["state=working", "state=done:id=child"]);
    assert_eq!(
        table.summary().map(|s| s.state),
        Some(ProgramState::Working)
    );
    let table = table_of(&["state=done", "state=error:id=child", "state=idle:id=other"]);
    assert_eq!(table.summary().map(|s| s.state), Some(ProgramState::Error));
    let table = table_of(&["state=error", "state=working:id=child"]);
    assert_eq!(
        table.summary().map(|s| s.state),
        Some(ProgramState::Working)
    );
}

#[test]
fn ac5_a_single_record_decides_alone() {
    let table = table_of(&["state=done:id=only"]);
    assert_eq!(
        table.summary(),
        Some(Summary {
            state: ProgramState::Done,
            title: None,
            app: None,
        })
    );
}

#[test]
fn ac5_title_and_app_come_from_the_deciding_record() {
    let root = format!("state=done:app=rootapp:title={}", b64("root title"));
    let low = format!("state=idle:id=low:app=lowapp:title={}", b64("low title"));
    let top = format!("state=blocked:id=top:app=topapp:title={}", b64("top title"));
    let table = table_of(&[root.as_str(), low.as_str(), top.as_str()]);
    assert_eq!(
        table.summary(),
        Some(Summary {
            state: ProgramState::Blocked,
            title: Some("top title".to_owned()),
            app: Some("topapp".to_owned()),
        })
    );
}

#[test]
fn ac5_the_deciding_records_missing_title_is_not_borrowed_from_another_record() {
    let root = format!("state=done:title={}", b64("root title"));
    let table = table_of(&[root.as_str(), "state=blocked:id=top"]);
    assert_eq!(table.summary().and_then(|s| s.title), None);
}

#[test]
fn ac5_ties_go_to_the_most_recently_updated_record() {
    let a = format!("state=working:id=a:title={}", b64("A"));
    let b = format!("state=working:id=b:title={}", b64("B"));
    let mut table = table_of(&[a.as_str(), b.as_str()]);
    assert_eq!(table.summary().and_then(|s| s.title).as_deref(), Some("B"));

    // Re-reporting a refreshes its recency, so it now wins the tie.
    feed(&mut table, &a);
    assert_eq!(table.summary().and_then(|s| s.title).as_deref(), Some("A"));
}

#[test]
fn ac5_ties_across_the_root_and_a_descendant_follow_recency_too() {
    let root = format!("state=error:title={}", b64("root"));
    let child = format!("state=error:id=c:title={}", b64("child"));
    let mut table = table_of(&[root.as_str(), child.as_str()]);
    assert_eq!(
        table.summary().and_then(|s| s.title).as_deref(),
        Some("child")
    );
    feed(&mut table, &root);
    assert_eq!(
        table.summary().and_then(|s| s.title).as_deref(),
        Some("root")
    );
}

#[test]
fn ac5_a_higher_rank_beats_recency() {
    let blocked = format!("state=blocked:id=old:title={}", b64("old blocked"));
    let working = format!("state=working:id=new:title={}", b64("new working"));
    let table = table_of(&[blocked.as_str(), working.as_str()]);
    assert_eq!(
        table.summary().and_then(|s| s.title).as_deref(),
        Some("old blocked")
    );
}

#[test]
fn ac5_a_record_with_its_own_app_keeps_it() {
    let table = table_of(&["state=idle:app=rootapp", "state=blocked:id=a:app=own"]);
    assert_eq!(table.summary().and_then(|s| s.app).as_deref(), Some("own"));
}

#[test]
fn ac5_a_record_without_app_takes_the_app_of_its_nearest_present_ancestor() {
    // `a/b` is absent, so the nearest ancestor record of `a/b/c` is `a`.
    let table = table_of(&[
        "state=idle:app=rootapp",
        "state=idle:id=a:app=aapp",
        "state=blocked:id=a/b/c",
    ]);
    assert_eq!(table.summary().and_then(|s| s.app).as_deref(), Some("aapp"));

    // `a/b` is present now and nearer.
    let table = table_of(&[
        "state=idle:app=rootapp",
        "state=idle:id=a:app=aapp",
        "state=idle:id=a/b:app=bapp",
        "state=blocked:id=a/b/c",
    ]);
    assert_eq!(table.summary().and_then(|s| s.app).as_deref(), Some("bapp"));
}

#[test]
fn ac5_the_root_counts_as_an_ancestor_of_every_id() {
    let table = table_of(&["state=idle:app=rootapp", "state=blocked:id=x"]);
    assert_eq!(
        table.summary().and_then(|s| s.app).as_deref(),
        Some("rootapp")
    );
    let table = table_of(&["state=idle:app=rootapp", "state=blocked:id=x/y/z"]);
    assert_eq!(
        table.summary().and_then(|s| s.app).as_deref(),
        Some("rootapp")
    );
}

#[test]
fn ac5_the_ancestor_is_looked_up_at_summary_time() {
    let mut table = table_of(&[
        "state=done:app=rootapp",
        "state=idle:id=a:app=aapp",
        "state=error:id=a/b",
    ]);
    assert_eq!(table.summary().and_then(|s| s.app).as_deref(), Some("aapp"));
    // The idle ancestor goes away at the next prompt; the root is next.
    assert!(table.prompt_start());
    assert_eq!(
        table.summary().and_then(|s| s.app).as_deref(),
        Some("rootapp")
    );
    // Replace the root with a record that has no app: nothing left to inherit.
    table.apply(Report::Set {
        id: String::new(),
        record: blank(ProgramState::Done),
    });
    assert_eq!(table.summary().and_then(|s| s.app), None);
}

#[test]
fn ac5_an_ancestor_without_an_app_passes_the_lookup_further_up() {
    let table = table_of(&[
        "state=idle:app=rootapp",
        "state=idle:id=a",
        "state=blocked:id=a/b",
    ]);
    assert_eq!(
        table.summary().and_then(|s| s.app).as_deref(),
        Some("rootapp")
    );
}

#[test]
fn ac5_siblings_prefixes_and_descendants_are_not_ancestors() {
    let table = table_of(&[
        "state=idle:id=ab:app=prefix",
        "state=idle:id=a/c:app=sibling",
        "state=idle:id=a/b/d:app=descendant",
        "state=blocked:id=a/b",
    ]);
    let summary = table.summary().unwrap();
    assert_eq!(summary.state, ProgramState::Blocked);
    assert_eq!(summary.app, None);
}

#[test]
fn ac5_the_title_is_sanitized_in_the_summary() {
    let table = table_of(&[&format!("state=idle:title={}", b64("a\u{200b}b\u{202e}c"))]);
    assert_eq!(
        table.summary().and_then(|s| s.title).as_deref(),
        Some("abc")
    );
}

#[test]
fn ac5_a_title_that_is_empty_after_sanitization_is_absent() {
    let table = table_of(&[&format!(
        "state=idle:title={}",
        b64("\u{200b}\u{feff}\u{202e}")
    )]);
    let summary = table.summary().unwrap();
    assert_eq!(summary.title, None);
    // The stored record still holds the decoded text.
    assert_eq!(
        table.get("").and_then(|r| r.title.as_deref()),
        Some("\u{200b}\u{feff}\u{202e}")
    );
}

#[test]
fn ac5_the_summary_title_is_cut_to_80_characters() {
    let table = table_of(&[&format!("state=idle:title={}", b64(&"x".repeat(150)))]);
    assert_eq!(table.summary().and_then(|s| s.title), Some("x".repeat(80)));
}

#[test]
fn ac5_summary_does_not_modify_the_table() {
    let table = lifecycle_table();
    let before = table.clone();
    let _ = table.summary();
    assert_eq!(table, before);
}

// ── AC-6: title sanitization for names ────────────────────────────────

#[test]
fn ac6_control_characters_are_removed() {
    let mut input = String::from("a");
    for c in ('\u{0}'..='\u{1f}').chain('\u{7f}'..='\u{9f}') {
        input.push(c);
        input.push('b');
    }
    let cleaned = sanitize_title(&input);
    assert!(cleaned.chars().all(|c| c == 'a' || c == 'b'), "{cleaned:?}");
    assert_eq!(cleaned.chars().filter(|&c| c == 'b').count(), 32 + 33);
    assert!(cleaned.starts_with('a'));
}

#[test]
fn ac6_every_listed_invisible_formatting_character_is_removed() {
    let ranges: [(u32, u32); 7] = [
        (0x00AD, 0x00AD),
        (0x061C, 0x061C),
        (0x200B, 0x200F),
        (0x2028, 0x202E),
        (0x2060, 0x2064),
        (0x2066, 0x2069),
        (0xFEFF, 0xFEFF),
    ];
    for (from, to) in ranges {
        for code in from..=to {
            let c = char::from_u32(code).unwrap();
            let input = format!("a{c}b");
            assert_eq!(sanitize_title(&input), "ab", "U+{code:04X}");
        }
    }
}

#[test]
fn ac6_ordinary_text_is_kept_as_is() {
    for text in [
        "plain title",
        "  spaced  ",
        "日本語のタイトル",
        "emoji 😀",
        "\u{a0}nbsp",
        "tab\u{2009}thin",
        "<b>bold</b> & {agent_status} %41 \\n",
        "",
    ] {
        assert_eq!(sanitize_title(text), text);
    }
}

#[test]
fn ac6_the_result_is_truncated_to_80_characters() {
    assert_eq!(sanitize_title(&"a".repeat(80)), "a".repeat(80));
    assert_eq!(sanitize_title(&"a".repeat(81)), "a".repeat(80));
    assert_eq!(sanitize_title(&"a".repeat(500)), "a".repeat(80));
}

#[test]
fn ac6_truncation_counts_characters_not_bytes() {
    assert_eq!(sanitize_title(&"é".repeat(80)), "é".repeat(80));
    assert_eq!(sanitize_title(&"日".repeat(100)), "日".repeat(80));
    assert_eq!(sanitize_title(&"😀".repeat(81)), "😀".repeat(80));
}

#[test]
fn ac6_removal_happens_before_truncation() {
    let input = "a\u{200b}".repeat(100);
    assert_eq!(sanitize_title(&input), "a".repeat(80));
}

#[test]
fn ac6_title_and_msg_are_kept_as_plain_text_and_never_interpreted() {
    let markup = "<b>bold</b> <script>alert(1)</script> &amp; {agent_status} %41 \\x1b[31m **md**";
    let body = format!("state=idle:title={}:msg={}", b64(markup), b64(markup));
    let record = record_of(&body);
    assert_eq!(record.title.as_deref(), Some(markup));
    assert_eq!(record.msg.as_deref(), Some(markup));
    // Names get the same text back, not an interpretation of it.
    let table = table_of(&[body.as_str()]);
    assert_eq!(
        table.summary().and_then(|s| s.title).as_deref(),
        Some(markup)
    );
}

// ── AC-7: export / import ─────────────────────────────────────────────

#[test]
fn ac7_export_lists_records_oldest_updated_first_with_all_fields() {
    let full = format!(
        "state=blocked:id=a/b:kind=permission:progress=42:app=claude:title={}:msg={}",
        b64("Title"),
        b64("Message")
    );
    let mut table = table_of(&["state=idle", full.as_str(), "state=done:id=z"]);
    // Touch the root so it becomes the most recently updated.
    feed(&mut table, "state=error");

    assert_eq!(
        table.export(),
        vec![
            ExportedRecord {
                id: "a/b".to_owned(),
                state: "blocked".to_owned(),
                kind: Some("permission".to_owned()),
                progress: Some(42),
                app: Some("claude".to_owned()),
                title: Some("Title".to_owned()),
                msg: Some("Message".to_owned()),
            },
            exported("z", "done"),
            exported("", "error"),
        ]
    );
}

#[test]
fn ac7_export_of_an_empty_table_is_empty() {
    assert!(Table::default().export().is_empty());
    assert_eq!(Table::import(Vec::new()), Table::default());
}

#[test]
fn ac7_importing_an_export_yields_an_equal_table() {
    let full = format!(
        "state=blocked:id=a/b:kind=question:progress=1:app=x:title={}:msg={}",
        b64("T"),
        b64("M")
    );
    let table = table_of(&[
        "state=idle",
        full.as_str(),
        "state=working:id=w:progress=100",
        "state=error:id=e",
    ]);
    assert_eq!(Table::import(table.export()), table);
}

#[test]
fn ac7_the_imported_table_evicts_the_same_record_next() {
    let mut original = full_table();
    // Make the update order differ from insertion order.
    feed(&mut original, "state=working:id=n5");
    feed(&mut original, "state=done:id=n100");
    feed(&mut original, "state=idle:id=n0");

    let mut imported = Table::import(original.export());
    assert_eq!(imported, original);

    for extra in ["extra1", "extra2", "extra3"] {
        let body = format!("state=idle:id={extra}");
        feed(&mut original, &body);
        feed(&mut imported, &body);
        assert_eq!(imported, original, "after inserting {extra}");
    }
    // n0 was refreshed before the export, so n1, n2 and n3 went first.
    for gone in ["n1", "n2", "n3"] {
        assert!(imported.get(gone).is_none(), "{gone}");
    }
    for kept in ["n0", "n4", "n5", "n100"] {
        assert!(imported.get(kept).is_some(), "{kept}");
    }
}

#[test]
fn ac7_import_keeps_valid_boundary_values() {
    let long_title = "t".repeat(192);
    let long_msg = "m".repeat(2048);
    let records = vec![
        ExportedRecord {
            title: Some(long_title.clone()),
            msg: Some(long_msg.clone()),
            ..exported("", "idle")
        },
        ExportedRecord {
            kind: Some("auth".to_owned()),
            progress: Some(100),
            app: Some("a".repeat(32)),
            ..exported(
                &format!("{0}/{0}/{0}/{1}", "s".repeat(32), "t".repeat(29)),
                "blocked",
            )
        },
        ExportedRecord {
            progress: Some(0),
            ..exported("a/b/c/d/e/f/g/h", "working")
        },
    ];
    let table = Table::import(records.clone());
    assert_eq!(table.export(), records);
}

#[test]
fn ac7_import_drops_a_record_that_violates_any_rule() {
    let bad: Vec<(&str, ExportedRecord)> = vec![
        ("id with a space", exported("a b", "idle")),
        ("id with empty segment", exported("a//b", "idle")),
        ("id with leading slash", exported("/a", "idle")),
        (
            "id with nine segments",
            exported("a/b/c/d/e/f/g/h/i", "idle"),
        ),
        (
            "id with a 33-byte segment",
            exported(&"s".repeat(33), "idle"),
        ),
        (
            "id over 128 bytes",
            exported(
                &format!("{0}/{0}/{0}/{1}", "s".repeat(32), "t".repeat(30)),
                "idle",
            ),
        ),
        ("unknown state", exported("x", "bogus")),
        ("clear is not a state", exported("x", "clear")),
        ("state with wrong case", exported("x", "Idle")),
        ("empty state", exported("x", "")),
        (
            "kind on a working record",
            ExportedRecord {
                kind: Some("auth".to_owned()),
                ..exported("x", "working")
            },
        ),
        (
            "kind on a done record",
            ExportedRecord {
                kind: Some("auth".to_owned()),
                ..exported("x", "done")
            },
        ),
        (
            "unknown kind word",
            ExportedRecord {
                kind: Some("bogus".to_owned()),
                ..exported("x", "blocked")
            },
        ),
        (
            "progress on an idle record",
            ExportedRecord {
                progress: Some(5),
                ..exported("x", "idle")
            },
        ),
        (
            "progress on a done record",
            ExportedRecord {
                progress: Some(5),
                ..exported("x", "done")
            },
        ),
        (
            "progress on an error record",
            ExportedRecord {
                progress: Some(5),
                ..exported("x", "error")
            },
        ),
        (
            "progress above 100",
            ExportedRecord {
                progress: Some(101),
                ..exported("x", "working")
            },
        ),
        (
            "huge progress",
            ExportedRecord {
                progress: Some(u32::MAX),
                ..exported("x", "blocked")
            },
        ),
        (
            "app with a space",
            ExportedRecord {
                app: Some("a b".to_owned()),
                ..exported("x", "idle")
            },
        ),
        (
            "empty app",
            ExportedRecord {
                app: Some(String::new()),
                ..exported("x", "idle")
            },
        ),
        (
            "33-byte app",
            ExportedRecord {
                app: Some("a".repeat(33)),
                ..exported("x", "idle")
            },
        ),
        (
            "title above 192 bytes",
            ExportedRecord {
                title: Some("t".repeat(193)),
                ..exported("x", "idle")
            },
        ),
        (
            "title with a control character",
            ExportedRecord {
                title: Some("a\nb".to_owned()),
                ..exported("x", "idle")
            },
        ),
        (
            "title with a C1 control character",
            ExportedRecord {
                title: Some("a\u{85}b".to_owned()),
                ..exported("x", "idle")
            },
        ),
        (
            "msg above 2048 bytes",
            ExportedRecord {
                msg: Some("m".repeat(2049)),
                ..exported("x", "idle")
            },
        ),
        (
            "msg with DEL",
            ExportedRecord {
                msg: Some("a\u{7f}b".to_owned()),
                ..exported("x", "idle")
            },
        ),
    ];

    for (what, record) in bad {
        let table = Table::import(vec![
            exported("first", "idle"),
            record,
            exported("last", "done"),
        ]);
        assert_eq!(
            ids(&table),
            ["first", "last"],
            "{what} must be dropped alone"
        );
    }
}

#[test]
fn ac7_import_keeps_at_most_the_256_most_recently_updated_records() {
    let records: Vec<ExportedRecord> = (0..300)
        .map(|i| exported(&format!("n{i}"), "idle"))
        .collect();
    let table = Table::import(records);
    assert_eq!(table.len(), MAX_RECORDS);
    let expected: Vec<String> = (44..300).map(|i| format!("n{i}")).collect();
    assert_eq!(ids(&table), expected);
}

#[test]
fn ac7_invalid_records_do_not_count_toward_the_cap() {
    let mut records: Vec<ExportedRecord> = Vec::new();
    for i in 0..256 {
        records.push(exported(&format!("n{i}"), "idle"));
        records.push(exported(&format!("bad{i}"), "bogus"));
    }
    let table = Table::import(records);
    assert_eq!(table.len(), MAX_RECORDS);
    assert!(table.get("n0").is_some());
    assert!(table.get("n255").is_some());
}

#[test]
fn ac7_import_re_validates_an_exported_table_without_loss() {
    let table = lifecycle_table();
    let round_trip = Table::import(table.export());
    assert_eq!(round_trip.export(), table.export());
    assert_eq!(round_trip.summary(), table.summary());
}

// ── AC-8: build ───────────────────────────────────────────────────────

#[test]
fn ac8_the_module_is_declared_without_a_gui_gate() {
    let lib = include_str!("../lib.rs");
    let lines: Vec<&str> = lib.lines().map(str::trim).collect();
    let at = lines
        .iter()
        .position(|line| *line == "pub mod program_status;")
        .expect("lib.rs declares `pub mod program_status;`");
    let previous = lines[..at]
        .iter()
        .rev()
        .find(|line| !line.is_empty() && !line.starts_with("//"));
    assert!(
        !previous.is_some_and(|line| line.starts_with("#[cfg")),
        "the declaration must not sit behind a cfg gate, found {previous:?}"
    );
}

#[test]
fn ac8_the_module_uses_only_always_built_crates() {
    let source = include_str!("../program_status.rs");
    for line in source.lines().map(str::trim) {
        if let Some(path) = line.strip_prefix("use ") {
            let root = path.split("::").next().unwrap_or("");
            assert!(
                ["std", "base64", "self", "super"].contains(&root),
                "unexpected dependency in `{line}`"
            );
        }
    }
    assert!(
        !source.contains("crate::"),
        "the module must not reach into other project modules"
    );
}

// ── pane-state-rank-unify: ProgramState::rank derives from compose_rank ──
//
// Names carry the `pane-state-rank-unify` task acceptance criterion they prove
// (`rank_unify_ac1_` .. `rank_unify_ac3_`). They iterate `ProgramState::ALL`
// (five members): `AgentState::ALL` has four and excludes `Error`.

#[test]
fn rank_unify_ac2_each_program_state_maps_to_the_agent_state_of_the_same_name() {
    use crate::agent_status::AgentState;

    assert_eq!(ProgramState::Idle.agent_state(), AgentState::Idle);
    assert_eq!(ProgramState::Working.agent_state(), AgentState::Working);
    assert_eq!(ProgramState::Done.agent_state(), AgentState::Done);
    assert_eq!(ProgramState::Blocked.agent_state(), AgentState::Blocked);
    assert_eq!(ProgramState::Error.agent_state(), AgentState::Error);
}

#[test]
fn rank_unify_ac2_the_mapped_agent_state_display_word_equals_the_protocol_word() {
    for state in ProgramState::ALL {
        assert_eq!(
            state.agent_state().to_string(),
            state.word(),
            "{state:?} must map to the AgentState with the same protocol word"
        );
    }
}

#[test]
fn rank_unify_ac1_rank_equals_the_compose_rank_of_the_mapped_agent_state() {
    for state in ProgramState::ALL {
        assert_eq!(
            state.rank(),
            state.agent_state().compose_rank(),
            "{state:?}: ProgramState::rank must be AgentState::compose_rank of its mapping"
        );
    }
}

#[test]
fn rank_unify_ac3_all_25_ordered_pairs_order_the_same_under_both_rank_functions() {
    let mut pairs = 0;
    for a in ProgramState::ALL {
        for b in ProgramState::ALL {
            assert_eq!(
                a.rank().cmp(&b.rank()),
                a.agent_state()
                    .compose_rank()
                    .cmp(&b.agent_state().compose_rank()),
                "ordering of ({a:?}, {b:?}) must agree between ProgramState::rank and AgentState::compose_rank"
            );
            pairs += 1;
        }
    }
    assert_eq!(pairs, 25);
}

/// The doc comment directly above the first line that starts with
/// `signature`, `///` markers removed and the lines joined by single spaces.
fn doc_comment_above(source: &str, signature: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim_start().starts_with(signature))
        .unwrap_or_else(|| panic!("source declares `{signature}`"));
    let mut doc: Vec<&str> = Vec::new();
    for line in lines[..at].iter().rev() {
        match line.trim_start().strip_prefix("///") {
            Some(text) => doc.push(text.trim()),
            None => break,
        }
    }
    doc.reverse();
    doc.join(" ")
}

#[test]
fn rank_unify_ac4_compose_rank_doc_states_the_single_definition_and_its_derived_users() {
    let doc = doc_comment_above(include_str!("../agent_status.rs"), "pub fn compose_rank(");
    for required in [
        "blocked > working > error > done > idle",
        "single definition",
        "read-flag free",
        "`ProgramState::rank` takes its value from this function",
        "`Table::summary` uses `ProgramState::rank`",
        "`priority_rank`",
        "separately defined",
        "unseen-aware",
        "does not derive its values from this function",
    ] {
        assert!(
            doc.contains(required),
            "the compose_rank doc must contain `{required}`, got: {doc}"
        );
    }
    assert!(
        !doc.contains("adds the unseen distinction"),
        "the compose_rank doc must not say the cross-pane aggregation builds on it: {doc}"
    );
}

#[test]
fn rank_unify_ac5_program_state_rank_doc_names_compose_rank_as_the_source() {
    let doc = doc_comment_above(include_str!("../program_status.rs"), "pub fn rank(");
    assert!(
        doc.contains("AgentState::compose_rank"),
        "the ProgramState::rank doc must name AgentState::compose_rank, got: {doc}"
    );
    assert!(
        !doc.contains("single definition") && !doc.contains("single place"),
        "the ProgramState::rank doc must not present itself as the definition: {doc}"
    );
}

// ── parse_received (osc7501-leading-zero-length task0001, AC-4) ───────
//
// The whole-sequence length is `ESC ]` (2) + the received OSC-string length
// + the terminator. These tests build every boundary from the received
// length, never from the canonical `7501;` introducer.

/// The received length that makes the whole sequence `total` bytes.
fn received_for_sequence_len(total: usize, terminator: Terminator) -> usize {
    total - 2 - terminator.len()
}

fn is_root_set(parsed: &Parsed, state: ProgramState) -> bool {
    matches!(
        parsed,
        Parsed::Report(Report::Set { id, record }) if id.is_empty() && record.state == state
    )
}

#[test]
fn parse_received_ac4_accepts_a_whole_sequence_of_4096_bytes_and_ignores_4097() {
    for terminator in [Terminator::Bel, Terminator::St] {
        let body = b"state=error";
        let at_limit = parse_received(
            body,
            received_for_sequence_len(4096, terminator),
            terminator,
        );
        assert!(
            is_root_set(&at_limit, ProgramState::Error),
            "{terminator:?}: 4096 bytes must be accepted, got {at_limit:?}"
        );

        let over = parse_received(
            body,
            received_for_sequence_len(4097, terminator),
            terminator,
        );
        assert_eq!(over, Parsed::Ignored, "{terminator:?}: 4097 bytes");
    }
}

#[test]
fn parse_received_ac4_the_terminator_counts_toward_the_whole_sequence() {
    // The same received length is accepted with BEL and over the limit
    // with ST.
    let received = received_for_sequence_len(4096, Terminator::Bel);
    assert!(is_root_set(
        &parse_received(b"state=error", received, Terminator::Bel),
        ProgramState::Error
    ));
    assert_eq!(
        parse_received(b"state=error", received, Terminator::St),
        Parsed::Ignored
    );
}

#[test]
fn parse_received_ac4_a_short_received_length_does_not_ignore_the_body() {
    for terminator in [Terminator::Bel, Terminator::St] {
        for received in [0, 1, 11, 16] {
            assert!(
                is_root_set(
                    &parse_received(b"state=error", received, terminator),
                    ProgramState::Error
                ),
                "{terminator:?} received {received}"
            );
        }
    }
}

#[test]
fn parse_received_ac4_the_limit_follows_the_received_length_not_the_replaced_text() {
    // 4074 bytes of 0xFF are received as 4074 bytes but their replacement
    // text is 3 bytes each, far past 4096 bytes in all.
    let mut body = b"state=error:x=".to_vec();
    for _ in 0..4074 {
        body.extend_from_slice("\u{FFFD}".as_bytes());
    }
    assert!(body.len() > MAX_SEQUENCE_BYTES);
    let received = 5 + b"state=error:x=".len() + 4074;
    assert_eq!(received, received_for_sequence_len(4096, Terminator::Bel));

    assert!(is_root_set(
        &parse_received(&body, received, Terminator::Bel),
        ProgramState::Error
    ));
    assert_eq!(
        parse_received(&body, received + 1, Terminator::Bel),
        Parsed::Ignored
    );
}

#[test]
fn parse_received_ac4_a_query_is_a_query_for_every_received_length() {
    let max_osc_len = 16 * 1024 * 1024;
    for terminator in [Terminator::Bel, Terminator::St] {
        for received in [
            0,
            received_for_sequence_len(4096, terminator),
            received_for_sequence_len(4097, terminator),
            5_000,
            max_osc_len,
            max_osc_len + 1,
            usize::MAX,
        ] {
            assert_eq!(
                parse_received(b"?", received, terminator),
                Parsed::Query,
                "{terminator:?} received {received}"
            );
        }
    }
}

#[test]
fn parse_received_ac4_only_a_body_of_exactly_a_question_mark_is_a_query() {
    for body in [&b"??"[..], b"? ", b" ?", b"", b"?=", b"state=?"] {
        assert_ne!(
            parse_received(body, 0, Terminator::Bel),
            Parsed::Query,
            "{body:?}"
        );
        assert_eq!(
            parse_received(body, usize::MAX, Terminator::Bel),
            Parsed::Ignored,
            "{body:?} with a huge received length"
        );
    }
}

#[test]
fn parse_received_ac4_a_saturated_received_length_is_ignored_without_overflow() {
    for terminator in [Terminator::Bel, Terminator::St] {
        for received in [usize::MAX, usize::MAX - 1, usize::MAX - 2, usize::MAX - 3] {
            assert_eq!(
                parse_received(b"state=error", received, terminator),
                Parsed::Ignored,
                "{terminator:?} received {received}"
            );
        }
    }
}

#[test]
fn parse_received_ac4_the_canonical_entries_are_parse_received_with_the_canonical_length() {
    let boundary_bel = body_of_sequence_len(4096, Terminator::Bel);
    let boundary_st = body_of_sequence_len(4097, Terminator::St);
    let bodies = [
        "state=working",
        "id=a:state=blocked:kind=question",
        "state=clear",
        "?",
        "",
        "state=sleeping",
        boundary_bel.as_str(),
        boundary_st.as_str(),
    ];
    for terminator in [Terminator::Bel, Terminator::St] {
        for body in bodies {
            // `7501;` introducer (5 bytes) + the body.
            let canonical = 5 + body.len();
            let expected = parse_received(body.as_bytes(), canonical, terminator);
            assert_eq!(parse(body, terminator), expected, "{terminator:?} {body:?}");
            assert_eq!(
                parse_bytes(body.as_bytes(), terminator),
                expected,
                "{terminator:?} {body:?}"
            );
        }
    }
}

#[test]
fn parse_received_ac7_rejecting_or_ignoring_input_logs_nothing() {
    // Rejected input is untrusted and could flood the log: the module has
    // no logging call at all.
    let source = include_str!("../program_status.rs");
    for needle in ["log::", "println!", "eprintln!", "dbg!"] {
        assert!(!source.contains(needle), "found `{needle}`");
    }
}
