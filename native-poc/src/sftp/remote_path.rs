//! OSC 7 CWD → remote upload directory, and local-path paste formatting.
//!
//! For an SFTP drop on an SSH tab the remote destination is derived from the
//! tab's most recent OSC 7 CWD (a `file://host/path` URI or a bare path). For
//! a drop on a non-SSH tab the local paths are formatted for a terminal paste.

/// Derive a remote directory from an OSC 7 CWD string.
///
/// Accepts either a `file://[host]/path` URI (percent-decoded) or a bare path.
/// Returns the decoded directory, or an empty string when the input is empty.
pub fn extract_remote_path(cwd: &str) -> String {
    let s = cwd.trim();
    if s.is_empty() {
        return String::new();
    }

    let path = if let Some(rest) = s.strip_prefix("file://") {
        // file://[host]/path → drop the optional host segment.
        match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => rest,
        }
    } else {
        s
    };

    percent_decode(path)
}

/// Format local paths for a terminal paste: space-joined and shell-safe.
///
/// Each path is POSIX single-quote escaped so that shell metacharacters
/// (`$`, `` ` ``, `*`, `~`, quotes, …) are inserted literally and cannot run a
/// command when the user later submits the line. Paths made up solely of
/// shell-safe characters are emitted unquoted for readability.
///
/// Security: a path containing a control character (newline, carriage return,
/// NUL, …) is dropped entirely rather than written. A newline in particular
/// would otherwise be submitted as Enter at paste time and execute whatever
/// precedes it — single quoting alone cannot prevent that, so such paths are
/// refused outright. (A maliciously-named dropped file is the threat here.)
pub fn format_paths_for_paste(paths: &[String]) -> String {
    paths
        .iter()
        .filter(|p| !p.chars().any(|c| c.is_control()))
        .map(|p| shell_quote(p))
        .collect::<Vec<_>>()
        .join(" ")
}

/// POSIX single-quote a path for safe terminal paste. Returns the path unquoted
/// when it consists solely of shell-safe characters; otherwise wraps it in
/// single quotes, rendering any embedded `'` as `'\''`.
fn shell_quote(p: &str) -> String {
    let safe = !p.is_empty()
        && p.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '/' | '.' | '_' | '-' | '+' | '=' | ':' | ',' | '@' | '%')
        });
    if safe {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\'', "'\\''"))
    }
}

/// Whether a dropped path points at a directory (so the upload uses `put -r`).
/// A path that cannot be stat'd is treated as a file.
pub fn is_directory(path: &std::path::Path) -> bool {
    path.is_dir()
}

/// Percent-decode a URI path (`%20` → space, etc.). Bytes that are not a valid
/// `%XX` escape are passed through unchanged.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let len = bytes.len();
    while i < len {
        if bytes[i] == b'%' && i + 2 < len {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned())
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_remote_path_plain_path() {
        assert_eq!(extract_remote_path("/home/user/work"), "/home/user/work");
    }

    #[test]
    fn extract_remote_path_file_uri_with_host() {
        assert_eq!(
            extract_remote_path("file://myhost/home/user/work"),
            "/home/user/work"
        );
    }

    #[test]
    fn extract_remote_path_file_uri_no_host() {
        assert_eq!(extract_remote_path("file:///var/tmp"), "/var/tmp");
    }

    #[test]
    fn extract_remote_path_percent_decodes_spaces() {
        assert_eq!(
            extract_remote_path("file://host/home/My%20Documents"),
            "/home/My Documents"
        );
    }

    #[test]
    fn extract_remote_path_percent_decodes_non_ascii() {
        // %E6%97%A5%E6%9C%AC = "日本"
        assert_eq!(
            extract_remote_path("file://host/srv/%E6%97%A5%E6%9C%AC"),
            "/srv/日本"
        );
    }

    #[test]
    fn extract_remote_path_empty() {
        assert_eq!(extract_remote_path(""), "");
        assert_eq!(extract_remote_path("   "), "");
    }

    #[test]
    fn format_paths_for_paste_no_spaces() {
        let paths = vec!["/a/b.txt".to_string(), "/c/d.txt".to_string()];
        assert_eq!(format_paths_for_paste(&paths), "/a/b.txt /c/d.txt");
    }

    #[test]
    fn format_paths_for_paste_quotes_spaces() {
        let paths = vec!["/a/My File.txt".to_string(), "/c/d.txt".to_string()];
        assert_eq!(format_paths_for_paste(&paths), "'/a/My File.txt' /c/d.txt");
    }

    #[test]
    fn format_paths_for_paste_single() {
        let paths = vec!["/only.txt".to_string()];
        assert_eq!(format_paths_for_paste(&paths), "/only.txt");
    }

    #[test]
    fn format_paths_for_paste_empty() {
        assert_eq!(format_paths_for_paste(&[]), "");
    }

    #[test]
    fn format_paths_for_paste_neutralizes_shell_metacharacters() {
        // `$(...)`, backticks and `*` must be single-quoted so they are inserted
        // literally and never expand/execute when the line is submitted.
        let paths = vec!["/tmp/$(rm -rf ~)".to_string()];
        assert_eq!(format_paths_for_paste(&paths), "'/tmp/$(rm -rf ~)'");
        let paths = vec!["/tmp/`id`".to_string()];
        assert_eq!(format_paths_for_paste(&paths), "'/tmp/`id`'");
    }

    #[test]
    fn format_paths_for_paste_escapes_embedded_single_quote() {
        let paths = vec!["/tmp/it's a file".to_string()];
        assert_eq!(format_paths_for_paste(&paths), "'/tmp/it'\\''s a file'");
    }

    #[test]
    fn format_paths_for_paste_drops_paths_with_control_chars() {
        // A newline would be submitted as Enter at paste time and execute the
        // preceding text — such paths are refused outright (dropped).
        let paths = vec![
            "/tmp/safe.txt".to_string(),
            "/tmp/evil\nrm -rf ~".to_string(),
            "/tmp/also\rbad".to_string(),
            "/tmp/nul\0byte".to_string(),
        ];
        assert_eq!(format_paths_for_paste(&paths), "/tmp/safe.txt");
    }
}
