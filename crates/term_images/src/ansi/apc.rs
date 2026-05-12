//! APC (Application Program Command) sequence parsing.
//!
//! This module handles APC sequences which start with `ESC _` and end with ST (`ESC \`).
//! The primary use case is Kitty Graphics Protocol sequences.
//!
//! # Kitty Graphics Protocol Format
//!
//! ```text
//! ESC _ G <key>=<value>,<key>=<value>;[base64_payload] ESC \
//! ```
//!
//! # Example
//!
//! ```
//! use term_images::ansi::apc::{ApcAction, KittyCommand, parse_kitty_command};
//!
//! let data = b"Ga=T,f=100,s=10,v=10;iVBORw0KGgo=";
//! let command = parse_kitty_command(data);
//! assert!(command.is_some());
//! ```

use serde::Serialize;
use std::collections::HashMap;

/// Maximum size for APC data buffer.
pub const MAX_APC_LEN: usize = 1024 * 1024; // 1MB max for base64 image data

/// APC sequence action.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", content = "data")]
pub enum ApcAction {
    /// Kitty Graphics Protocol command.
    KittyGraphics(KittyCommand),

    /// Unknown or malformed APC sequence.
    Unknown(String),
}

/// Kitty Graphics Protocol command.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KittyCommand {
    /// Action type: t (transmit), T (transmit and display), p (put/display),
    /// d (delete), q (query), f (frame), a (animate), c (compose)
    pub action: KittyAction,

    /// Image ID (i=)
    pub image_id: Option<u32>,

    /// Placement ID (p=)
    pub placement_id: Option<u32>,

    /// Transmission medium: d (direct), f (file), t (temp file), s (shared memory)
    pub transmission: Option<KittyTransmission>,

    /// Format: 24 (RGB), 32 (RGBA), 100 (PNG)
    pub format: Option<KittyFormat>,

    /// Compression: z (zlib)
    pub compression: Option<KittyCompression>,

    /// Width in pixels (s=)
    pub width: Option<u32>,

    /// Height in pixels (v=)
    pub height: Option<u32>,

    /// More data chunks follow (m=1) or final chunk (m=0)
    pub more: bool,

    /// Display columns (c=)
    pub columns: Option<u32>,

    /// Display rows (r=)
    pub rows: Option<u32>,

    /// X offset within cell (X=)
    pub x_offset: Option<u32>,

    /// Y offset within cell (Y=)
    pub y_offset: Option<u32>,

    /// Z-index for layering (z=)
    pub z_index: Option<i32>,

    /// Cursor movement mode (C=0 or C=1)
    pub cursor_movement: Option<u8>,

    /// Delete target for a=d (d=)
    pub delete_target: Option<KittyDeleteTarget>,

    /// Quiet mode - suppress responses (q=)
    pub quiet: Option<u8>,

    /// Base64-encoded payload data
    pub payload: String,

    /// Raw control data (for debugging)
    #[serde(skip)]
    pub raw_control: String,

    // =========================================================================
    // Animation Parameters (a=f, a=a, a=c)
    // =========================================================================
    /// Animation frame number (for a=f frames)
    pub frame_number: Option<u32>,

    /// Background color for frame (Y= RGBA)
    pub background_color: Option<u32>,

    /// Base frame for composition (c= frame number)
    pub base_frame: Option<u32>,

    /// Composition mode (X=1 for replace, default alpha blend)
    pub composition_mode: Option<u8>,

    /// Frame gap/delay in milliseconds (z= value, negative means no gap)
    pub frame_gap: Option<i32>,

    /// Animation state control (s=1 stop, s=2 loading, s=3 loop)
    pub animation_state: Option<u8>,

    /// Loop count for animation (v= value, 0 = infinite)
    pub animation_loops: Option<u32>,

    /// Target frame for composition (a=c)
    pub target_frame: Option<u32>,

    /// Source frame for composition (a=c)
    pub source_frame: Option<u32>,
}

/// Kitty action type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KittyAction {
    /// Transmit image data (a=t)
    Transmit,
    /// Transmit and display (a=T)
    TransmitAndDisplay,
    /// Put/display image at position (a=p)
    Put,
    /// Delete images (a=d)
    Delete,
    /// Query protocol support (a=q)
    Query,
    /// Animation frame (a=f)
    Frame,
    /// Animate (a=a)
    Animate,
    /// Compose frames (a=c)
    Compose,
}

