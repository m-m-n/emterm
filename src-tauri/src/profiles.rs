//! Terminal profiles: per-tab shell/SSH/WSL spawn configuration.
//!
//! Port of the WebView build's profile launch path:
//! - `src/profile/types.ts` (`parseEnvVars`)
//! - `src/tab-bar/tab-bar-ui.ts` (`createTabWithProfile` / `launchSshProfile`
//!   / `launchWslProfile`)
//! - `src-tauri/src/ssh/detect.rs` (`build_ssh_args` / `expand_tilde`)
//!
//! A [`app_settings::Profile`] resolves into a [`SpawnOverrides`] that
//! `Tab::spawn_shell` threads down to `PtySession::spawn`. SSH profiles
//! become a plain local PTY running `ssh_command_path` with arguments built
//! by [`build_ssh_args`]; there is no separate SSH transport.

use app_settings::Profile;

use crate::settings::Settings;

/// Per-spawn overrides resolved from a profile. `None` fields fall back to
/// the global `Settings` value at the spawn site.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpawnOverrides {
    /// Shell executable. `None` keeps `settings.shell_path` (and its
    /// `$SHELL` / `/bin/sh` fallback chain).
    pub shell_path: Option<String>,
    /// Full argv tail. `None` keeps `settings.shell_args`.
    pub shell_args: Option<Vec<String>>,
    /// Extra environment variables applied to the child, in declaration
    /// order (later duplicates win, matching the WebView's `Record`
    /// semantics where the last line for a key overwrote earlier ones).
    pub env_vars: Vec<(String, String)>,
    /// Working directory for the child. Validated (`is_dir`) at the spawn
    /// site; a missing directory logs a warning and is skipped, mirroring
    /// `src-tauri/src/pty/session.rs`.
    pub working_directory: Option<String>,
    /// The resolved SSH connection name for SSH-profile tabs. `Some` only on
    /// the SSH branch; `None` for plain/WSL tabs. Threaded onto the `Tab` so
    /// SFTP upload can rebuild the connection inputs for a drop on that tab.
    pub ssh_connection_name: Option<String>,
}

/// Parse a multi-line `KEY=VALUE` string into ordered pairs.
///
/// Rules (port of `src/profile/types.ts::parseEnvVars`):
/// - each line is one entry; lines without `=` are skipped
/// - empty / whitespace-only lines are skipped
/// - the key is everything before the first `=`, trimmed; empty keys are
///   skipped
/// - the value is everything after the first `=`, preserved as-is
pub fn parse_env_vars(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let trimmed = line.trim();
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq].trim();
        if key.is_empty() {
            continue;
        }
        out.push((key.to_string(), trimmed[eq + 1..].to_string()));
    }
    out
}

/// The user's home directory, or `None` when the platform variable is
/// unset. Used only for `~` expansion in identity-file paths.
fn home_dir() -> Option<String> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok()
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok()
    }
}

/// Expand a leading `~` to the user's home directory. `~otheruser` forms
/// are left untouched. Port of `src-tauri/src/ssh/detect.rs::expand_tilde`.
pub fn expand_tilde(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_string();
    };
    let Some(home) = home_dir() else {
        return path.to_string();
    };
    if rest.is_empty() {
        home
    } else if rest.starts_with('/') || rest.starts_with('\\') {
        format!("{home}{rest}")
    } else {
        // ~otheruser — not expanded
        path.to_string()
    }
}

/// Build ssh(1) argv from connection settings. Port of
/// `src-tauri/src/ssh/detect.rs::build_ssh_args` (the WebView build calls
/// this through the `build_ssh_args` Tauri command).
pub fn build_ssh_args(
    hostname: &str,
    port: u16,
    username: &str,
    identity_file: &str,
    ssh_options: &[(String, String)],
) -> Vec<String> {
    let mut args = Vec::new();

    if port != 22 {
        args.push("-p".to_string());
        args.push(port.to_string());
    }

    if !identity_file.is_empty() {
        args.push("-i".to_string());
        args.push(expand_tilde(identity_file));
    }

    for (key, value) in ssh_options {
        if !key.is_empty() {
            args.push("-o".to_string());
            args.push(format!("{key}={value}"));
        }
    }

    if !username.is_empty() {
        args.push(format!("{username}@{hostname}"));
    } else {
        args.push(hostname.to_string());
    }

    args
}

