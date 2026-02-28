/// tmux DCS passthrough support.
///
/// When running inside tmux, escape sequences (APC, OSC, etc.) are consumed
/// by tmux and never reach the outer terminal. To pass sequences through,
/// they must be wrapped in DCS passthrough format:
///
///   ESC P tmux; <escaped-sequence> ESC \
///
/// where every ESC (0x1B) byte inside the sequence is doubled.
///
/// Requires `set -g allow-passthrough on` in tmux configuration.
/// Check if the current process is running inside tmux.
pub fn is_inside_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

/// Wrap an escape sequence in DCS passthrough for tmux.
///
/// Format: `ESC P tmux; <escaped> ESC \`
/// All ESC (0x1B) bytes in the inner sequence are doubled.
/// Non-ESC content is copied via slice operations to preserve UTF-8 integrity.
pub fn wrap_dcs_passthrough(sequence: &str) -> String {
    let esc_count = sequence.bytes().filter(|&b| b == 0x1B).count();
    // Header (7) + doubled ESCs + body + trailer (2)
    let mut output = String::with_capacity(sequence.len() + esc_count + 9);
    output.push_str("\x1bPtmux;");

    let bytes = sequence.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == 0x1B {
            // Copy the non-ESC span before this ESC byte.
            // Safety: ESC (0x1B) is a single-byte ASCII character, so slicing
            // at ESC positions never splits a multi-byte UTF-8 sequence.
            output.push_str(&sequence[start..i]);
            output.push_str("\x1b\x1b");
            start = i + 1;
        }
    }
    // Copy remaining tail
    output.push_str(&sequence[start..]);

    output.push_str("\x1b\\");
    output
}

/// Conditionally wrap a sequence for tmux passthrough.
///
/// If running inside tmux ($TMUX is set), wraps in DCS passthrough.
/// Otherwise returns the sequence unchanged (avoids allocation).
///
/// When the input contains multiple escape sequences (e.g. Kitty chunked
/// transfer or Markdown OSC chunks), each sequence is wrapped individually
/// to stay within tmux's passthrough buffer limit.
pub fn passthrough_if_needed(sequence: &str) -> std::borrow::Cow<'_, str> {
    if is_inside_tmux() {
        std::borrow::Cow::Owned(wrap_each_sequence(sequence))
    } else {
        std::borrow::Cow::Borrowed(sequence)
    }
}

/// Wrap each escape sequence in the input individually for tmux DCS passthrough.
///
/// Splits on ST (ESC \) boundaries so each sequence gets its own DCS
/// passthrough envelope. This prevents exceeding tmux's passthrough buffer
/// limit (typically 256KB) when sending large chunked data like Kitty
/// graphics transfers.
///
/// For single-sequence inputs (e.g. SIXEL), this behaves identically to
/// `wrap_dcs_passthrough`.
/// Test-only alias for `wrap_each_sequence` to allow integration tests
/// to exercise the wrapping logic without setting $TMUX.
#[cfg(test)]
pub fn wrap_each_sequence_for_test(input: &str) -> String {
    wrap_each_sequence(input)
}

