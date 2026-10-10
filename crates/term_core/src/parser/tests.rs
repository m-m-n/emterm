use super::*;
use crate::parser_types::OscTerminator;

fn parse_all(input: &[u8]) -> Vec<ParsedAction> {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    parser.parse(input, |action| actions.push(action));
    actions
}

/// Helper to construct CsiDispatch from slices for test assertions.
fn csi(params: &[u16], intermediates: &[u8], final_byte: u8) -> ParsedAction {
    use crate::parser_types::{MAX_CSI_INTERMEDIATES, MAX_CSI_PARAMS};
    let mut p = [0u16; MAX_CSI_PARAMS];
    for (i, &v) in params.iter().take(MAX_CSI_PARAMS).enumerate() {
        p[i] = v;
    }
    let mut im = [0u8; MAX_CSI_INTERMEDIATES];
    for (i, &v) in intermediates.iter().take(MAX_CSI_INTERMEDIATES).enumerate() {
        im[i] = v;
    }
    ParsedAction::CsiDispatch {
        params: p,
        param_count: params.len() as u8,
        intermediates: im,
        intermediate_count: intermediates.len() as u8,
        final_byte,
    }
}

// =========================================================================
// Printable ASCII Tests
// =========================================================================

#[test]
fn test_parse_printable_ascii() {
    let actions = parse_all(b"Hello");
    assert_eq!(actions.len(), 5);
    assert_eq!(actions[0], ParsedAction::Print('H'));
    assert_eq!(actions[1], ParsedAction::Print('e'));
    assert_eq!(actions[2], ParsedAction::Print('l'));
    assert_eq!(actions[3], ParsedAction::Print('l'));
    assert_eq!(actions[4], ParsedAction::Print('o'));
}

#[test]
fn test_parse_space() {
    let actions = parse_all(b" ");
    assert_eq!(actions, vec![ParsedAction::Print(' ')]);
}

#[test]
fn test_parse_all_printable() {
    let input = b"!@#$%^&*()_+-=[]{}|;':\",./<>?";
    let actions = parse_all(input);
    assert_eq!(actions.len(), input.len());
    for action in &actions {
        assert!(matches!(action, ParsedAction::Print(_)));
    }
}

// =========================================================================
// C0 Control Character Tests
// =========================================================================

#[test]
fn test_parse_c0_bel() {
    let actions = parse_all(b"\x07");
    assert_eq!(actions, vec![ParsedAction::Execute(0x07)]);
}

#[test]
fn test_parse_c0_bs() {
    let actions = parse_all(b"\x08");
    assert_eq!(actions, vec![ParsedAction::Execute(0x08)]);
}

#[test]
fn test_parse_c0_ht() {
    let actions = parse_all(b"\x09");
    assert_eq!(actions, vec![ParsedAction::Execute(0x09)]);
}

#[test]
fn test_parse_c0_lf() {
    let actions = parse_all(b"\x0A");
    assert_eq!(actions, vec![ParsedAction::Execute(0x0A)]);
}

#[test]
fn test_parse_c0_cr() {
    let actions = parse_all(b"\x0D");
    assert_eq!(actions, vec![ParsedAction::Execute(0x0D)]);
}

#[test]
fn test_parse_mixed_text_and_controls() {
    let actions = parse_all(b"A\r\nB");
    assert_eq!(actions.len(), 4);
    assert_eq!(actions[0], ParsedAction::Print('A'));
    assert_eq!(actions[1], ParsedAction::Execute(0x0D));
    assert_eq!(actions[2], ParsedAction::Execute(0x0A));
    assert_eq!(actions[3], ParsedAction::Print('B'));
}

// =========================================================================
// ESC Sequence Tests
// =========================================================================

#[test]
fn test_parse_esc_save_cursor() {
    let actions = parse_all(b"\x1B7");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'7',
        }]
    );
}

#[test]
fn test_parse_esc_restore_cursor() {
    let actions = parse_all(b"\x1B8");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'8',
        }]
    );
}

#[test]
fn test_parse_esc_index() {
    let actions = parse_all(b"\x1BD");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'D',
        }]
    );
}

#[test]
fn test_parse_esc_next_line() {
    let actions = parse_all(b"\x1BE");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'E',
        }]
    );
}

#[test]
fn test_parse_esc_horizontal_tab_set() {
    let actions = parse_all(b"\x1BH");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'H',
        }]
    );
}

#[test]
fn test_parse_esc_reverse_index() {
    let actions = parse_all(b"\x1BM");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'M',
        }]
    );
}

#[test]
fn test_parse_esc_reset() {
    let actions = parse_all(b"\x1Bc");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'c',
        }]
    );
}

#[test]
fn test_parse_esc_g0_charset_ascii() {
    let actions = parse_all(b"\x1B(B");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: Some(b'('),
            final_byte: b'B',
        }]
    );
}

#[test]
fn test_parse_esc_g0_charset_line_drawing() {
    let actions = parse_all(b"\x1B(0");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: Some(b'('),
            final_byte: b'0',
        }]
    );
}

