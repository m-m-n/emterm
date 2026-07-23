//! Locale-aware string helpers for the CLI subcommand surface.
//!
//! Mirrors the legacy `src-tauri/locales/{en,ja}.json` `cli.*` and
//! `error.*` keys. The native-poc binary deliberately does not depend on
//! `rust-i18n`; per-call `Locale` dispatch is used instead.
//!
//! Each helper takes a `Locale` argument so test code can bypass the
//! cached active locale entirely.

use crate::i18n::Locale;
use std::path::Path;

/// Escape C0 / DEL / C1 control characters in any user-influenced string
/// that we are about to write to stderr.
///
/// eMterm runs error output through the user's terminal — an attacker
/// who can influence a `--protocol` value, a filename, or any other
/// value interpolated into an error message could otherwise inject raw
/// OSC / APC / CSI sequences (clipboard manipulation, terminal state
/// changes, …). Escape every control character to a visible `\xNN` /
/// `\u{NNNN}` form so it can never act as a terminal control.
pub fn escape_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let code = ch as u32;
        if ch.is_control() || (0x80..=0x9F).contains(&code) {
            if code < 0x80 {
                out.push_str(&format!("\\x{:02x}", code));
            } else {
                out.push_str(&format!("\\u{{{:x}}}", code));
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn escape_path(path: &Path) -> String {
    escape_control_chars(&path.display().to_string())
}

// ---------------------------------------------------------------------
// error.* messages
// ---------------------------------------------------------------------

pub fn err_file_not_found(loc: Locale, path: &Path) -> String {
    let p = escape_path(path);
    match loc {
        Locale::En => format!("File not found: {}", p),
        Locale::Ja => format!("ファイルが見つかりません: {}", p),
    }
}

pub fn err_not_a_file(loc: Locale, path: &Path) -> String {
    let p = escape_path(path);
    match loc {
        Locale::En => format!("Path is not a file: {}", p),
        Locale::Ja => format!("ファイルではありません: {}", p),
    }
}

pub fn err_file_read_error(loc: Locale, error: &std::io::Error) -> String {
    let e = escape_control_chars(&error.to_string());
    match loc {
        Locale::En => format!("Failed to read file: {}", e),
        Locale::Ja => format!("ファイルの読み込みに失敗しました: {}", e),
    }
}

pub fn err_file_too_large(loc: Locale, size: u64, max_size: u64) -> String {
    match loc {
        Locale::En => format!(
            "File size ({} bytes) exceeds {} bytes limit",
            size, max_size
        ),
        Locale::Ja => format!(
            "ファイルサイズ ({}バイト) が{}バイトの制限を超えています",
            size, max_size
        ),
    }
}

pub fn err_unsupported_image_format(loc: Locale, format: image::ImageFormat) -> String {
    match loc {
        Locale::En => format!("Unsupported image format: {:?}", format),
        Locale::Ja => format!("サポートされていない画像形式: {:?}", format),
    }
}

pub fn err_image_decode_error(loc: Locale, error: &image::ImageError) -> String {
    let e = escape_control_chars(&error.to_string());
    match loc {
        Locale::En => format!("Failed to decode image: {}", e),
        Locale::Ja => format!("画像のデコードに失敗しました: {}", e),
    }
}

pub fn err_invalid_protocol(loc: Locale, protocol: &str) -> String {
    let p = escape_control_chars(protocol);
    match loc {
        Locale::En => format!("Invalid protocol: {}", p),
        Locale::Ja => format!("無効なプロトコル: {}", p),
    }
}

pub fn err_encoding_error(loc: Locale, error: &str) -> String {
    let e = escape_control_chars(error);
    match loc {
        Locale::En => format!("Encoding error: {}", e),
        Locale::Ja => format!("エンコードエラー: {}", e),
    }
}

pub fn err_name_required(loc: Locale) -> String {
    match loc {
        Locale::En => "--name is required when reading from stdin".to_string(),
        Locale::Ja => "標準入力から読み込む場合は --name が必要です".to_string(),
    }
}

pub fn err_permission_denied(loc: Locale, path: &Path) -> String {
    let p = escape_path(path);
    match loc {
        Locale::En => format!("Permission denied: {}", p),
        Locale::Ja => format!("アクセスが拒否されました: {}", p),
    }
}

pub fn err_unsupported_extension(loc: Locale, path: &Path, allowed: &[&str]) -> String {
    let p = escape_path(path);
    let list = allowed.join(", ");
    match loc {
        Locale::En => format!(
            "Unsupported file extension: {} (expected one of: {})",
            p, list
        ),
        Locale::Ja => format!(
            "サポートされていない拡張子です: {} (対応拡張子: {})",
            p, list
        ),
    }
}

// ---------------------------------------------------------------------
// cli.* messages (clap help text)
// ---------------------------------------------------------------------

pub fn cli_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "eMterm - Modern terminal emulator with rich rendering",
        Locale::Ja => "eMterm - リッチレンダリング対応モダンターミナルエミュレータ",
    }
}