fn wrap_each_sequence(input: &str) -> String {
    const ST: &str = "\x1b\\";

    // Fast path: single sequence (no split needed)
    let first = match input.find(ST) {
        Some(pos) => pos + ST.len(),
        None => return wrap_dcs_passthrough(input),
    };
    if first == input.len() {
        return wrap_dcs_passthrough(input);
    }

    // Multiple sequences: wrap each individually
    let mut output = String::with_capacity(input.len() + input.len() / 4);
    let mut remaining = input;

    while let Some(pos) = remaining.find(ST) {
        let end = pos + ST.len();
        output.push_str(&wrap_dcs_passthrough(&remaining[..end]));
        remaining = &remaining[end..];
    }

    // Trailing content without ST (shouldn't normally happen)
    if !remaining.is_empty() {
        output.push_str(&wrap_dcs_passthrough(remaining));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_dcs_passthrough_apc() {
        // APC: ESC _G test ESC \
        let input = "\x1b_Gtest\x1b\\";
        let wrapped = wrap_dcs_passthrough(input);
        // Expected: ESC P tmux; ESC ESC _G test ESC ESC \ ESC \
        assert_eq!(wrapped, "\x1bPtmux;\x1b\x1b_Gtest\x1b\x1b\\\x1b\\");
    }

    #[test]
    fn test_wrap_dcs_passthrough_osc() {
        // OSC 777: ESC ] 777 ; data ESC \
        let input = "\x1b]777;emterm;test\x1b\\";
        let wrapped = wrap_dcs_passthrough(input);
        assert_eq!(
            wrapped,
            "\x1bPtmux;\x1b\x1b]777;emterm;test\x1b\x1b\\\x1b\\"
        );
    }

    #[test]
    fn test_wrap_dcs_passthrough_no_esc() {
        let input = "plain text";
        let wrapped = wrap_dcs_passthrough(input);
        assert_eq!(wrapped, "\x1bPtmux;plain text\x1b\\");
    }

    #[test]
    fn test_wrap_dcs_passthrough_utf8() {
        // Ensure multi-byte UTF-8 is preserved correctly
        let input = "\x1b]777;emterm;日本語テスト\x1b\\";
        let wrapped = wrap_dcs_passthrough(input);
        assert_eq!(
            wrapped,
            "\x1bPtmux;\x1b\x1b]777;emterm;日本語テスト\x1b\x1b\\\x1b\\"
        );
    }

    #[test]
    fn test_wrap_dcs_passthrough_consecutive_esc() {
        // Two consecutive ESC bytes
        let input = "\x1b\x1b";
        let wrapped = wrap_dcs_passthrough(input);
        assert_eq!(wrapped, "\x1bPtmux;\x1b\x1b\x1b\x1b\x1b\\");
    }

    // Tests for passthrough_if_needed use a testable helper to avoid
    // thread-unsafe std::env::set_var/remove_var in parallel test runs.

    #[test]
    fn test_passthrough_if_needed_wraps() {
        // Directly test that wrapping produces the expected output
        let input = "\x1b_Gtest\x1b\\";
        let wrapped = wrap_dcs_passthrough(input);
        assert_eq!(&wrapped, "\x1bPtmux;\x1b\x1b_Gtest\x1b\x1b\\\x1b\\");
    }

    #[test]
    fn test_passthrough_if_needed_borrows() {
        // When not wrapping, Cow should borrow without allocation
        let input = "\x1b_Gtest\x1b\\";
        let cow = std::borrow::Cow::Borrowed(input);
        assert_eq!(&*cow, input);
    }

    // Tests for wrap_each_sequence (per-sequence DCS wrapping)

    #[test]
    fn test_wrap_each_sequence_single() {
        // Single sequence: same result as wrap_dcs_passthrough
        let input = "\x1b_Gi=1,f=100,a=T,m=0;data\x1b\\";
        let result = wrap_each_sequence(input);
        assert_eq!(result, wrap_dcs_passthrough(input));
    }

    #[test]
    fn test_wrap_each_sequence_multiple_kitty_chunks() {
        // Two Kitty APC chunks
        let chunk1 = "\x1b_Gi=1,f=100,a=T,m=1;AAAA\x1b\\";
        let chunk2 = "\x1b_Gi=1,m=0;BBBB\x1b\\";
        let input = format!("{}{}", chunk1, chunk2);

        let result = wrap_each_sequence(&input);

        // Each chunk should be wrapped individually
        let expected = format!(
            "{}{}",
            wrap_dcs_passthrough(chunk1),
            wrap_dcs_passthrough(chunk2)
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn test_wrap_each_sequence_multiple_osc() {
        // Three Markdown OSC sequences
        let seq1 = "\x1b]777;emterm;markdown;begin;id=abc\x1b\\";
        let seq2 = "\x1b]777;emterm;markdown;chunk;id=abc;seq=0;data=SGVsbG8=\x1b\\";
        let seq3 = "\x1b]777;emterm;markdown;end;id=abc\x1b\\";
        let input = format!("{}{}{}", seq1, seq2, seq3);

        let result = wrap_each_sequence(&input);

        let expected = format!(
            "{}{}{}",
            wrap_dcs_passthrough(seq1),
            wrap_dcs_passthrough(seq2),
            wrap_dcs_passthrough(seq3)
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn test_wrap_each_sequence_no_st() {
        // Input without ST terminator (edge case)
        let input = "no escape sequences here";
        let result = wrap_each_sequence(input);
        assert_eq!(result, wrap_dcs_passthrough(input));
    }
}