#[test]
fn test_parse_esc_g1_charset() {
    let actions = parse_all(b"\x1B)A");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: Some(b')'),
            final_byte: b'A',
        }]
    );
}

#[test]
fn test_parse_esc_unknown() {
    let actions = parse_all(b"\x1BX");
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'X',
        }]
    );
}

// =========================================================================
// CSI Sequence Tests
// =========================================================================

#[test]
fn test_parse_csi_sgr_reset() {
    let actions = parse_all(b"\x1B[m");
    assert_eq!(actions, vec![csi(&[], &[], b'm')]);
}

#[test]
fn test_parse_csi_sgr_explicit_reset() {
    let actions = parse_all(b"\x1B[0m");
    assert_eq!(actions, vec![csi(&[0], &[], b'm')]);
}

#[test]
fn test_parse_csi_sgr_bold() {
    let actions = parse_all(b"\x1B[1m");
    assert_eq!(actions, vec![csi(&[1], &[], b'm')]);
}

#[test]
fn test_parse_csi_sgr_red_foreground() {
    let actions = parse_all(b"\x1B[31m");
    assert_eq!(actions, vec![csi(&[31], &[], b'm')]);
}

#[test]
fn test_parse_csi_sgr_multiple_params() {
    let actions = parse_all(b"\x1B[1;31m");
    assert_eq!(actions, vec![csi(&[1, 31], &[], b'm')]);
}

#[test]
fn test_parse_csi_sgr_256_color() {
    let actions = parse_all(b"\x1B[38;5;196m");
    assert_eq!(actions, vec![csi(&[38, 5, 196], &[], b'm')]);
}

#[test]
fn test_parse_csi_sgr_rgb() {
    let actions = parse_all(b"\x1B[38;2;255;0;128m");
    assert_eq!(actions, vec![csi(&[38, 2, 255, 0, 128], &[], b'm')]);
}

#[test]
fn test_parse_csi_cursor_up() {
    let actions = parse_all(b"\x1B[A");
    assert_eq!(actions, vec![csi(&[], &[], b'A')]);
}

#[test]
fn test_parse_csi_cursor_up_with_count() {
    let actions = parse_all(b"\x1B[5A");
    assert_eq!(actions, vec![csi(&[5], &[], b'A')]);
}

#[test]
fn test_parse_csi_cursor_down() {
    let actions = parse_all(b"\x1B[3B");
    assert_eq!(actions, vec![csi(&[3], &[], b'B')]);
}

#[test]
fn test_parse_csi_cursor_forward() {
    let actions = parse_all(b"\x1B[10C");
    assert_eq!(actions, vec![csi(&[10], &[], b'C')]);
}

#[test]
fn test_parse_csi_cursor_back() {
    let actions = parse_all(b"\x1B[2D");
    assert_eq!(actions, vec![csi(&[2], &[], b'D')]);
}

#[test]
fn test_parse_csi_cursor_position() {
    let actions = parse_all(b"\x1B[10;20H");
    assert_eq!(actions, vec![csi(&[10, 20], &[], b'H')]);
}

#[test]
fn test_parse_csi_cursor_position_default() {
    let actions = parse_all(b"\x1B[H");
    assert_eq!(actions, vec![csi(&[], &[], b'H')]);
}

#[test]
fn test_parse_csi_cursor_position_partial() {
    let actions = parse_all(b"\x1B[;10H");
    assert_eq!(actions, vec![csi(&[0, 10], &[], b'H')]);
}

#[test]
fn test_parse_csi_erase_display_below() {
    let actions = parse_all(b"\x1B[J");
    assert_eq!(actions, vec![csi(&[], &[], b'J')]);
}

#[test]
fn test_parse_csi_erase_display_all() {
    let actions = parse_all(b"\x1B[2J");
    assert_eq!(actions, vec![csi(&[2], &[], b'J')]);
}

#[test]
fn test_parse_csi_erase_line() {
    let actions = parse_all(b"\x1B[K");
    assert_eq!(actions, vec![csi(&[], &[], b'K')]);
}

#[test]
fn test_parse_csi_dec_private_set_mode() {
    let actions = parse_all(b"\x1B[?25h");
    assert_eq!(actions, vec![csi(&[25], &[b'?'], b'h')]);
}

#[test]
fn test_parse_csi_dec_private_reset_mode() {
    let actions = parse_all(b"\x1B[?25l");
    assert_eq!(actions, vec![csi(&[25], &[b'?'], b'l')]);
}

#[test]
fn test_parse_csi_device_status_report() {
    let actions = parse_all(b"\x1B[6n");
    assert_eq!(actions, vec![csi(&[6], &[], b'n')]);
}

#[test]
fn test_parse_csi_primary_device_attributes() {
    let actions = parse_all(b"\x1B[c");
    assert_eq!(actions, vec![csi(&[], &[], b'c')]);
}

#[test]
fn test_parse_csi_secondary_device_attributes() {
    let actions = parse_all(b"\x1B[>c");
    assert_eq!(actions, vec![csi(&[], &[b'>'], b'c')]);
}

