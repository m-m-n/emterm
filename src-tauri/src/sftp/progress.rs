//! SFTP stdout/stderr line parsing for transfer progress and error extraction.
//!
//! Parses sftp output to extract transfer progress information.
//! sftp progress output format varies across versions, so parsing is conservative.
//!
//! Note: When sftp runs in batch mode (`-b -`), progress bars are typically
//! suppressed because stderr is not a tty. `parse_error_line` is used by
//! `upload.rs` for stderr error detection. `parse_progress_line` is available
//! for future use if interactive-mode upload is implemented.

/// Parsed progress information from an sftp output line.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressInfo {
    pub percent: u8,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
}

/// Parse an sftp output line for transfer progress.
///
/// sftp progress lines typically look like:
/// ```text
/// file.txt                              100%   1234     1.2KB/s   00:01
/// file.txt                               50%   617      1.2KB/s   00:00
/// ```
///
/// Returns `Some(ProgressInfo)` if the line contains progress information,
/// `None` if the line is not a progress line.
pub fn parse_progress_line(line: &str) -> Option<ProgressInfo> {
    // Look for percentage pattern: digits followed by %
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Find percentage: look for pattern like "100%" or " 50%"
    let percent_idx = trimmed.find('%')?;
    if percent_idx == 0 {
        return None;
    }

    // Extract the number before %
    let before_percent = &trimmed[..percent_idx];
    let percent_str = before_percent
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()?;

    if percent_str.is_empty() {
        return None;
    }

    let percent: u8 = percent_str.parse().ok()?;
    if percent > 100 {
        return None;
    }

    // Try to extract byte count after the percentage
    let after_percent = &trimmed[percent_idx + 1..];
    let bytes_transferred = parse_byte_count(after_percent).unwrap_or(0);

    // Estimate total bytes from percentage
    let total_bytes = if percent > 0 && bytes_transferred > 0 {
        (bytes_transferred as f64 / percent as f64 * 100.0) as u64
    } else {
        0
    };

    Some(ProgressInfo {
        percent,
        bytes_transferred,
        total_bytes,
    })
}

/// Parse a byte count from sftp output.
///
/// Handles formats like "1234", "1.2KB", "1.5MB", "2.3GB"
fn parse_byte_count(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Split on whitespace and take the first token that looks like a size
    for token in trimmed.split_whitespace() {
        if let Some(bytes) = parse_size_token(token) {
            return Some(bytes);
        }
    }

    None
}

/// Parse a single size token like "1234", "1.2KB", "1.5MB", "2.3GB"
fn parse_size_token(token: &str) -> Option<u64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    // Find where the numeric part ends
    let num_end = token
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(token.len());

    if num_end == 0 {
        return None;
    }

    let num_str = &token[..num_end];
    let suffix = token[num_end..].to_ascii_uppercase();

    let value: f64 = num_str.parse().ok()?;

    let multiplier: f64 = match suffix.as_str() {
        "" | "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };

    Some((value * multiplier) as u64)
}

/// Check if an sftp output line indicates an error.
///
/// Returns the error message if the line is an error, None otherwise.
pub fn parse_error_line(line: &str) -> Option<String> {
    let trimmed = line.trim();

    // Common sftp error prefixes
    if trimmed.starts_with("Couldn't")
        || trimmed.starts_with("Permission denied")
        || trimmed.starts_with("No such file")
        || trimmed.starts_with("Connection closed")
        || trimmed.starts_with("ssh:")
        || trimmed.starts_with("sftp>") && trimmed.contains("not found")
    {
        return Some(trimmed.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_progress_line_100_percent() {
        let line = "file.txt                              100%   1234     1.2KB/s   00:01";
        let result = parse_progress_line(line);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.percent, 100);
    }

    #[test]
    fn test_parse_progress_line_partial() {
        let line = "file.txt                               50%   617      1.2KB/s   00:00";
        let result = parse_progress_line(line);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.percent, 50);
    }

    #[test]
    fn test_parse_progress_line_zero_percent() {
        let line = "file.txt                                0%    0     0.0KB/s   --:--";
        let result = parse_progress_line(line);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.percent, 0);
    }

    #[test]
    fn test_parse_progress_line_not_progress() {
        assert!(parse_progress_line("sftp> put file.txt").is_none());
        assert!(parse_progress_line("Connected to host").is_none());
        assert!(parse_progress_line("").is_none());
    }

    #[test]
    fn test_parse_progress_line_invalid_percent() {
        // 200% should be rejected
        let line = "file.txt  200%   1234   1.2KB/s";
        assert!(parse_progress_line(line).is_none());
    }

    #[test]
    fn test_parse_size_token_bytes() {
        assert_eq!(parse_size_token("1234"), Some(1234));
        assert_eq!(parse_size_token("0"), Some(0));
    }

    #[test]
    fn test_parse_size_token_kb() {
        assert_eq!(parse_size_token("1KB"), Some(1024));
        assert_eq!(parse_size_token("1.5KB"), Some(1536));
    }

    #[test]
    fn test_parse_size_token_mb() {
        assert_eq!(parse_size_token("1MB"), Some(1048576));
        assert_eq!(parse_size_token("2.5MB"), Some(2621440));
    }

    #[test]
    fn test_parse_size_token_gb() {
        assert_eq!(parse_size_token("1GB"), Some(1073741824));
    }

    #[test]
    fn test_parse_size_token_invalid() {
        assert!(parse_size_token("").is_none());
        assert!(parse_size_token("abc").is_none());
        assert!(parse_size_token("TB").is_none());
    }

    #[test]
    fn test_parse_error_line_permission_denied() {
        let line = "Permission denied (publickey)";
        let result = parse_error_line(line);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Permission denied"));
    }

    #[test]
    fn test_parse_error_line_no_such_file() {
        let line = "No such file or directory";
        assert!(parse_error_line(line).is_some());
    }

    #[test]
    fn test_parse_error_line_couldnt() {
        let line = "Couldn't stat remote file";
        assert!(parse_error_line(line).is_some());
    }

    #[test]
    fn test_parse_error_line_not_error() {
        assert!(parse_error_line("Connected to host").is_none());
        assert!(parse_error_line("").is_none());
        assert!(parse_error_line("file.txt  100%  1234").is_none());
    }

    #[test]
    fn test_parse_error_line_ssh_error() {
        let line = "ssh: connect to host example.com port 22: Connection refused";
        assert!(parse_error_line(line).is_some());
    }

    #[test]
    fn test_parse_progress_with_bytes_after_percent() {
        let line = "data.tar.gz                           75%   150MB   5.2MB/s   00:10";
        let result = parse_progress_line(line);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.percent, 75);
        assert!(info.bytes_transferred > 0);
    }
}
