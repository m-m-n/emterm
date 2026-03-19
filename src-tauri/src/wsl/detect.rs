//! WSL distribution detection utilities.
//!
//! Detects installed WSL distributions on Windows via `wsl.exe --list --quiet`.

/// Lists installed WSL distributions by executing `wsl.exe --list --quiet`.
///
/// # Returns
///
/// A vector of distribution names. Returns an empty vector if WSL is not
/// installed, no distributions are found, or the command fails.
///
/// # Platform
///
/// This function is only available on Windows. On Linux, the Tauri command
/// that wraps this is not registered.
#[cfg(windows)]
pub fn list_distributions() -> Vec<String> {
    let output = match std::process::Command::new("wsl.exe")
        .args(["--list", "--quiet"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    parse_wsl_output(&output.stdout)
}

/// Parses the raw output from `wsl --list --quiet`.
///
/// The output may be UTF-16LE encoded (common on Windows) or UTF-8.
/// Handles BOM, null bytes, empty lines, and trailing whitespace.
pub fn parse_wsl_output(raw: &[u8]) -> Vec<String> {
    let text = decode_wsl_output(raw);

    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

/// Decodes raw bytes from WSL output, handling UTF-16LE and UTF-8.
fn decode_wsl_output(raw: &[u8]) -> String {
    // Check for UTF-16LE BOM (FF FE)
    if raw.len() >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
        return decode_utf16le(&raw[2..]);
    }

    // Heuristic: wsl.exe --list --quiet on Windows produces UTF-16LE output.
    // This handles the BOM-less case by checking for the null-byte pattern
    // typical of ASCII-range text in UTF-16LE: [char, 0x00, char, 0x00, ...].
    // WSL distro names conventionally start with ASCII letters, so this is reliable.
    if raw.len() >= 4 && raw[1] == 0x00 && raw[3] == 0x00 {
        return decode_utf16le(raw);
    }

    // Assume UTF-8, strip BOM if present
    let text = String::from_utf8_lossy(raw);
    text.strip_prefix('\u{FEFF}').unwrap_or(&text).to_string()
}

/// Decodes UTF-16LE bytes to a String.
/// Invalid surrogate pairs are replaced with U+FFFD (replacement character)
/// to make data loss visible rather than silent.
fn decode_utf16le(raw: &[u8]) -> String {
    let u16_iter = raw
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));

    char::decode_utf16(u16_iter)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_utf8_output() {
        let input = b"Ubuntu-22.04\nDebian\n";
        let result = parse_wsl_output(input);
        assert_eq!(result, vec!["Ubuntu-22.04", "Debian"]);
    }

    #[test]
    fn test_parse_utf16le_with_bom() {
        // UTF-16LE BOM + "Ubuntu\r\n"
        let mut data: Vec<u8> = vec![0xFF, 0xFE]; // BOM
        for ch in "Ubuntu\r\n".encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let result = parse_wsl_output(&data);
        assert_eq!(result, vec!["Ubuntu"]);
    }

    #[test]
    fn test_parse_utf16le_without_bom() {
        // UTF-16LE without BOM: "Debian\r\n"
        let mut data: Vec<u8> = Vec::new();
        for ch in "Debian\r\n".encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let result = parse_wsl_output(&data);
        assert_eq!(result, vec!["Debian"]);
    }

    #[test]
    fn test_parse_multiple_distros_utf16le() {
        let mut data: Vec<u8> = vec![0xFF, 0xFE]; // BOM
        for ch in "Ubuntu-22.04\r\nDebian\r\nArchLinux\r\n".encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let result = parse_wsl_output(&data);
        assert_eq!(result, vec!["Ubuntu-22.04", "Debian", "ArchLinux"]);
    }

    #[test]
    fn test_parse_empty_output() {
        let result = parse_wsl_output(b"");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_only_whitespace() {
        let result = parse_wsl_output(b"\n\n  \n");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_filters_empty_lines() {
        let input = b"Ubuntu\n\n\nDebian\n\n";
        let result = parse_wsl_output(input);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn test_parse_trims_whitespace() {
        let input = b"  Ubuntu  \n  Debian  \n";
        let result = parse_wsl_output(input);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn test_parse_handles_crlf() {
        let input = b"Ubuntu\r\nDebian\r\n";
        let result = parse_wsl_output(input);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn test_parse_utf8_with_bom() {
        let input = b"\xEF\xBB\xBFUbuntu\nDebian\n";
        let result = parse_wsl_output(input);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn test_parse_distro_name_with_spaces() {
        // WSL distro names can technically have spaces
        let input = b"My Custom Distro\nUbuntu\n";
        let result = parse_wsl_output(input);
        assert_eq!(result, vec!["My Custom Distro", "Ubuntu"]);
    }

    #[test]
    fn test_decode_utf16le_basic() {
        let mut data: Vec<u8> = Vec::new();
        for ch in "Hello".encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        assert_eq!(decode_utf16le(&data), "Hello");
    }
}