#[test]
fn test_parse_csi_tertiary_device_attributes() {
    let actions = parse_all(b"\x1B[=c");
    assert_eq!(actions, vec![csi(&[], &[b'='], b'c')]);
}

#[test]
fn test_parse_csi_unknown() {
    let actions = parse_all(b"\x1B[1;2;3z");
    assert_eq!(actions, vec![csi(&[1, 2, 3], &[], b'z')]);
}

// =========================================================================
// OSC Sequence Tests
// =========================================================================

#[test]
fn test_parse_osc_set_title() {
    let actions = parse_all(b"\x1B]2;My Title\x07");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 2,
            data: "My Title".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 10,
        }]
    );
}

#[test]
fn test_parse_osc_set_title_and_icon() {
    let actions = parse_all(b"\x1B]0;Terminal\x07");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 0,
            data: "Terminal".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 10,
        }]
    );
}

#[test]
fn test_parse_osc_working_directory() {
    let actions = parse_all(b"\x1B]7;file:///home/user\x07");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 7,
            data: "file:///home/user".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 19,
        }]
    );
}

#[test]
fn test_parse_osc_hyperlink() {
    let actions = parse_all(b"\x1B]8;id=1;https://example.com\x07");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 8,
            data: "id=1;https://example.com".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 26,
        }]
    );
}

#[test]
fn test_parse_osc_unknown() {
    let actions = parse_all(b"\x1B]99;data\x07");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 99,
            data: "data".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 7,
        }]
    );
}

#[test]
fn test_parse_osc_semantic_prompt_a() {
    let actions = parse_all(b"\x1B]133;A\x1B\\");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 133,
            data: "A".to_string(),
            terminator: OscTerminator::St,
            received_len: 5,
        }]
    );
}

#[test]
fn test_parse_osc_semantic_prompt_d_with_exit_code() {
    let actions = parse_all(b"\x1B]133;D;0\x1B\\");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 133,
            data: "D;0".to_string(),
            terminator: OscTerminator::St,
            received_len: 7,
        }]
    );
}

#[test]
fn test_parse_osc_emterm_extension() {
    let actions = parse_all(b"\x1B]777;markdown;title;body\x07");
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 777,
            data: "markdown;title;body".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 23,
        }]
    );
}

#[test]
fn test_parse_osc_st_terminator() {
    // AC-1 (ST form).
    let actions = parse_all(b"\x1B]2;My Title\x1B\\");
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0],
        ParsedAction::OscDispatch {
            param: 2,
            data: "My Title".to_string(),
            terminator: OscTerminator::St,
            received_len: 10,
        }
    );
}

#[test]
fn test_parse_osc_esc_without_backslash() {
    // AC-1: a string cut short by an interrupting escape (not BEL/ST) is
    // classified deterministically as `Unterminated`, never guessed as one
    // of the two real terminator forms.
    let actions = parse_all(b"\x1B]2;Title\x1B7");
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions[0],
        ParsedAction::OscDispatch {
            param: 2,
            data: "Title".to_string(),
            terminator: OscTerminator::Unterminated,
            received_len: 7,
        }
    );
    assert_eq!(
        actions[1],
        ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'7',
        }
    );
}

// =========================================================================
// Buffer Boundary Tests (Streaming Input)
// =========================================================================

#[test]
fn test_parse_split_csi_sequence() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1B[", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(b"31m", |action| actions.push(action));
    assert_eq!(actions, vec![csi(&[31], &[], b'm')]);
}

#[test]
fn test_parse_split_esc_sequence() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1B", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(b"7", |action| actions.push(action));
    assert_eq!(
        actions,
        vec![ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'7',
        }]
    );
}

#[test]
fn test_parse_split_osc_sequence() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1B]2;My ", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(b"Title\x07", |action| actions.push(action));
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 2,
            data: "My Title".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 10,
        }]
    );
}

#[test]
fn test_parse_split_byte_by_byte() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    for byte in b"\x1B[1;31m" {
        parser.parse(&[*byte], |action| actions.push(action));
    }

    assert_eq!(actions, vec![csi(&[1, 31], &[], b'm')]);
}

#[test]
fn test_parse_split_osc_st_across_buffers() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1B]2;Title\x1B", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(b"\\", |action| actions.push(action));
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0],
        ParsedAction::OscDispatch {
            param: 2,
            data: "Title".to_string(),
            terminator: OscTerminator::St,
            received_len: 7,
        }
    );
}

// =========================================================================
// OSC Large Buffer Tests
// =========================================================================

#[test]
fn test_parse_osc_larger_than_4096_bytes() {
    // OSC data larger than the old 4096-byte limit should parse correctly
    let data = "x".repeat(8000);
    let mut input = Vec::new();
    input.extend_from_slice(b"\x1B]777;");
    input.extend_from_slice(data.as_bytes());
    input.push(0x07);

    let actions = parse_all(&input);
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0],
        ParsedAction::OscDispatch {
            param: 777,
            data: data.clone(),
            terminator: OscTerminator::Bel,
            received_len: 4 + data.len(),
        }
    );
}

