//! Tests for `SshConnection`, `SshOption`, and SSH-related profile fields.

use super::*;

#[test]
fn test_app_settings_default_has_empty_ssh_fields() {
    let settings = AppSettings::default();
    assert_eq!(settings.ssh_command_path, "");
    assert!(settings.ssh_connections.is_empty());
}

#[test]
fn test_deserialize_missing_ssh_fields_use_defaults() {
    let json = r#"{}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.ssh_command_path, "");
    assert!(settings.ssh_connections.is_empty());
}

#[test]
fn test_deserialize_null_ssh_fields_use_defaults() {
    let json = r#"{"ssh_command_path": null, "ssh_connections": null}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.ssh_command_path, "");
    assert!(settings.ssh_connections.is_empty());
}

#[test]
fn test_ssh_connection_serialization() {
    let conn = SshConnection {
        name: "My Server".to_string(),
        hostname: "example.com".to_string(),
        port: 22,
        username: "admin".to_string(),
        identity_file: "~/.ssh/id_rsa".to_string(),
        ssh_options: vec![SshOption {
            key: "StrictHostKeyChecking".to_string(),
            value: "no".to_string(),
        }],
        extra_options: String::new(),
    };
    let json = serde_json::to_string(&conn).unwrap();
    let restored: SshConnection = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.name, conn.name);
    assert_eq!(restored.ssh_options.len(), 1);
    assert_eq!(restored.ssh_options[0].key, "StrictHostKeyChecking");
}

#[test]
fn test_ssh_connection_defaults() {
    let json = r#"{"name": "test", "hostname": "host.com"}"#;
    let conn: SshConnection = serde_json::from_str(json).unwrap();
    assert_eq!(conn.name, "test");
    assert_eq!(conn.hostname, "host.com");
    assert_eq!(conn.port, 22);
    assert_eq!(conn.username, "");
    assert_eq!(conn.identity_file, "");
    assert!(conn.ssh_options.is_empty());
}

#[test]
fn test_ssh_connection_backward_compat_extra_options() {
    // Old settings.json with extra_options should still deserialize
    let json = r#"{"name": "old", "hostname": "host.com", "extra_options": "-o Foo=bar"}"#;
    let conn: SshConnection = serde_json::from_str(json).unwrap();
    assert_eq!(conn.extra_options, "-o Foo=bar");
    assert!(conn.ssh_options.is_empty());
}

#[test]
fn test_ssh_connection_null_port_defaults_to_22() {
    let json = r#"{"name": "test", "hostname": "host.com", "port": null}"#;
    let conn: SshConnection = serde_json::from_str(json).unwrap();
    assert_eq!(conn.port, 22);
}

#[test]
fn test_settings_with_ssh_connections_round_trip() {
    let mut settings = AppSettings::default();
    settings.ssh_command_path = "/usr/bin/ssh".to_string();
    settings.ssh_connections = vec![
        SshConnection {
            name: "Server 1".to_string(),
            hostname: "server1.example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            identity_file: String::new(),
            ssh_options: Vec::new(),
            extra_options: String::new(),
        },
        SshConnection {
            name: "Server 2".to_string(),
            hostname: "server2.example.com".to_string(),
            port: 2222,
            username: String::new(),
            identity_file: "~/.ssh/id_ed25519".to_string(),
            ssh_options: vec![SshOption {
                key: "StrictHostKeyChecking".to_string(),
                value: "no".to_string(),
            }],
            extra_options: String::new(),
        },
    ];
    let json = serde_json::to_string(&settings).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.ssh_command_path, "/usr/bin/ssh");
    assert_eq!(restored.ssh_connections.len(), 2);
    assert_eq!(restored.ssh_connections[0].name, "Server 1");
    assert_eq!(restored.ssh_connections[1].port, 2222);
    assert_eq!(restored.ssh_connections[1].ssh_options.len(), 1);
}

#[test]
fn test_profile_ssh_connection_name_default() {
    let json = r#"{"name": "Test"}"#;
    let profile: Profile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.ssh_connection_name, "");
}

#[test]
fn test_profile_ssh_connection_name_round_trip() {
    let profile = Profile {
        name: "SSH Profile".to_string(),
        shell_path: String::new(),
        shell_args: vec![],
        env_vars: String::new(),
        working_directory: String::new(),
        is_default: false,
        ssh_connection_name: "My Server".to_string(),
        wsl_distro_name: String::new(),
    };
    let json = serde_json::to_string(&profile).unwrap();
    let restored: Profile = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.ssh_connection_name, "My Server");
}

fn make_ssh_conn(name: &str, hostname: &str, port: u16) -> SshConnection {
    SshConnection {
        name: name.to_string(),
        hostname: hostname.to_string(),
        port,
        username: String::new(),
        identity_file: String::new(),
        ssh_options: Vec::new(),
        extra_options: String::new(),
    }
}

#[test]
fn test_validate_rejects_empty_ssh_connection_name() {
    let mut settings = AppSettings::default();
    settings.ssh_connections = vec![make_ssh_conn("", "host.com", 22)];
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_empty_ssh_hostname() {
    let mut settings = AppSettings::default();
    settings.ssh_connections = vec![make_ssh_conn("Test", "", 22)];
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_port_zero() {
    let mut settings = AppSettings::default();
    settings.ssh_connections = vec![make_ssh_conn("Test", "host.com", 0)];
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_accepts_port_1() {
    let mut settings = AppSettings::default();
    settings.ssh_connections = vec![make_ssh_conn("Test", "host.com", 1)];
    assert!(validate_settings(&settings).is_ok());
}

#[test]
fn test_validate_accepts_port_65535() {
    let mut settings = AppSettings::default();
    settings.ssh_connections = vec![make_ssh_conn("Test", "host.com", 65535)];
    assert!(validate_settings(&settings).is_ok());
}

#[test]
fn test_validate_accepts_valid_ssh_connection() {
    let mut settings = AppSettings::default();
    settings.ssh_connections = vec![SshConnection {
        name: "Production".to_string(),
        hostname: "prod.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: "~/.ssh/id_ed25519".to_string(),
        ssh_options: Vec::new(),
        extra_options: String::new(),
    }];
    assert!(validate_settings(&settings).is_ok());
}
