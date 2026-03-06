//! SSH config parser.
//!
//! Parses ~/.ssh/config to extract Host directive values with per-host directives.

use serde::Serialize;
use std::path::Path;

/// Parsed SSH config host block with per-host directives.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SshConfigHost {
    pub host: String,
    pub hostname: String,
    pub port: u16,
    pub user: String,
    pub identity_file: String,
}

/// Parse an SSH config file and extract hosts with their directives.
///
/// # Rules
///
/// - Extracts values from `Host` directives (case-insensitive keyword)
/// - Parses per-host directives: Hostname, Port, User, IdentityFile (case-insensitive)
/// - Skips entries containing wildcards (`*` or `?`)
/// - Skips comment lines (starting with `#`)
/// - Handles multi-value Host lines (e.g., `Host foo bar` -> two entries sharing directives)
/// - Returns deduplicated list (by host alias)
/// - Returns empty list if file does not exist
pub fn parse_ssh_config(path: &Path) -> Vec<SshConfigHost> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    parse_ssh_config_from_str(&content)
}

/// Parse SSH config content string and extract hosts with directives.
fn parse_ssh_config_from_str(content: &str) -> Vec<SshConfigHost> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Temporary state for current host block
    let mut current_aliases: Vec<String> = Vec::new();
    let mut current_hostname = String::new();
    let mut current_port: u16 = 22;
    let mut current_user = String::new();
    let mut current_identity_file = String::new();

    let flush = |aliases: &mut Vec<String>,
                 hostname: &mut String,
                 port: &mut u16,
                 user: &mut String,
                 identity_file: &mut String,
                 results: &mut Vec<SshConfigHost>,
                 seen: &mut std::collections::HashSet<String>| {
        for alias in aliases.drain(..) {
            if seen.insert(alias.clone()) {
                results.push(SshConfigHost {
                    host: alias,
                    hostname: hostname.clone(),
                    port: *port,
                    user: user.clone(),
                    identity_file: identity_file.clone(),
                });
            }
        }
        *hostname = String::new();
        *port = 22;
        *user = String::new();
        *identity_file = String::new();
    };

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split into keyword and value (first whitespace)
        let (keyword, value) = match trimmed.split_once([' ', '\t']) {
            Some((k, v)) => (k, v.trim()),
            None => continue,
        };

        if keyword.eq_ignore_ascii_case("host") {
            // Flush previous host block
            flush(
                &mut current_aliases,
                &mut current_hostname,
                &mut current_port,
                &mut current_user,
                &mut current_identity_file,
                &mut results,
                &mut seen,
            );

            // Collect non-wildcard aliases
            for alias in value.split_whitespace() {
                if !alias.contains('*') && !alias.contains('?') && !alias.is_empty() {
                    current_aliases.push(alias.to_string());
                }
            }
        } else if keyword.eq_ignore_ascii_case("hostname") {
            current_hostname = value.to_string();
        } else if keyword.eq_ignore_ascii_case("port") {
            current_port = value.parse().unwrap_or(22);
        } else if keyword.eq_ignore_ascii_case("user") {
            current_user = value.to_string();
        } else if keyword.eq_ignore_ascii_case("identityfile") {
            current_identity_file = value.to_string();
        }
    }

    // Flush last block
    flush(
        &mut current_aliases,
        &mut current_hostname,
        &mut current_port,
        &mut current_user,
        &mut current_identity_file,
        &mut results,
        &mut seen,
    );

    results
}

/// Legacy function: parse hosts as string list (for backward compatibility).
pub fn parse_ssh_config_hosts(path: &Path) -> Vec<String> {
    parse_ssh_config(path).into_iter().map(|h| h.host).collect()
}