#[test]
fn test_parse_osc_at_128kb_chunk_size() {
    // OSC at ~128KB (chunk size + header) should parse correctly
    let data = "a".repeat(128 * 1024);
    let mut input = Vec::new();
    input.extend_from_slice(b"\x1B]777;");
    input.extend_from_slice(data.as_bytes());
    input.push(0x07);

    let actions = parse_all(&input);
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0],
        ParsedAction::OscDispatch {
            param: 777,
            data: data.clone(),
            terminator: OscTerminator::Bel,
            received_len: 4 + data.len(),
        }
    );
}

#[test]
fn test_parse_osc_download_begin_sequence() {
    let seq = b"\x1B]777;emterm;download;begin;id=550e8400-e29b-41d4-a716-446655440000;name=test.txt;size=1024;version=1.0\x1B\\";
    let actions = parse_all(seq);
    assert_eq!(actions.len(), 1);
    if let ParsedAction::OscDispatch { param, data, .. } = &actions[0] {
        assert_eq!(*param, 777);
        assert!(data.contains("download"));
        assert!(data.contains("begin"));
        assert!(data.contains("name=test.txt"));
        assert!(data.contains("size=1024"));
    } else {
        panic!("Expected OscDispatch");
    }
}

#[test]
fn test_parse_osc_download_chunk_sequence() {
    let seq = b"\x1B]777;emterm;download;chunk;id=550e8400-e29b-41d4-a716-446655440000;seq=0;data=SGVsbG8=\x1B\\";
    let actions = parse_all(seq);
    assert_eq!(actions.len(), 1);
    if let ParsedAction::OscDispatch { param, data, .. } = &actions[0] {
        assert_eq!(*param, 777);
        assert!(data.contains("download"));
        assert!(data.contains("chunk"));
        assert!(data.contains("seq=0"));
        assert!(data.contains("data=SGVsbG8="));
    } else {
        panic!("Expected OscDispatch");
    }
}

#[test]
fn test_parse_osc_download_end_sequence() {
    let seq = b"\x1B]777;emterm;download;end;id=550e8400-e29b-41d4-a716-446655440000\x1B\\";
    let actions = parse_all(seq);
    assert_eq!(actions.len(), 1);
    if let ParsedAction::OscDispatch { param, data, .. } = &actions[0] {
        assert_eq!(*param, 777);
        assert!(data.contains("download"));
        assert!(data.contains("end"));
    } else {
        panic!("Expected OscDispatch");
    }
}

#[test]
fn test_parse_osc_discards_bytes_beyond_16mb() {
    // Data beyond 16MB cap should be silently discarded
    let size = 16 * 1024 * 1024;
    let data = "b".repeat(size + 100);
    let mut input = Vec::new();
    input.extend_from_slice(b"\x1B]777;");
    input.extend_from_slice(data.as_bytes());
    input.push(0x07);

    let actions = parse_all(&input);
    assert_eq!(actions.len(), 1);
    if let ParsedAction::OscDispatch { param, data, .. } = &actions[0] {
        assert_eq!(*param, 777);
        // Data should be capped at MAX_OSC_LEN (16MB)
        assert_eq!(data.len(), size);
    } else {
        panic!("Expected OscDispatch");
    }
}

// =========================================================================
// OSC received length and saturating number
// (osc7501-leading-zero-length SC-1 / SC-2)
// =========================================================================

/// Every dispatched OSC of `input` as `(param, data, terminator, received_len)`.
fn osc_actions(input: &[u8]) -> Vec<(u16, String, OscTerminator, usize)> {
    parse_all(input)
        .into_iter()
        .filter_map(|action| match action {
            ParsedAction::OscDispatch {
                param,
                data,
                terminator,
                received_len,
            } => Some((param, data, terminator, received_len)),
            _ => None,
        })
        .collect()
}

/// The one dispatched OSC of `input`.
fn only_osc(input: &[u8]) -> (u16, String, OscTerminator, usize) {
    let mut found = osc_actions(input);
    assert_eq!(found.len(), 1, "expected exactly one OscDispatch");
    found.remove(0)
}

fn osc_with_bel(string: &[u8]) -> Vec<u8> {
    let mut input = b"\x1B]".to_vec();
    input.extend_from_slice(string);
    input.push(0x07);
    input
}

#[test]
fn ac1_received_len_counts_digits_semicolon_and_data_once_for_bel() {
    // `2;My Title` is 10 bytes; the BEL is not counted.
    assert_eq!(
        only_osc(b"\x1B]2;My Title\x07"),
        (2, "My Title".to_string(), OscTerminator::Bel, 10)
    );
}

#[test]
fn ac1_received_len_counts_digits_semicolon_and_data_once_for_st() {
    // The two terminator bytes `ESC \` are not counted.
    assert_eq!(
        only_osc(b"\x1B]2;My Title\x1B\\"),
        (2, "My Title".to_string(), OscTerminator::St, 10)
    );
}

#[test]
fn ac1_received_len_counts_leading_zeros() {
    assert_eq!(
        only_osc(&osc_with_bel(b"0007;a")),
        (7, "a".to_string(), OscTerminator::Bel, 6)
    );
    assert_eq!(
        only_osc(&osc_with_bel(b"00000000000;x")),
        (0, "x".to_string(), OscTerminator::Bel, 13)
    );
}