/// The profile flagged `is_default`, if any. The settings panel enforces
/// at-most-one default; when the JSON was hand-edited with several, the
/// first wins (same as the WebView's `profiles.find()`).
pub fn default_profile(profiles: &[Profile]) -> Option<&Profile> {
    profiles.iter().find(|p| p.is_default)
}

/// Resolve a profile into [`SpawnOverrides`].
///
/// Mirrors `tab-bar-ui.ts::createTabWithProfile`:
/// - `wsl_distro_name` set → `wsl.exe -d <distro> --cd ~` (Windows-only
///   profiles; on Linux the spawn will fail with a logged error)
/// - `ssh_connection_name` set → look up the connection, build ssh argv,
///   run `settings.ssh_command_path`
/// - otherwise → plain overrides from the non-empty profile fields
///
/// Errors are user-facing strings (the WebView showed them via `alert`);
/// the caller logs them and skips the tab.
pub fn resolve_spawn(profile: &Profile, settings: &Settings) -> Result<SpawnOverrides, String> {
    let env_vars = parse_env_vars(&profile.env_vars);

    if !profile.wsl_distro_name.is_empty() {
        return Ok(SpawnOverrides {
            shell_path: Some("wsl.exe".to_string()),
            shell_args: Some(vec![
                "-d".to_string(),
                profile.wsl_distro_name.clone(),
                "--cd".to_string(),
                "~".to_string(),
            ]),
            env_vars: Vec::new(),
            working_directory: None,
            ssh_connection_name: None,
        });
    }

    if !profile.ssh_connection_name.is_empty() {
        let Some(conn) = settings
            .ssh_connections
            .iter()
            .find(|c| c.name == profile.ssh_connection_name)
        else {
            return Err(format!(
                "SSH connection \"{}\" not found",
                profile.ssh_connection_name
            ));
        };
        if settings.ssh_command_path.is_empty() {
            return Err("SSH command not configured. Cannot launch SSH connection.".to_string());
        }
        let opts: Vec<(String, String)> = conn
            .ssh_options
            .iter()
            .map(|o| (o.key.clone(), o.value.clone()))
            .collect();
        let args = build_ssh_args(
            &conn.hostname,
            conn.port,
            &conn.username,
            &conn.identity_file,
            &opts,
        );
        return Ok(SpawnOverrides {
            shell_path: Some(settings.ssh_command_path.clone()),
            shell_args: Some(args),
            env_vars,
            working_directory: non_empty(&profile.working_directory),
            ssh_connection_name: Some(profile.ssh_connection_name.clone()),
        });
    }

    Ok(SpawnOverrides {
        shell_path: non_empty(&profile.shell_path),
        shell_args: if profile.shell_args.is_empty() {
            None
        } else {
            Some(profile.shell_args.clone())
        },
        env_vars,
        working_directory: non_empty(&profile.working_directory),
        ssh_connection_name: None,
    })
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_settings::{SshConnection, SshOption};

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_string(),
            shell_path: String::new(),
            shell_args: Vec::new(),
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        }
    }

    fn conn(name: &str) -> SshConnection {
        SshConnection {
            name: name.to_string(),
            hostname: "example.com".to_string(),
            port: 22,
            username: String::new(),
            identity_file: String::new(),
            ssh_options: Vec::new(),
            extra_options: String::new(),
        }
    }

    // ── parse_env_vars (port of src/profile/types.test.ts) ──

    #[test]
    fn parse_env_vars_basic() {
        let vars = parse_env_vars("FOO=bar\nBAZ=qux");
        assert_eq!(
            vars,
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux".to_string()),
            ]
        );
    }

    #[test]
    fn parse_env_vars_skips_lines_without_eq_and_blank() {
        let vars = parse_env_vars("FOO=bar\n\n  \nnot-an-entry\nBAZ=qux");
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn parse_env_vars_value_may_contain_eq() {
        let vars = parse_env_vars("KEY=a=b=c");
        assert_eq!(vars, vec![("KEY".to_string(), "a=b=c".to_string())]);
    }

    #[test]
    fn parse_env_vars_trims_key_preserves_value() {
        let vars = parse_env_vars("  KEY  =value with spaces");
        assert_eq!(
            vars,
            vec![("KEY".to_string(), "value with spaces".to_string())]
        );
    }

    #[test]
    fn parse_env_vars_skips_empty_key() {
        let vars = parse_env_vars("=value\n =v2");
        assert!(vars.is_empty());
    }

    // ── build_ssh_args (port of src-tauri/src/ssh/detect.rs tests) ──

    #[test]
    fn ssh_args_minimal() {
        assert_eq!(
            build_ssh_args("example.com", 22, "", "", &[]),
            ["example.com"]
        );
    }

    #[test]
    fn ssh_args_custom_port() {
        assert_eq!(
            build_ssh_args("host.com", 8022, "", "", &[]),
            ["-p", "8022", "host.com"]
        );
    }

    #[test]
    fn ssh_args_username() {
        assert_eq!(
            build_ssh_args("host.com", 22, "admin", "", &[]),
            ["admin@host.com"]
        );
    }

    #[test]
    fn ssh_args_identity_file() {
        assert_eq!(
            build_ssh_args("host.com", 22, "", "/path/to/key", &[]),
            ["-i", "/path/to/key", "host.com"]
        );
    }

    #[test]
    fn ssh_args_options_and_all_fields() {
        let opts = vec![("StrictHostKeyChecking".to_string(), "no".to_string())];
        let args = build_ssh_args("example.com", 2222, "user", "~/.ssh/id_rsa", &opts);
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "2222");
        assert_eq!(args[2], "-i");
        assert!(args[3].ends_with("/.ssh/id_rsa"));
        assert_eq!(args[4], "-o");
        assert_eq!(args[5], "StrictHostKeyChecking=no");
        assert_eq!(args[6], "user@example.com");
    }

    #[test]
    fn ssh_args_skips_empty_option_key() {
        let opts = vec![(String::new(), "value".to_string())];
        assert_eq!(build_ssh_args("host.com", 22, "", "", &opts), ["host.com"]);
    }

    // ── expand_tilde ──

    #[test]
    fn tilde_absolute_path_untouched() {
        assert_eq!(expand_tilde("/absolute/path"), "/absolute/path");
    }

    #[test]
    fn tilde_expands_home_prefix() {
        let result = expand_tilde("~/.ssh/id_rsa");
        assert!(!result.starts_with('~'), "should expand ~: {result}");
        assert!(result.ends_with("/.ssh/id_rsa"));
    }

    #[test]
    fn tilde_other_user_untouched() {
        assert_eq!(expand_tilde("~otheruser/.ssh"), "~otheruser/.ssh");
    }

    // ── default_profile ──

    #[test]
    fn default_profile_first_flagged_wins() {
        let mut a = profile("a");
        let mut b = profile("b");
        a.is_default = false;
        b.is_default = true;
        let mut c = profile("c");
        c.is_default = true;
        let profiles = vec![a, b, c];
        assert_eq!(default_profile(&profiles).unwrap().name, "b");
    }

    #[test]
    fn default_profile_none_flagged() {
        assert!(default_profile(&[profile("a")]).is_none());
    }

    // ── resolve_spawn ──

    #[test]
    fn resolve_plain_profile_maps_non_empty_fields() {
        let mut p = profile("dev");
        p.shell_path = "/bin/zsh".to_string();
        p.shell_args = vec!["-l".to_string()];
        p.env_vars = "FOO=bar".to_string();
        p.working_directory = "/tmp".to_string();
        let s = Settings::default();
        let o = resolve_spawn(&p, &s).unwrap();
        assert_eq!(o.shell_path.as_deref(), Some("/bin/zsh"));
        assert_eq!(o.shell_args.as_deref(), Some(&["-l".to_string()][..]));
        assert_eq!(o.env_vars, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(o.working_directory.as_deref(), Some("/tmp"));
    }

    #[test]
    fn resolve_empty_profile_keeps_global_fallbacks() {
        let p = profile("empty");
        let s = Settings::default();
        let o = resolve_spawn(&p, &s).unwrap();
        assert_eq!(o, SpawnOverrides::default());
    }

    #[test]
    fn resolve_wsl_profile_builds_wsl_argv() {
        let mut p = profile("ubuntu");
        p.wsl_distro_name = "Ubuntu-24.04".to_string();
        // WSL branch wins even when ssh_connection_name is also set
        p.ssh_connection_name = "ignored".to_string();
        let s = Settings::default();
        let o = resolve_spawn(&p, &s).unwrap();
        assert_eq!(o.shell_path.as_deref(), Some("wsl.exe"));
        assert_eq!(
            o.shell_args.as_deref().unwrap(),
            &["-d", "Ubuntu-24.04", "--cd", "~"]
        );
        // WSL tabs carry no SSH connection name.
        assert_eq!(o.ssh_connection_name, None);
    }

    #[test]
    fn resolve_ssh_profile_builds_ssh_argv() {
        let mut p = profile("remote");
        p.ssh_connection_name = "work".to_string();
        p.env_vars = "FOO=bar".to_string();
        let mut c = conn("work");
        c.port = 2222;
        c.username = "user".to_string();
        c.ssh_options = vec![SshOption {
            key: "ServerAliveInterval".to_string(),
            value: "60".to_string(),
        }];
        let mut s = Settings::default();
        s.ssh_connections = vec![c];
        s.ssh_command_path = "/usr/bin/ssh".to_string();
        let o = resolve_spawn(&p, &s).unwrap();
        assert_eq!(o.shell_path.as_deref(), Some("/usr/bin/ssh"));
        assert_eq!(
            o.shell_args.as_deref().unwrap(),
            &[
                "-p",
                "2222",
                "-o",
                "ServerAliveInterval=60",
                "user@example.com"
            ]
        );
        assert_eq!(o.env_vars, vec![("FOO".to_string(), "bar".to_string())]);
        // SSH tabs carry the resolved connection name for SFTP upload.
        assert_eq!(o.ssh_connection_name.as_deref(), Some("work"));
    }

    #[test]
    fn resolve_plain_profile_has_no_connection_name() {
        let mut p = profile("dev");
        p.shell_path = "/bin/zsh".to_string();
        let s = Settings::default();
        let o = resolve_spawn(&p, &s).unwrap();
        assert_eq!(o.ssh_connection_name, None);
    }

    #[test]
    fn resolve_ssh_profile_unknown_connection_errors() {
        let mut p = profile("remote");
        p.ssh_connection_name = "missing".to_string();
        let s = Settings::default();
        let err = resolve_spawn(&p, &s).unwrap_err();
        assert!(err.contains("missing"), "{err}");
    }

    #[test]
    fn resolve_ssh_profile_without_ssh_command_errors() {
        let mut p = profile("remote");
        p.ssh_connection_name = "work".to_string();
        let mut s = Settings::default();
        s.ssh_connections = vec![conn("work")];
        s.ssh_command_path = String::new();
        let err = resolve_spawn(&p, &s).unwrap_err();
        assert!(err.contains("not configured"), "{err}");
    }
}
