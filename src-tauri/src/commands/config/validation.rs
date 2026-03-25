use rust_i18n::t;

use super::settings::AppSettings;
use super::types::*;

// ============================================================
// Validation
// ============================================================

/// Validates settings values and returns an error message if invalid.
pub(super) fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    if settings.font_size < MIN_FONT_SIZE || settings.font_size > MAX_FONT_SIZE {
        return Err(t!(
            "validation.fontSize",
            min = MIN_FONT_SIZE,
            max = MAX_FONT_SIZE
        )
        .to_string());
    }

    if settings.padding > MAX_PADDING {
        return Err(t!("validation.padding", min = MIN_PADDING, max = MAX_PADDING).to_string());
    }

    if settings.scrollback_lines > MAX_SCROLLBACK_LINES {
        return Err(t!(
            "validation.scrollbackLines",
            min = MIN_SCROLLBACK_LINES,
            max = MAX_SCROLLBACK_LINES
        )
        .to_string());
    }

    if settings.scroll_speed < MIN_SCROLL_SPEED || settings.scroll_speed > MAX_SCROLL_SPEED {
        return Err(t!(
            "validation.scrollSpeed",
            min = MIN_SCROLL_SPEED,
            max = MAX_SCROLL_SPEED
        )
        .to_string());
    }

    if settings.markdown_font_size < MIN_FONT_SIZE || settings.markdown_font_size > MAX_FONT_SIZE {
        return Err(t!(
            "validation.markdownFontSize",
            min = MIN_FONT_SIZE,
            max = MAX_FONT_SIZE
        )
        .to_string());
    }

    for (i, profile) in settings.profiles.iter().enumerate() {
        if profile.name.trim().is_empty() {
            return Err(t!("validation.profileNameEmpty", index = i + 1).to_string());
        }
    }

    for (i, conn) in settings.ssh_connections.iter().enumerate() {
        if conn.name.trim().is_empty() {
            return Err(t!("validation.sshConnectionNameEmpty", index = i + 1).to_string());
        }
        let hostname = conn.hostname.trim();
        if hostname.is_empty() {
            return Err(t!("validation.sshHostnameEmpty", index = i + 1).to_string());
        }
        // Reject hostnames with control characters, spaces, or shell metacharacters
        if hostname
            .chars()
            .any(|c| c.is_control() || c == ' ' || c == '\t')
        {
            return Err(t!("validation.sshHostnameInvalid", index = i + 1).to_string());
        }
        // Port range: u16 caps at 65535, so only check lower bound
        if conn.port == 0 {
            return Err(t!(
                "validation.sshPortRange",
                index = i + 1,
                min = 1,
                max = 65535
            )
            .to_string());
        }
        // Reject dangerous SSH options that allow arbitrary command execution
        for opt in &conn.ssh_options {
            if is_dangerous_ssh_option(&opt.key) {
                return Err(t!(
                    "validation.sshOptionDangerous",
                    index = i + 1,
                    key = opt.key
                )
                .to_string());
            }
        }
    }

    // Status bar validation
    if let Some(font_size) = settings.statusbar_font_size {
        if font_size < MIN_FONT_SIZE as f32 || font_size > MAX_FONT_SIZE as f32 {
            return Err(t!(
                "validation.statusbarFontSize",
                min = MIN_FONT_SIZE,
                max = MAX_FONT_SIZE
            )
            .to_string());
        }
    }

    for (name, cmd) in &settings.statusbar_custom_commands {
        if name.trim().is_empty() {
            return Err(t!("validation.statusbarCommandNameEmpty").to_string());
        }
        if cmd.executable.trim().is_empty() {
            return Err(t!("validation.statusbarCommandExecutableEmpty", name = name).to_string());
        }
        // Reject executable paths with arguments (spaces after trimming suggest arguments)
        if cmd.executable.trim().contains(' ') {
            return Err(t!("validation.statusbarCommandNoArgs", name = name).to_string());
        }
    }

    Ok(())
}

/// SSH option keys that allow arbitrary command execution.
fn is_dangerous_ssh_option(key: &str) -> bool {
    const DANGEROUS_KEYS: &[&str] = &[
        "proxycommand",
        "localcommand",
        "remotecommand",
        "permitlocalcommand",
    ];
    DANGEROUS_KEYS.contains(&key.to_ascii_lowercase().as_str())
}