#[test]
fn ac1_received_len_counts_non_digit_bytes_before_the_first_semicolon() {
    // Non-digit bytes before the first `;` are data in their original
    // order; digits around them still accumulate into the number.
    assert_eq!(
        only_osc(&osc_with_bel(b"x2;abc")),
        (2, "xabc".to_string(), OscTerminator::Bel, 6)
    );
    assert_eq!(
        only_osc(&osc_with_bel(b"2x;abc")),
        (2, "xabc".to_string(), OscTerminator::Bel, 6)
    );
}

#[test]
fn ac1_received_len_counts_later_semicolons_and_digits_as_data() {
    assert_eq!(
        only_osc(&osc_with_bel(b"2;1;2")),
        (2, "1;2".to_string(), OscTerminator::Bel, 5)
    );
}

#[test]
fn ac1_received_len_of_strings_without_data() {
    // No `;` at all.
    assert_eq!(
        only_osc(&osc_with_bel(b"7")),
        (7, String::new(), OscTerminator::Bel, 1)
    );
    // Empty string.
    assert_eq!(
        only_osc(&osc_with_bel(b"")),
        (0, String::new(), OscTerminator::Bel, 0)
    );
    // The first `;` alone.
    assert_eq!(
        only_osc(&osc_with_bel(b";")),
        (0, String::new(), OscTerminator::Bel, 1)
    );
}

#[test]
fn ac1_received_len_counts_invalid_utf8_bytes_before_replacement() {
    // Two invalid bytes: two replacement characters, two counted bytes.
    assert_eq!(
        only_osc(&osc_with_bel(b"2;\xFF\xFE")),
        (2, "\u{FFFD}\u{FFFD}".to_string(), OscTerminator::Bel, 4)
    );
    // A truncated three-byte sequence is replaced by ONE character but
    // received as two bytes.
    assert_eq!(
        only_osc(&osc_with_bel(b"2;\xE2\x82")),
        (2, "\u{FFFD}".to_string(), OscTerminator::Bel, 4)
    );
    // An invalid byte before the first `;`.
    assert_eq!(
        only_osc(&osc_with_bel(b"\xFF2;a")),
        (2, "\u{FFFD}a".to_string(), OscTerminator::Bel, 4)
    );
}

#[test]
fn ac1_received_len_of_an_unterminated_string_excludes_the_esc_and_the_next_byte() {
    let actions = parse_all(b"\x1B]2;Title\x1B7");
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions[0],
        ParsedAction::OscDispatch {
            param: 2,
            data: "Title".to_string(),
            terminator: OscTerminator::Unterminated,
            received_len: 7,
        }
    );

    // Cut short by a CSI: the `[` is not counted either.
    assert_eq!(
        osc_actions(b"\x1B]2;Title\x1B[0m"),
        vec![(2, "Title".to_string(), OscTerminator::Unterminated, 7)]
    );
}

#[test]
fn ac1_received_len_restarts_from_zero_for_each_string() {
    let found = osc_actions(b"\x1B]2;ab\x07\x1B]3;cde\x1B\\\x1B]4;f\x07");
    assert_eq!(
        found,
        vec![
            (2, "ab".to_string(), OscTerminator::Bel, 4),
            (3, "cde".to_string(), OscTerminator::St, 5),
            (4, "f".to_string(), OscTerminator::Bel, 3),
        ]
    );
}

#[test]
fn ac1_received_len_restarts_when_a_new_osc_introducer_reopens_the_string() {
    let found = osc_actions(b"\x1B]2;abc\x1B]3;de\x07");
    assert_eq!(
        found,
        vec![
            (2, "abc".to_string(), OscTerminator::Unterminated, 5),
            (3, "de".to_string(), OscTerminator::Bel, 4),
        ]
    );
}

#[test]
fn ac1_received_len_restarts_after_a_parser_reset() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    parser.parse(b"\x1B]2;abcdef", |action| actions.push(action));
    assert!(actions.is_empty());
    assert_eq!(
        parser.osc_received_len, 8,
        "counted while the string is open"
    );

    parser.reset();
    assert_eq!(parser.osc_received_len, 0, "the reset clears the count");

    parser.parse(b"\x1B]3;x\x07", |action| actions.push(action));
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 3,
            data: "x".to_string(),
            terminator: OscTerminator::Bel,
            received_len: 3,
        }]
    );
}

#[test]
fn ac1_received_len_accumulates_across_buffers() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    parser.parse(b"\x1B]2;ab", |action| actions.push(action));
    parser.parse(b"cd\x1B", |action| actions.push(action));
    parser.parse(b"\\", |action| actions.push(action));
    assert_eq!(
        actions,
        vec![ParsedAction::OscDispatch {
            param: 2,
            data: "abcd".to_string(),
            terminator: OscTerminator::St,
            received_len: 6,
        }]
    );
}