/// Get the default SSH config file path for the current user.
pub fn default_ssh_config_path() -> Option<std::path::PathBuf> {
    super::home_dir().map(|h| std::path::PathBuf::from(h).join(".ssh").join("config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Legacy host-name-only tests (using parse_ssh_config_from_str -> host field)
    // ============================================================

    fn host_names(content: &str) -> Vec<String> {
        parse_ssh_config_from_str(content)
            .into_iter()
            .map(|h| h.host)
            .collect()
    }

    #[test]
    fn test_parse_empty_content() {
        assert!(host_names("").is_empty());
    }

    #[test]
    fn test_parse_basic_hosts() {
        let content =
            "Host server1\n  HostName 192.168.1.1\n\nHost server2\n  HostName 192.168.1.2\n";
        assert_eq!(host_names(content), vec!["server1", "server2"]);
    }

    #[test]
    fn test_parse_skips_wildcard_star() {
        let content = "Host *\n  ServerAliveInterval 60\n\nHost myserver\n  HostName example.com\n";
        assert_eq!(host_names(content), vec!["myserver"]);
    }

    #[test]
    fn test_parse_skips_wildcard_question() {
        let content =
            "Host web?\n  HostName web.example.com\n\nHost db1\n  HostName db.example.com\n";
        assert_eq!(host_names(content), vec!["db1"]);
    }

    #[test]
    fn test_parse_skips_partial_wildcards() {
        let content = "Host prod-*\n  User admin\n\nHost staging\n  User dev\n";
        assert_eq!(host_names(content), vec!["staging"]);
    }

    #[test]
    fn test_parse_comment_lines() {
        let content =
            "# This is a comment\nHost myhost\n  # Another comment\n  HostName example.com\n";
        assert_eq!(host_names(content), vec!["myhost"]);
    }

    #[test]
    fn test_parse_multi_value_host_line() {
        let content = "Host foo bar baz\n  HostName shared.example.com\n";
        assert_eq!(host_names(content), vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_parse_multi_value_with_wildcards() {
        let content = "Host foo * bar\n  HostName shared.example.com\n";
        assert_eq!(host_names(content), vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_deduplicates() {
        let content = "Host server1\n  HostName a.com\n\nHost server1\n  HostName b.com\n";
        assert_eq!(host_names(content), vec!["server1"]);
    }

    #[test]
    fn test_parse_tab_separator() {
        let content = "Host\ttabhost\n  HostName example.com\n";
        assert_eq!(host_names(content), vec!["tabhost"]);
    }

    #[test]
    fn test_parse_mixed_indentation() {
        let content = "  Host indented\n\t\tHostName example.com\n    Host another\n";
        assert_eq!(host_names(content), vec!["indented", "another"]);
    }

    #[test]
    fn test_parse_only_wildcards_returns_empty() {
        let content = "Host *\n  ServerAliveInterval 60\n\nHost *.example.com\n  User admin\n";
        assert!(host_names(content).is_empty());
    }

    #[test]
    fn test_parse_nonexistent_file() {
        let result = parse_ssh_config_hosts(Path::new("/nonexistent/path/.ssh/config"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_include_not_followed() {
        let content = "Include ~/.ssh/config.d/*\nHost visible\n  HostName example.com\n";
        assert_eq!(host_names(content), vec!["visible"]);
    }

    #[test]
    fn test_default_ssh_config_path() {
        let path = default_ssh_config_path();
        if std::env::var("HOME").is_ok() || std::env::var("USERPROFILE").is_ok() {
            assert!(path.is_some());
            let p = path.unwrap();
            assert!(p.ends_with(".ssh/config") || p.ends_with(".ssh\\config"));
        }
    }

    // ============================================================
    // Case-insensitive Host keyword tests
    // ============================================================

    #[test]
    fn test_parse_host_case_insensitive() {
        let content = "host lowercase\nHOST uppercase\nHost normal\n";
        assert_eq!(
            host_names(content),
            vec!["lowercase", "uppercase", "normal"]
        );
    }

    // ============================================================
    // Per-host directive tests
    // ============================================================

    #[test]
    fn test_parse_directives_basic() {
        let content = "\
Host myserver
  HostName 10.0.0.1
  Port 2222
  User admin
  IdentityFile ~/.ssh/id_myserver
";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 1);
        let h = &hosts[0];
        assert_eq!(h.host, "myserver");
        assert_eq!(h.hostname, "10.0.0.1");
        assert_eq!(h.port, 2222);
        assert_eq!(h.user, "admin");
        assert_eq!(h.identity_file, "~/.ssh/id_myserver");
    }

    #[test]
    fn test_parse_directives_defaults() {
        let content = "Host minimal\n  HostName example.com\n";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 1);
        let h = &hosts[0];
        assert_eq!(h.hostname, "example.com");
        assert_eq!(h.port, 22);
        assert_eq!(h.user, "");
        assert_eq!(h.identity_file, "");
    }

    #[test]
    fn test_parse_directives_no_hostname() {
        let content = "Host simple\n  User dev\n";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "");
        assert_eq!(hosts[0].user, "dev");
    }

    #[test]
    fn test_parse_directives_case_insensitive_keywords() {
        let content = "\
Host mixed
  hostname lower.example.com
  PORT 3333
  user testuser
  identityfile ~/key
";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 1);
        let h = &hosts[0];
        assert_eq!(h.hostname, "lower.example.com");
        assert_eq!(h.port, 3333);
        assert_eq!(h.user, "testuser");
        assert_eq!(h.identity_file, "~/key");
    }

    #[test]
    fn test_parse_directives_multi_host_shares_directives() {
        let content = "\
Host foo bar
  HostName shared.example.com
  Port 4444
  User shared
";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 2);
        for h in &hosts {
            assert_eq!(h.hostname, "shared.example.com");
            assert_eq!(h.port, 4444);
            assert_eq!(h.user, "shared");
        }
        assert_eq!(hosts[0].host, "foo");
        assert_eq!(hosts[1].host, "bar");
    }

    #[test]
    fn test_parse_directives_multiple_blocks() {
        let content = "\
Host alpha
  HostName alpha.example.com
  Port 22

Host beta
  HostName beta.example.com
  Port 8022
  User betauser
  IdentityFile ~/.ssh/id_beta
";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host, "alpha");
        assert_eq!(hosts[0].hostname, "alpha.example.com");
        assert_eq!(hosts[0].port, 22);
        assert_eq!(hosts[1].host, "beta");
        assert_eq!(hosts[1].hostname, "beta.example.com");
        assert_eq!(hosts[1].port, 8022);
        assert_eq!(hosts[1].user, "betauser");
        assert_eq!(hosts[1].identity_file, "~/.ssh/id_beta");
    }

    #[test]
    fn test_parse_directives_invalid_port_defaults_to_22() {
        let content = "Host bad\n  Port notanumber\n";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts[0].port, 22);
    }

    #[test]
    fn test_parse_directives_wildcard_block_skipped() {
        let content = "\
Host *
  ServerAliveInterval 60
  User globaluser

Host real
  HostName real.example.com
";
        let hosts = parse_ssh_config_from_str(content);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "real");
        assert_eq!(hosts[0].hostname, "real.example.com");
        // Wildcard block's User should NOT leak into the next block
        assert_eq!(hosts[0].user, "");
    }
}