/// Kitty transmission medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KittyTransmission {
    /// Direct base64 data (t=d)
    Direct,
    /// File path (t=f)
    File,
    /// Temporary file (t=t)
    TempFile,
    /// Shared memory (t=s)
    SharedMemory,
}

/// Kitty image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KittyFormat {
    /// RGB 24-bit (f=24)
    Rgb,
    /// RGBA 32-bit (f=32)
    Rgba,
    /// PNG (f=100)
    Png,
}

/// Kitty compression type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KittyCompression {
    /// Zlib compression (o=z)
    Zlib,
}

/// Kitty delete target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KittyDeleteTarget {
    /// Delete all images on screen (d=a)
    All,
    /// Delete all images (d=A)
    AllIncludingHidden,
    /// Delete by image ID (d=i)
    ById,
    /// Delete by placement ID (d=p)
    ByPlacement,
    /// Delete at cursor position (d=c)
    AtCursor,
    /// Delete at cursor position by columns (d=C)
    AtCursorByColumns,
    /// Delete at specific position (d=x, d=y)
    AtPosition,
    /// Delete at specific cell by row/column (d=X, d=Y)
    AtCell,
    /// Delete by z-index (d=z)
    ByZIndex,
}

impl Default for KittyCommand {
    fn default() -> Self {
        Self {
            action: KittyAction::TransmitAndDisplay,
            image_id: None,
            placement_id: None,
            transmission: None,
            format: None,
            compression: None,
            width: None,
            height: None,
            more: false,
            columns: None,
            rows: None,
            x_offset: None,
            y_offset: None,
            z_index: None,
            cursor_movement: None,
            delete_target: None,
            quiet: None,
            payload: String::new(),
            raw_control: String::new(),
            // Animation fields
            frame_number: None,
            background_color: None,
            base_frame: None,
            composition_mode: None,
            frame_gap: None,
            animation_state: None,
            animation_loops: None,
            target_frame: None,
            source_frame: None,
        }
    }
}