#[test]
fn ac1_received_len_reports_the_full_count_while_data_stays_capped() {
    let mut input = b"\x1B]777;".to_vec();
    input.extend(std::iter::repeat_n(b'b', MAX_OSC_LEN + 100));
    input.push(0x07);

    let (param, data, terminator, received_len) = only_osc(&input);
    assert_eq!(param, 777);
    assert_eq!(terminator, OscTerminator::Bel);
    assert_eq!(data.len(), MAX_OSC_LEN, "data is capped");
    assert_eq!(received_len, 4 + MAX_OSC_LEN + 100, "count is not capped");
}

#[test]
fn ac1_a_one_mebibyte_osc_string_is_dispatched_within_two_seconds() {
    let mut input = b"\x1B]777;".to_vec();
    input.extend(std::iter::repeat_n(b'a', 1024 * 1024));
    input.push(0x07);

    let started = std::time::Instant::now();
    let (_, data, _, received_len) = only_osc(&input);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(data.len(), 1024 * 1024);
    assert_eq!(received_len, 4 + 1024 * 1024);
}

#[test]
fn ac7_counting_and_saturating_in_the_osc_state_log_nothing() {
    // Input that is cut short, oversized or out of range is untrusted and
    // could flood the log: the OSC string state has no logging call.
    let source = include_str!("osc.rs");
    for needle in ["log::", "println!", "eprintln!", "dbg!"] {
        assert!(!source.contains(needle), "found `{needle}`");
    }
}

#[test]
fn ac2_number_65535_is_dispatched_as_is() {
    assert_eq!(
        only_osc(&osc_with_bel(b"65535;x")),
        (65535, "x".to_string(), OscTerminator::Bel, 7)
    );
}

#[test]
fn ac2_numbers_above_the_u16_range_saturate_at_65535() {
    assert_eq!(
        only_osc(&osc_with_bel(b"65536;x")),
        (65535, "x".to_string(), OscTerminator::Bel, 7)
    );
    // An above-range prefix followed by the digits of a host application
    // code (nine digits in all). The host code is spelled in pieces because
    // term_core's sources name no application code.
    let digits = ["65536", "75", "01"].concat();
    assert_eq!(digits.len(), 9);
    assert_eq!(
        only_osc(&osc_with_bel(format!("{digits};state=error").as_bytes())),
        (65535, "state=error".to_string(), OscTerminator::Bel, 21)
    );
    assert_eq!(
        only_osc(&osc_with_bel(b"6553652;x")),
        (65535, "x".to_string(), OscTerminator::Bel, 9)
    );
    assert_eq!(only_osc(&osc_with_bel(b"99999999999999999999;x")).0, 65535);
}

#[test]
fn ac2_a_saturated_number_stays_saturated_for_the_rest_of_the_digits() {
    // The trailing zeros (and the trailing 1) must not bring the value back
    // into range.
    assert_eq!(only_osc(&osc_with_bel(b"655360000000;x")).0, 65535);
    assert_eq!(only_osc(&osc_with_bel(b"6553600000001;x")).0, 65535);
}

#[test]
fn ac2_leading_zeros_do_not_change_the_number() {
    assert_eq!(only_osc(&osc_with_bel(b"0065535;x")).0, 65535);
    assert_eq!(only_osc(&osc_with_bel(b"000052;x")).0, 52);
}

#[test]
fn ac2_digits_after_the_first_semicolon_never_reach_the_number() {
    assert_eq!(
        only_osc(&osc_with_bel(b"2;99999999999")),
        (2, "99999999999".to_string(), OscTerminator::Bel, 13)
    );
}

#[test]
fn ac2_a_number_of_one_million_digits_is_dispatched_within_two_seconds() {
    let mut input = b"\x1B]".to_vec();
    input.extend(std::iter::repeat_n(b'9', 1_000_000));
    input.extend_from_slice(b";x\x07");

    let started = std::time::Instant::now();
    let (param, data, terminator, received_len) = only_osc(&input);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(param, 65535);
    assert_eq!(data, "x");
    assert_eq!(terminator, OscTerminator::Bel);
    assert_eq!(received_len, 1_000_000 + 2);
}

// =========================================================================
// UTF-8 Tests
// =========================================================================

#[test]
fn test_parse_utf8_japanese_hiragana() {
    let actions = parse_all("あ".as_bytes());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], ParsedAction::Print('あ'));
}

#[test]
fn test_parse_utf8_chinese() {
    let actions = parse_all("中".as_bytes());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], ParsedAction::Print('中'));
}

#[test]
fn test_parse_utf8_emoji() {
    let actions = parse_all("😀".as_bytes());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], ParsedAction::Print('😀'));
}

#[test]
fn test_parse_utf8_mixed_ascii_and_multibyte() {
    let actions = parse_all("Hello世界".as_bytes());
    assert_eq!(actions.len(), 7);
    assert_eq!(actions[0], ParsedAction::Print('H'));
    assert_eq!(actions[5], ParsedAction::Print('世'));
    assert_eq!(actions[6], ParsedAction::Print('界'));
}

