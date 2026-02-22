/// APC handler: detects Kitty graphics queries and responds synchronously.
///
/// Kitty query (`a=q`) responses must arrive before the DSR sentinel
/// that capability-detection libraries use as a read-stop signal.
/// By handling queries here (instead of routing through the async
/// Tauri backend), the response is written to the PTY in the same
/// synchronous pass as DA1/DSR responses.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    /// Try to handle an APC payload as a Kitty graphics query.
    ///
    /// Returns `true` if the payload was a Kitty query (`a=q`) and
    /// a synchronous response was generated. Returns `false` for all
    /// other APC payloads, which should be forwarded to the async path.
    pub(crate) fn try_handle_kitty_query(&mut self, payload: &[u8]) -> bool {
        // Must start with 'G' for Kitty Graphics Protocol
        if payload.first() != Some(&b'G') {
            return false;
        }

        let data = &payload[1..];

        // Find the control data portion (before ';' separator)
        let control_data = match data.iter().position(|&b| b == b';') {
            Some(pos) => &data[..pos],
            None => data,
        };

        // Parse key=value pairs to detect a=q and extract i=<id>, q=<quiet>
        let mut is_query = false;
        let mut image_id: Option<u32> = None;
        let mut quiet: Option<u8> = None;

        for pair in control_data.split(|&b| b == b',') {
            if let Some(eq_pos) = pair.iter().position(|&b| b == b'=') {
                let key = &pair[..eq_pos];
                let val = &pair[eq_pos + 1..];
                match key {
                    b"a" => {
                        if val == b"q" {
                            is_query = true;
                        }
                    }
                    b"i" => {
                        image_id = parse_u32_from_bytes(val);
                    }
                    b"q" => {
                        quiet = parse_u32_from_bytes(val).map(|v| v as u8);
                    }
                    _ => {}
                }
            }
        }

        if !is_query {
            return false;
        }

        // Check quiet suppression (q=1 suppresses OK responses)
        if quiet == Some(1) {
            return true; // Query handled, but response suppressed
        }

        // Build response: ESC _ G [i=<id>] ; OK ESC backslash
        let mut buf = [0u8; 32];
        buf[0] = 0x1B;
        buf[1] = b'_';
        buf[2] = b'G';
        let mut pos = 3;

        if let Some(id) = image_id {
            buf[pos] = b'i';
            pos += 1;
            buf[pos] = b'=';
            pos += 1;
            pos = write_u32_decimal(&mut buf, pos, id);
        }

        buf[pos] = b';';
        pos += 1;
        buf[pos] = b'O';
        pos += 1;
        buf[pos] = b'K';
        pos += 1;
        buf[pos] = 0x1B;
        pos += 1;
        buf[pos] = b'\\';
        pos += 1;

        // Write to response buffer and fire callback
        let len = pos.min(self.response_buffer.len());
        self.response_buffer[..len].copy_from_slice(&buf[..len]);
        self.response_len = len as u8;
        self.fire_device_response_callback();

        true
    }
}

/// Parse a u32 from ASCII decimal bytes.
fn parse_u32_from_bytes(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut result: u32 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(result)
}

/// Write a u32 as decimal digits to buffer, return new position.
fn write_u32_decimal(buf: &mut [u8], start: usize, val: u32) -> usize {
    if val == 0 {
        buf[start] = b'0';
        return start + 1;
    }
    let mut digits = [0u8; 10];
    let mut n = val;
    let mut count = 0;
    while n > 0 {
        digits[count] = (n % 10) as u8 + b'0';
        n /= 10;
        count += 1;
    }
    let mut pos = start;
    for i in (0..count).rev() {
        buf[pos] = digits[i];
        pos += 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    use super::*;

    #[test]
    fn test_kitty_query_with_id() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA";
        assert!(core.try_handle_kitty_query(payload));
        let response = core.get_response_bytes();
        assert_eq!(&response, b"\x1b_Gi=31;OK\x1b\\");
    }

    #[test]
    fn test_kitty_query_without_id() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=q;AAAA";
        assert!(core.try_handle_kitty_query(payload));
        let response = core.get_response_bytes();
        assert_eq!(&response, b"\x1b_G;OK\x1b\\");
    }

    #[test]
    fn test_kitty_query_quiet_suppressed() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Gi=31,a=q,q=1;AAAA";
        assert!(core.try_handle_kitty_query(payload));
        // Response suppressed, response_len should be 0
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_non_query_returns_false() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Transmit action (a=T) - should not be handled
        let payload = b"Ga=T,f=100;iVBORw0KGgo=";
        assert!(!core.try_handle_kitty_query(payload));
    }

    #[test]
    fn test_kitty_default_action_returns_false() {
        let mut core = TerminalCore::new(80, 24, 0);
        // No action specified (defaults to TransmitAndDisplay)
        let payload = b"Gf=100;iVBORw0KGgo=";
        assert!(!core.try_handle_kitty_query(payload));
    }

    #[test]
    fn test_non_kitty_apc_returns_false() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Hello";
        assert!(!core.try_handle_kitty_query(payload));
    }

    #[test]
    fn test_empty_payload_returns_false() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"";
        assert!(!core.try_handle_kitty_query(payload));
    }

    #[test]
    fn test_kitty_query_no_payload() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=q";
        assert!(core.try_handle_kitty_query(payload));
        let response = core.get_response_bytes();
        assert_eq!(&response, b"\x1b_G;OK\x1b\\");
    }

    #[test]
    fn test_kitty_query_large_id() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Gi=4294967295,a=q;";
        assert!(core.try_handle_kitty_query(payload));
        let response = core.get_response_bytes();
        assert_eq!(&response, b"\x1b_Gi=4294967295;OK\x1b\\");
    }

    #[test]
    fn test_parse_u32_from_bytes() {
        assert_eq!(parse_u32_from_bytes(b"0"), Some(0));
        assert_eq!(parse_u32_from_bytes(b"31"), Some(31));
        assert_eq!(parse_u32_from_bytes(b"4294967295"), Some(4294967295));
        assert_eq!(parse_u32_from_bytes(b""), None);
        assert_eq!(parse_u32_from_bytes(b"abc"), None);
        assert_eq!(parse_u32_from_bytes(b"12x"), None);
    }

    #[test]
    fn test_write_u32_decimal() {
        let mut buf = [0u8; 20];
        assert_eq!(write_u32_decimal(&mut buf, 0, 0), 1);
        assert_eq!(&buf[..1], b"0");

        let mut buf = [0u8; 20];
        assert_eq!(write_u32_decimal(&mut buf, 0, 31), 2);
        assert_eq!(&buf[..2], b"31");

        let mut buf = [0u8; 20];
        assert_eq!(write_u32_decimal(&mut buf, 0, 12345), 5);
        assert_eq!(&buf[..5], b"12345");
    }
}