pub fn cli_markdown_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Display Markdown file in eMterm",
        Locale::Ja => "eMtermでMarkdownファイルを表示",
    }
}

pub fn cli_markdown_file(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Path to Markdown file",
        Locale::Ja => "Markdownファイルのパス",
    }
}

pub fn cli_json_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Display JSON file in eMterm",
        Locale::Ja => "eMtermでJSONファイルを表示",
    }
}

pub fn cli_json_file(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Path to JSON file",
        Locale::Ja => "JSONファイルのパス",
    }
}

pub fn cli_yaml_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Display YAML file in eMterm",
        Locale::Ja => "eMtermでYAMLファイルを表示",
    }
}

pub fn cli_yaml_file(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Path to YAML file",
        Locale::Ja => "YAMLファイルのパス",
    }
}

pub fn cli_html_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Display HTML file in eMterm",
        Locale::Ja => "eMtermでHTMLファイルを表示",
    }
}

pub fn cli_html_file(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Path to HTML file",
        Locale::Ja => "HTMLファイルのパス",
    }
}

pub fn cli_image_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Display image file in eMterm",
        Locale::Ja => "eMtermで画像ファイルを表示",
    }
}

pub fn cli_image_file(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Path to image file",
        Locale::Ja => "画像ファイルのパス",
    }
}

pub fn cli_image_protocol(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Image protocol to use",
        Locale::Ja => "使用する画像プロトコル",
    }
}

pub fn cli_agent_status_about(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Report or clear this pane's agent status",
        Locale::Ja => "このペインのエージェント状態を報告・クリア",
    }
}

pub fn cli_agent_status_state(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "State to report (idle|working|blocked|done), or clear",
        Locale::Ja => "報告する状態 (idle|working|blocked|done)、またはclear",
    }
}

pub fn cli_agent_status_name(loc: Locale) -> &'static str {
    match loc {
        Locale::En => "Agent name to attach to the report",
        Locale::Ja => "報告に添付するエージェント名",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn file_not_found_en_includes_path() {
        let msg = err_file_not_found(Locale::En, &PathBuf::from("missing.txt"));
        assert!(msg.contains("File not found"));
        assert!(msg.contains("missing.txt"));
    }

    #[test]
    fn file_not_found_ja_includes_path() {
        let msg = err_file_not_found(Locale::Ja, &PathBuf::from("test.txt"));
        assert!(msg.contains("test.txt"));
        assert!(msg.contains("ファイルが見つかりません"));
    }

    #[test]
    fn file_too_large_en_includes_sizes() {
        let msg = err_file_too_large(Locale::En, 3_000_000, 2_000_000);
        assert!(msg.contains("exceeds"));
        assert!(msg.contains("3000000"));
        assert!(msg.contains("2000000"));
    }

    #[test]
    fn unsupported_extension_en_includes_path_and_allowed() {
        let msg = err_unsupported_extension(Locale::En, &PathBuf::from("f.txt"), &["html", "htm"]);
        assert!(msg.contains("f.txt"));
        assert!(msg.contains("html"));
        assert!(msg.contains("htm"));
    }

    #[test]
    fn unsupported_extension_ja_includes_path() {
        let msg = err_unsupported_extension(Locale::Ja, &PathBuf::from("f.txt"), &["html", "htm"]);
        assert!(msg.contains("f.txt"));
        assert!(msg.contains("サポートされていない拡張子です"));
    }

    #[test]
    fn invalid_protocol_includes_value() {
        let msg = err_invalid_protocol(Locale::En, "ascii");
        assert!(msg.contains("Invalid protocol"));
        assert!(msg.contains("ascii"));
    }

    #[test]
    fn escape_control_chars_replaces_c0() {
        let s = "\x1b]777;evil\x07";
        let escaped = escape_control_chars(s);
        assert!(!escaped.contains('\x1b'));
        assert!(!escaped.contains('\x07'));
        assert!(escaped.contains("\\x1b"));
        assert!(escaped.contains("\\x07"));
    }

    #[test]
    fn escape_control_chars_preserves_printable() {
        let s = "ascii_と日本語_AB12";
        assert_eq!(escape_control_chars(s), s);
    }

    #[test]
    fn err_invalid_protocol_escapes_control_chars() {
        let payload = "evil\x1b]52;c;ZXZpbA==\x07";
        let msg = err_invalid_protocol(Locale::En, payload);
        // Raw ESC / BEL MUST NOT appear in the output.
        assert!(!msg.contains('\x1b'));
        assert!(!msg.contains('\x07'));
        // The escaped form is present so the user can still see what was given.
        assert!(msg.contains("\\x1b"));
    }

    #[test]
    fn err_file_not_found_escapes_control_chars_in_path() {
        let p = PathBuf::from("inj\x1b]0;hacked\x07.md");
        let msg = err_file_not_found(Locale::En, &p);
        assert!(!msg.contains('\x1b'));
        assert!(!msg.contains('\x07'));
        assert!(msg.contains("\\x1b"));
    }
}