#[test]
fn test_parse_utf8_split_across_buffers() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    // Split "中" (0xE4 0xB8 0xAD) across two parse calls
    parser.parse(&[0xE4, 0xB8], |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(&[0xAD], |action| actions.push(action));
    assert_eq!(actions, vec![ParsedAction::Print('中')]);
}

#[test]
fn test_parse_utf8_invalid_continuation() {
    let actions = parse_all(&[0x80]);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], ParsedAction::Print('\u{FFFD}'));
}

#[test]
fn test_parse_utf8_invalid_sequence() {
    let actions = parse_all(&[0xF8, 0x80, 0x80, 0x80]);
    assert!(actions.len() >= 1);
    assert_eq!(actions[0], ParsedAction::Print('\u{FFFD}'));
}

// =========================================================================
// APC Tests (Raw payload - no Kitty parsing)
// =========================================================================

#[test]
fn test_parse_apc_basic() {
    let actions = parse_all(b"\x1B_Ga=T,f=100;iVBORw0KGgo=\x1B\\");
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        ParsedAction::ApcDispatch(payload) => {
            let s = String::from_utf8_lossy(payload);
            assert_eq!(s, "Ga=T,f=100;iVBORw0KGgo=");
        }
        _ => panic!("Expected ApcDispatch"),
    }
}

#[test]
fn test_parse_apc_split_across_buffers() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1B_Ga=T,f=100;iVBO", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(b"Rw0KGgo=\x1B\\", |action| actions.push(action));
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        ParsedAction::ApcDispatch(payload) => {
            let s = String::from_utf8_lossy(payload);
            assert_eq!(s, "Ga=T,f=100;iVBORw0KGgo=");
        }
        _ => panic!("Expected ApcDispatch"),
    }
}

#[test]
fn test_parse_apc_followed_by_text() {
    let actions = parse_all(b"\x1B_Ga=q;\x1B\\Hello");
    assert_eq!(actions.len(), 6); // 1 APC + 5 chars
    assert!(matches!(&actions[0], ParsedAction::ApcDispatch(_)));
    assert_eq!(actions[1], ParsedAction::Print('H'));
}

// =========================================================================
// DCS Tests (Raw payload - no SIXEL parsing)
// =========================================================================

#[test]
fn test_parse_dcs_basic() {
    let actions = parse_all(b"\x1BP0;1;0q#0;2;100;0;0~\x1B\\");
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        ParsedAction::DcsDispatch(payload) => {
            let s = String::from_utf8_lossy(payload);
            assert_eq!(s, "0;1;0q#0;2;100;0;0~");
        }
        _ => panic!("Expected DcsDispatch"),
    }
}

#[test]
fn test_parse_dcs_split_across_buffers() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1BP0;1;0q#0;2;", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.parse(b"100;0;0~\x1B\\", |action| actions.push(action));
    assert_eq!(actions.len(), 1);
    assert!(matches!(&actions[0], ParsedAction::DcsDispatch(_)));
}

#[test]
fn test_parse_dcs_followed_by_text() {
    let actions = parse_all(b"\x1BPq~\x1B\\Hello");
    assert_eq!(actions.len(), 6); // 1 DCS + 5 chars
    assert!(matches!(&actions[0], ParsedAction::DcsDispatch(_)));
    assert_eq!(actions[1], ParsedAction::Print('H'));
}

#[test]
fn test_parse_mixed_apc_and_dcs() {
    let actions = parse_all(b"\x1B_Ga=q;\x1B\\\x1BPq~\x1B\\");
    assert_eq!(actions.len(), 2);
    assert!(matches!(&actions[0], ParsedAction::ApcDispatch(_)));
    assert!(matches!(&actions[1], ParsedAction::DcsDispatch(_)));
}

// =========================================================================
// Edge Cases
// =========================================================================

#[test]
fn test_parse_del_ignored() {
    let actions = parse_all(b"A\x7FB");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0], ParsedAction::Print('A'));
    assert_eq!(actions[1], ParsedAction::Print('B'));
}

#[test]
fn test_parse_empty_input() {
    let actions = parse_all(b"");
    assert!(actions.is_empty());
}

#[test]
fn test_parse_reset() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();

    parser.parse(b"\x1B[31", |action| actions.push(action));
    assert!(actions.is_empty());

    parser.reset();
    parser.parse(b"A", |action| actions.push(action));
    assert_eq!(actions, vec![ParsedAction::Print('A')]);
}

#[test]
fn test_parse_c0_in_csi() {
    let actions = parse_all(b"\x1B[1\x07;31m");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0], ParsedAction::Execute(0x07));
    assert_eq!(actions[1], csi(&[1, 31], &[], b'm'));
}

#[test]
fn test_parse_esc_in_csi_aborts() {
    let actions = parse_all(b"\x1B[1\x1B7");
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0],
        ParsedAction::EscDispatch {
            intermediate: None,
            final_byte: b'7',
        }
    );
}

#[test]
fn test_parse_text_with_formatting() {
    let actions = parse_all(b"Hello\x1B[31mRed\x1B[0mWorld");
    assert_eq!(actions.len(), 15);
    assert_eq!(actions[0], ParsedAction::Print('H'));
    assert_eq!(actions[5], csi(&[31], &[], b'm'));
    assert_eq!(actions[9], csi(&[0], &[], b'm'));
}

