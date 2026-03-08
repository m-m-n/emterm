//! SFTP command argument construction from SSH connection settings.
//!
//! Builds the argument list for spawning an `sftp` subprocess.
//! Note: sftp uses `-P` (uppercase) for port, unlike ssh which uses `-p`.

use crate::ssh::detect::expand_tilde;

/// Build sftp command arguments from SSH connection settings.
///
/// Constructs: `sftp [-P port] [-i identity_file] [-o Key=Value ...] -b - [user@]hostname`
pub fn build_sftp_args(
    hostname: &str,
    port: u16,
    username: &str,
    identity_file: &str,
    ssh_options: &[(String, String)],
) -> Vec<String> {
    let mut args = Vec::new();

    if port != 22 {
        args.push("-P".to_string());
        args.push(port.to_string());
    }

    if !identity_file.is_empty() {
        let expanded = expand_tilde(identity_file);
        args.push("-i".to_string());
        args.push(expanded);
    }

    for (key, value) in ssh_options {
        if !key.is_empty() {
            args.push("-o".to_string());
            args.push(format!("{}={}", key, value));
        }
    }

    // Batch mode: read commands from stdin
    args.push("-b".to_string());
    args.push("-".to_string());

    // IPv6 addresses must be enclosed in brackets for sftp
    let host = if hostname.contains(':') {
        format!("[{}]", hostname)
    } else {
        hostname.to_string()
    };

    if !username.is_empty() {
        args.push(format!("{}@{}", username, host));
    } else {
        args.push(host);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sftp_args_minimal() {
        let args = build_sftp_args("example.com", 22, "", "", &[]);
        assert_eq!(args, vec!["-b", "-", "example.com"]);
    }

    #[test]
    fn test_build_sftp_args_with_custom_port_uses_uppercase_p() {
        let args = build_sftp_args("example.com", 2222, "", "", &[]);
        assert_eq!(args[0], "-P");
        assert_eq!(args[1], "2222");
        assert!(args.contains(&"-b".to_string()));
        assert!(args.contains(&"-".to_string()));
        assert!(args.contains(&"example.com".to_string()));
    }

    #[test]
    fn test_build_sftp_args_with_username() {
        let args = build_sftp_args("example.com", 22, "user", "", &[]);
        assert!(args.contains(&"user@example.com".to_string()));
    }

    #[test]
    fn test_build_sftp_args_with_identity_file() {
        let args = build_sftp_args("example.com", 22, "", "/path/to/key", &[]);
        assert_eq!(args[0], "-i");
        assert_eq!(args[1], "/path/to/key");
    }

    #[test]
    fn test_build_sftp_args_with_ssh_options() {
        let opts = vec![
            ("StrictHostKeyChecking".to_string(), "no".to_string()),
            ("ServerAliveInterval".to_string(), "60".to_string()),
        ];
        let args = build_sftp_args("example.com", 22, "", "", &opts);
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"StrictHostKeyChecking=no".to_string()));
        assert!(args.contains(&"ServerAliveInterval=60".to_string()));
    }

    #[test]
    fn test_build_sftp_args_skips_empty_key() {
        let opts = vec![("".to_string(), "value".to_string())];
        let args = build_sftp_args("example.com", 22, "", "", &opts);
        assert!(!args.contains(&"-o".to_string()));
    }

    #[test]
    fn test_build_sftp_args_with_all_fields() {
        let opts = vec![("StrictHostKeyChecking".to_string(), "no".to_string())];
        let args = build_sftp_args("example.com", 2222, "user", "/key", &opts);
        assert_eq!(args[0], "-P");
        assert_eq!(args[1], "2222");
        assert_eq!(args[2], "-i");
        assert_eq!(args[3], "/key");
        assert_eq!(args[4], "-o");
        assert_eq!(args[5], "StrictHostKeyChecking=no");
        assert_eq!(args[6], "-b");
        assert_eq!(args[7], "-");
        assert_eq!(args[8], "user@example.com");
    }

    #[test]
    fn test_build_sftp_args_with_tilde_identity_file() {
        let args = build_sftp_args("example.com", 22, "", "~/.ssh/id_rsa", &[]);
        assert_eq!(args[0], "-i");
        // Should be expanded (not start with ~)
        assert!(
            !args[1].starts_with('~'),
            "Identity file should be expanded: {}",
            args[1]
        );
        assert!(args[1].ends_with("/.ssh/id_rsa"));
    }

    #[test]
    fn test_build_sftp_args_batch_mode_always_present() {
        let args = build_sftp_args("example.com", 22, "", "", &[]);
        let b_idx = args.iter().position(|a| a == "-b").unwrap();
        assert_eq!(args[b_idx + 1], "-");
    }

    #[test]
    fn test_build_sftp_args_ipv6_address_bracketed() {
        let args = build_sftp_args("::1", 22, "", "", &[]);
        assert!(args.contains(&"[::1]".to_string()));
    }

    #[test]
    fn test_build_sftp_args_ipv6_with_username() {
        let args = build_sftp_args("::1", 2222, "user", "", &[]);
        assert!(args.contains(&"user@[::1]".to_string()));
    }

    #[test]
    fn test_build_sftp_args_ipv4_not_bracketed() {
        let args = build_sftp_args("192.168.1.1", 22, "", "", &[]);
        assert!(args.contains(&"192.168.1.1".to_string()));
    }
}
