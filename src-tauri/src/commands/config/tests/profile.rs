//! Tests for the `Profile` struct (definition lives in `settings.rs`).

use super::*;

#[test]
fn test_app_settings_default_has_empty_profiles() {
    let settings = AppSettings::default();
    assert!(settings.profiles.is_empty());
}

#[test]
fn test_deserialize_missing_profiles_defaults_to_empty() {
    let json = r#"{}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(settings.profiles.is_empty());
}

#[test]
fn test_deserialize_null_profiles_defaults_to_empty() {
    let json = r#"{"profiles": null}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(settings.profiles.is_empty());
}

#[test]
fn test_profile_round_trip() {
    let profile = Profile {
        name: "My Shell".to_string(),
        shell_path: "/usr/bin/fish".to_string(),
        shell_args: vec!["-l".to_string()],
        env_vars: "TERM=xterm-256color".to_string(),
        working_directory: "/tmp".to_string(),
        is_default: false,
        ssh_connection_name: String::new(),
        wsl_distro_name: String::new(),
    };
    let json = serde_json::to_string(&profile).unwrap();
    let restored: Profile = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, profile);
}

#[test]
fn test_profile_null_fields_use_defaults() {
    let json = r#"{
        "name": "Test",
        "shell_path": null,
        "shell_args": null,
        "env_vars": null,
        "working_directory": null,
        "is_default": null
    }"#;
    let profile: Profile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.name, "Test");
    assert_eq!(profile.shell_path, "");
    assert!(profile.shell_args.is_empty());
    assert_eq!(profile.env_vars, "");
    assert_eq!(profile.working_directory, "");
    assert!(!profile.is_default);
}

#[test]
fn test_profile_missing_optional_fields_use_defaults() {
    let json = r#"{"name": "Minimal"}"#;
    let profile: Profile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.name, "Minimal");
    assert_eq!(profile.shell_path, "");
    assert!(profile.shell_args.is_empty());
    assert_eq!(profile.env_vars, "");
    assert_eq!(profile.working_directory, "");
    assert!(!profile.is_default);
}

#[test]
fn test_settings_with_profiles_round_trip() {
    let mut settings = AppSettings::default();
    settings.profiles = vec![
        Profile {
            name: "Default".to_string(),
            shell_path: "/bin/bash".to_string(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: true,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        },
        Profile {
            name: "Dev".to_string(),
            shell_path: "/bin/zsh".to_string(),
            shell_args: vec!["--login".to_string()],
            env_vars: "NODE_ENV=development".to_string(),
            working_directory: "/home/user/dev".to_string(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        },
    ];
    let json = serde_json::to_string(&settings).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.profiles.len(), 2);
    assert_eq!(restored.profiles[0].name, "Default");
    assert!(restored.profiles[0].is_default);
    assert_eq!(restored.profiles[1].name, "Dev");
    assert_eq!(restored.profiles[1].shell_path, "/bin/zsh");
}

#[test]
fn test_validate_rejects_empty_profile_name() {
    let mut settings = AppSettings::default();
    settings.profiles = vec![Profile {
        name: "".to_string(),
        shell_path: String::new(),
        shell_args: vec![],
        env_vars: String::new(),
        working_directory: String::new(),
        is_default: false,
        ssh_connection_name: String::new(),
        wsl_distro_name: String::new(),
    }];
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_whitespace_only_profile_name() {
    let mut settings = AppSettings::default();
    settings.profiles = vec![Profile {
        name: "   ".to_string(),
        shell_path: String::new(),
        shell_args: vec![],
        env_vars: String::new(),
        working_directory: String::new(),
        is_default: false,
        ssh_connection_name: String::new(),
        wsl_distro_name: String::new(),
    }];
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_accepts_valid_profiles() {
    let mut settings = AppSettings::default();
    settings.profiles = vec![
        Profile {
            name: "Shell 1".to_string(),
            shell_path: String::new(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: true,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        },
        Profile {
            name: "Shell 2".to_string(),
            shell_path: "/bin/fish".to_string(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        },
    ];
    assert!(validate_settings(&settings).is_ok());
}

// Profile with wsl_distro_name round-trip
#[test]
fn test_profile_wsl_distro_name_round_trip() {
    let profile = Profile {
        name: "WSL Ubuntu".to_string(),
        shell_path: String::new(),
        shell_args: vec![],
        env_vars: String::new(),
        working_directory: String::new(),
        is_default: false,
        ssh_connection_name: String::new(),
        wsl_distro_name: "Ubuntu-22.04".to_string(),
    };
    let json = serde_json::to_string(&profile).unwrap();
    let restored: Profile = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.wsl_distro_name, "Ubuntu-22.04");
    assert_eq!(restored.ssh_connection_name, "");
}

// Profile wsl_distro_name defaults to empty
#[test]
fn test_profile_wsl_distro_name_default() {
    let json = r#"{"name": "Test"}"#;
    let profile: Profile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.wsl_distro_name, "");
}