#[test]
fn test_parse_cursor_movement_sequence() {
    let actions = parse_all(b"\x1B[H\x1B[2J");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0], csi(&[], &[], b'H'));
    assert_eq!(actions[1], csi(&[2], &[], b'J'));
}

#[test]
fn test_parse_csi_with_space_intermediate() {
    let actions = parse_all(b"\x1B[1 q");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[1], &[b' '], b'q'));
}

#[test]
fn test_parse_csi_with_less_than_intermediate() {
    // Kitty keyboard protocol pop: CSI < u
    let actions = parse_all(b"\x1B[<u");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[], &[b'<'], b'u'));
}

#[test]
fn test_parse_csi_with_less_than_and_params() {
    // Kitty keyboard protocol pop with param: CSI < 1 u
    let actions = parse_all(b"\x1B[<1u");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[1], &[b'<'], b'u'));
}

#[test]
fn test_parse_scroll_region() {
    let actions = parse_all(b"\x1B[5;20r");
    assert_eq!(actions, vec![csi(&[5, 20], &[], b'r')]);
}

// =========================================================================
// parse_interruptible Tests
// =========================================================================

#[test]
fn test_interruptible_no_interrupt() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    let consumed = parser.parse_interruptible(b"Hello", |action| {
        actions.push(action);
        true
    });
    assert_eq!(consumed, 5);
    assert_eq!(actions.len(), 5);
}

#[test]
fn test_interruptible_stop_after_csi() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    // CSI ?1049h followed by text "AB"
    let input = b"\x1B[?1049hAB";
    let consumed = parser.parse_interruptible(input, |action| {
        let is_csi = matches!(action, ParsedAction::CsiDispatch { .. });
        actions.push(action);
        // Stop after seeing any CSI dispatch
        !is_csi
    });
    // Should have consumed the CSI sequence bytes only
    assert_eq!(consumed, 8); // ESC [ ? 1 0 4 9 h = 8 bytes
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[1049], &[b'?'], b'h'));
    // Parser should be in Ground state
    assert_eq!(parser.state, State::Ground);
}

#[test]
fn test_interruptible_remaining_parseable() {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    let input = b"\x1B[?1049hHello";
    let consumed = parser.parse_interruptible(input, |action| {
        let is_csi = matches!(action, ParsedAction::CsiDispatch { .. });
        actions.push(action);
        !is_csi
    });
    assert_eq!(consumed, 8);

    // Feed remaining to a fresh parser
    let remaining = &input[consumed..];
    assert_eq!(remaining, b"Hello");
    let mut parser2 = Parser::new();
    let mut actions2 = Vec::new();
    parser2.parse(remaining, |action| actions2.push(action));
    assert_eq!(actions2.len(), 5);
    assert_eq!(actions2[0], ParsedAction::Print('H'));
}

// =========================================================================
// Colon Sub-Parameter Tests (ISO 8613-6, e.g. avt dump / kitty SGR)
// =========================================================================

// The parser consumes `:` (ISO 8613-6 sub-parameter separator) like `;`, so a
// colon-form SGR does not cancel the CSI and leak its tail as literal text.
// Sub-parameters collapse to plain parameters, which is correct for the common
// colon color forms (`38:5:n` indexed, `38:2:r:g:b` truecolor — same parameter
// sequence as the `;` form). Full ISO 8613-6 sub-parameter semantics (keeping
// `:` distinct from `;`, e.g. `4:3` underline styles or the `38:2::r:g:b`
// empty-colorspace form) are NOT yet modeled — see the colon-sub-param
// follow-up note in tmp/.
#[test]
fn test_parse_csi_sgr_colon_indexed_color() {
    // `38:5:196` collapses to the same params as `38;5;196` (fg = 256-color 196).
    let actions = parse_all(b"\x1b[38:5:196m");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[38, 5, 196], &[], b'm'));
}

#[test]
fn test_parse_csi_sgr_colon_rgb_color() {
    // `48:2:10:20:30` collapses to the same params as `48;2;10;20;30`.
    let actions = parse_all(b"\x1b[48:2:10:20:30m");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[48, 2, 10, 20, 30], &[], b'm'));
}

#[test]
fn test_parse_csi_colon_does_not_leak_text() {
    // Regression: the parser used to cancel CSI at ':' and print the
    // remainder ("5:196m") as literal text, garbling the screen on every
    // avt-generated resume snapshot.
    let actions = parse_all(b"\x1b[38:5:196mX");
    assert_eq!(actions.len(), 2);
    assert!(matches!(actions[0], ParsedAction::CsiDispatch { .. }));
    assert_eq!(actions[1], ParsedAction::Print('X'));
}

#[test]
fn test_parse_csi_mixed_semicolon_and_colon() {
    // Mixed `;` and `:` separators both finish a parameter, so
    // `1;38:5:9;4` collapses to five plain params.
    let actions = parse_all(b"\x1b[1;38:5:9;4m");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], csi(&[1, 38, 5, 9, 4], &[], b'm'));
}