/// Parse Kitty Graphics Protocol command from APC data.
///
/// The data should be the content after `ESC _ G` and before `ESC \`.
///
/// # Format
///
/// ```text
/// <control_data>;<payload>
/// ```
///
/// Control data is comma-separated key=value pairs.
pub fn parse_kitty_command(data: &[u8]) -> Option<KittyCommand> {
    // Must start with 'G' for Kitty Graphics
    if data.is_empty() || data[0] != b'G' {
        return None;
    }

    let data = &data[1..]; // Skip 'G'

    // Split into control data and payload at first semicolon
    let (control_data, payload) = match data.iter().position(|&b| b == b';') {
        Some(pos) => {
            let (ctrl, rest) = data.split_at(pos);
            (ctrl, &rest[1..]) // Skip the semicolon
        }
        None => (data, &[][..]),
    };

    let control_str = String::from_utf8_lossy(control_data);
    let mut cmd = KittyCommand {
        raw_control: control_str.to_string(),
        payload: String::from_utf8_lossy(payload).to_string(),
        ..Default::default()
    };

    // Parse key=value pairs
    let params = parse_key_value_pairs(&control_str);

    // Action (a=)
    if let Some(action) = params.get("a") {
        cmd.action = match action.as_str() {
            "t" => KittyAction::Transmit,
            "T" => KittyAction::TransmitAndDisplay,
            "p" => KittyAction::Put,
            "d" => KittyAction::Delete,
            "q" => KittyAction::Query,
            "f" => KittyAction::Frame,
            "a" => KittyAction::Animate,
            "c" => KittyAction::Compose,
            _ => KittyAction::TransmitAndDisplay, // Default
        };
    }

    // Image ID (i=)
    if let Some(id) = params.get("i") {
        cmd.image_id = id.parse().ok();
    }

    // Placement ID (p=)
    if let Some(id) = params.get("p") {
        cmd.placement_id = id.parse().ok();
    }

    // Transmission medium (t=)
    if let Some(t) = params.get("t") {
        cmd.transmission = match t.as_str() {
            "d" => Some(KittyTransmission::Direct),
            "f" => Some(KittyTransmission::File),
            "t" => Some(KittyTransmission::TempFile),
            "s" => Some(KittyTransmission::SharedMemory),
            _ => None,
        };
    }

    // Format (f=)
    if let Some(f) = params.get("f") {
        cmd.format = match f.as_str() {
            "24" => Some(KittyFormat::Rgb),
            "32" => Some(KittyFormat::Rgba),
            "100" => Some(KittyFormat::Png),
            _ => None,
        };
    }

    // Compression (o=)
    if let Some(o) = params.get("o") {
        cmd.compression = match o.as_str() {
            "z" => Some(KittyCompression::Zlib),
            _ => None,
        };
    }

    // Width (s=)
    if let Some(s) = params.get("s") {
        cmd.width = s.parse().ok();
    }

    // Height (v=)
    if let Some(v) = params.get("v") {
        cmd.height = v.parse().ok();
    }

    // More chunks (m=)
    if let Some(m) = params.get("m") {
        cmd.more = m == "1";
    }

    // Display columns (c=)
    if let Some(c) = params.get("c") {
        cmd.columns = c.parse().ok();
    }

    // Display rows (r=)
    if let Some(r) = params.get("r") {
        cmd.rows = r.parse().ok();
    }

    // X offset (X=)
    if let Some(x) = params.get("X") {
        cmd.x_offset = x.parse().ok();
    }

    // Y offset (Y=)
    if let Some(y) = params.get("Y") {
        cmd.y_offset = y.parse().ok();
    }

    // Z-index (z=)
    if let Some(z) = params.get("z") {
        cmd.z_index = z.parse().ok();
    }

    // Cursor movement (C=)
    if let Some(c) = params.get("C") {
        cmd.cursor_movement = c.parse().ok();
    }

    // Delete target (d=)
    if let Some(d) = params.get("d") {
        cmd.delete_target = match d.as_str() {
            "a" => Some(KittyDeleteTarget::All),
            "A" => Some(KittyDeleteTarget::AllIncludingHidden),
            "i" => Some(KittyDeleteTarget::ById),
            "p" => Some(KittyDeleteTarget::ByPlacement),
            "c" => Some(KittyDeleteTarget::AtCursor),
            "C" => Some(KittyDeleteTarget::AtCursorByColumns),
            "x" | "y" => Some(KittyDeleteTarget::AtPosition),
            "X" | "Y" => Some(KittyDeleteTarget::AtCell),
            "z" => Some(KittyDeleteTarget::ByZIndex),
            _ => None,
        };
    }

    // Quiet mode (q=)
    if let Some(q) = params.get("q") {
        cmd.quiet = q.parse().ok();
    }

    // =========================================================================
    // Animation Parameters
    // =========================================================================

    // Frame number for a=f (frame action uses 'r' for frame number in Kitty spec)
    // Also check 'x' as some implementations use it
    if cmd.action == KittyAction::Frame {
        // In Kitty protocol, for frames, 'r' is repurposed as frame number
        if let Some(r) = params.get("r") {
            cmd.frame_number = r.parse().ok();
        }
    }

    // Background color (Y= for frames, but we already use Y for y_offset)
    // In Kitty animation, 'Y' can be background color in hex format
    // We'll parse it as u32 for background_color if it's a frame action
    if cmd.action == KittyAction::Frame {
        if let Some(y_val) = params.get("Y") {
            // Try parsing as hex (without 0x prefix)
            if let Ok(color) = u32::from_str_radix(y_val, 16) {
                cmd.background_color = Some(color);
            }
        }
    }

    // Base frame for composition (c= in frame context)
    // Note: 'c' is also used for columns, but for Frame action it means base frame
    if cmd.action == KittyAction::Frame || cmd.action == KittyAction::Compose {
        if let Some(c_val) = params.get("c") {
            cmd.base_frame = c_val.parse().ok();
        }
    }

    // Composition mode (X= for frames)
    // Note: 'X' is also used for x_offset, for Frame action it means composition mode
    if cmd.action == KittyAction::Frame {
        if let Some(x_val) = params.get("X") {
            cmd.composition_mode = x_val.parse().ok();
        }
    }

    // Frame gap/delay (z= value, can be negative)
    // Note: 'z' is also z_index for placement, for Frame action it's delay
    if cmd.action == KittyAction::Frame {
        if let Some(z_val) = params.get("z") {
            cmd.frame_gap = z_val.parse().ok();
        }
    }

    // Animation state control (s= value)
    // s=1: stop, s=2: loading mode, s=3: loop
    if let Some(s) = params.get("s") {
        cmd.animation_state = s.parse().ok();
    }

    // Loop count for animation (v= value, 0 = infinite)
    // Note: 'v' is also height for image data, for Animate action it's loop count
    if cmd.action == KittyAction::Animate {
        if let Some(v_val) = params.get("v") {
            cmd.animation_loops = v_val.parse().ok();
        }
    }

    // Target and source frames for compose (a=c)
    if cmd.action == KittyAction::Compose {
        // Target frame is specified with 'r'
        if let Some(r_val) = params.get("r") {
            cmd.target_frame = r_val.parse().ok();
        }
        // Source frame is specified with 'c' (already parsed as base_frame above)
        cmd.source_frame = cmd.base_frame;
    }

    Some(cmd)
}

/// Parse comma-separated key=value pairs.
fn parse_key_value_pairs(s: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();

    for pair in s.split(',') {
        if let Some(eq_pos) = pair.find('=') {
            let key = &pair[..eq_pos];
            let value = &pair[eq_pos + 1..];
            result.insert(key.to_string(), value.to_string());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Kitty Command Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_kitty_basic_transmit_and_display() {
        let data = b"Ga=T,f=100,s=10,v=10;iVBORw0KGgo=";
        let cmd = parse_kitty_command(data);

        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert_eq!(cmd.action, KittyAction::TransmitAndDisplay);
        assert_eq!(cmd.format, Some(KittyFormat::Png));
        assert_eq!(cmd.width, Some(10));
        assert_eq!(cmd.height, Some(10));
        assert_eq!(cmd.payload, "iVBORw0KGgo=");
    }

    #[test]
    fn test_parse_kitty_transmit_only() {
        let data = b"Ga=t,i=1,f=32,s=100,v=50;AAAA";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Transmit);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.format, Some(KittyFormat::Rgba));
        assert_eq!(cmd.width, Some(100));
        assert_eq!(cmd.height, Some(50));
    }

    #[test]
    fn test_parse_kitty_put_action() {
        let data = b"Ga=p,i=1,p=2,c=10,r=5;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Put);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.placement_id, Some(2));
        assert_eq!(cmd.columns, Some(10));
        assert_eq!(cmd.rows, Some(5));
    }

    #[test]
    fn test_parse_kitty_delete_action() {
        let data = b"Ga=d,d=a;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Delete);
        assert_eq!(cmd.delete_target, Some(KittyDeleteTarget::All));
    }

    #[test]
    fn test_parse_kitty_query_action() {
        let data = b"Ga=q;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Query);
    }

    #[test]
    fn test_parse_kitty_chunked_transfer() {
        // First chunk
        let data1 = b"Ga=t,i=1,f=100,m=1;iVBORw0KGgoA";
        let cmd1 = parse_kitty_command(data1).unwrap();
        assert!(cmd1.more);

        // Final chunk
        let data2 = b"Ga=t,i=1,m=0;AAABBB==";
        let cmd2 = parse_kitty_command(data2).unwrap();
        assert!(!cmd2.more);
    }

    #[test]
    fn test_parse_kitty_compression() {
        let data = b"Ga=T,f=100,o=z;eJxLTEstAgA=";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.compression, Some(KittyCompression::Zlib));
    }

    #[test]
    fn test_parse_kitty_transmission_medium() {
        let data = b"Ga=t,t=f;L3RtcC9pbWFnZS5wbmc=";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.transmission, Some(KittyTransmission::File));
    }

    #[test]
    fn test_parse_kitty_z_index() {
        let data = b"Ga=p,i=1,z=-10;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.z_index, Some(-10));
    }

    #[test]
    fn test_parse_kitty_cursor_movement() {
        let data = b"Ga=T,C=0;data";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.cursor_movement, Some(0));
    }

    #[test]
    fn test_parse_kitty_quiet_mode() {
        let data = b"Ga=T,q=2;data";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.quiet, Some(2));
    }

    #[test]
    fn test_parse_kitty_default_action() {
        // No action specified, should default to TransmitAndDisplay
        let data = b"Gf=100;data";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::TransmitAndDisplay);
    }

    #[test]
    fn test_parse_kitty_empty_payload() {
        let data = b"Ga=d,d=a";
        let cmd = parse_kitty_command(data).unwrap();

        assert!(cmd.payload.is_empty());
    }

    #[test]
    fn test_parse_kitty_invalid_not_g() {
        let data = b"Xa=T;data";
        let cmd = parse_kitty_command(data);

        assert!(cmd.is_none());
    }

    #[test]
    fn test_parse_kitty_empty_data() {
        let cmd = parse_kitty_command(b"");
        assert!(cmd.is_none());
    }

    #[test]
    fn test_parse_kitty_rgb_format() {
        let data = b"Ga=T,f=24,s=2,v=2;AAAA";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.format, Some(KittyFormat::Rgb));
    }

    #[test]
    fn test_parse_kitty_offsets() {
        let data = b"Ga=p,i=1,X=5,Y=10;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.x_offset, Some(5));
        assert_eq!(cmd.y_offset, Some(10));
    }

    #[test]
    fn test_parse_kitty_delete_by_id() {
        let data = b"Ga=d,d=i,i=42;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Delete);
        assert_eq!(cmd.delete_target, Some(KittyDeleteTarget::ById));
        assert_eq!(cmd.image_id, Some(42));
    }

    #[test]
    fn test_parse_kitty_animation_frame() {
        let data = b"Ga=f,i=1;framedata";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Frame);
    }

    #[test]
    fn test_parse_kitty_animate() {
        let data = b"Ga=a,i=1;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Animate);
    }

    #[test]
    fn test_parse_kitty_compose() {
        let data = b"Ga=c,i=1;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Compose);
    }

    // =========================================================================
    // Key-Value Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_key_value_pairs_basic() {
        let pairs = parse_key_value_pairs("a=1,b=2,c=hello");
        assert_eq!(pairs.get("a"), Some(&"1".to_string()));
        assert_eq!(pairs.get("b"), Some(&"2".to_string()));
        assert_eq!(pairs.get("c"), Some(&"hello".to_string()));
    }

    #[test]
    fn test_parse_key_value_pairs_empty() {
        let pairs = parse_key_value_pairs("");
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_parse_key_value_pairs_no_equals() {
        let pairs = parse_key_value_pairs("a=1,invalid,b=2");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs.get("a"), Some(&"1".to_string()));
        assert_eq!(pairs.get("b"), Some(&"2".to_string()));
    }

    // =========================================================================
    // Animation Parameter Tests
    // =========================================================================

    #[test]
    fn test_parse_kitty_frame_with_delay() {
        let data = b"Ga=f,i=1,r=2,z=100;framedata";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Frame);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.frame_number, Some(2));
        assert_eq!(cmd.frame_gap, Some(100));
    }

    #[test]
    fn test_parse_kitty_frame_with_composition() {
        let data = b"Ga=f,i=1,c=1,X=1;framedata";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Frame);
        assert_eq!(cmd.base_frame, Some(1));
        assert_eq!(cmd.composition_mode, Some(1)); // Replace mode
    }

    #[test]
    fn test_parse_kitty_animate_with_state() {
        let data = b"Ga=a,i=1,s=3,v=5;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Animate);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.animation_state, Some(3)); // Playing
        assert_eq!(cmd.animation_loops, Some(5));
    }

    #[test]
    fn test_parse_kitty_compose_frames() {
        let data = b"Ga=c,i=1,c=2,r=3;";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Compose);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.source_frame, Some(2)); // Same as base_frame
        assert_eq!(cmd.target_frame, Some(3));
    }

    #[test]
    fn test_parse_kitty_frame_negative_gap() {
        let data = b"Ga=f,i=1,z=-1;framedata";
        let cmd = parse_kitty_command(data).unwrap();

        assert_eq!(cmd.action, KittyAction::Frame);
        assert_eq!(cmd.frame_gap, Some(-1)); // No gap
    }
}
