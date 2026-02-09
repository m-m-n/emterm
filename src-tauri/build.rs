use std::process::Command;

fn main() {
    let version = git_version();
    println!("cargo::rustc-env=APP_VERSION={version}");

    // Rebuild when git HEAD or tags change
    println!("cargo::rerun-if-changed=../.git/HEAD");
    println!("cargo::rerun-if-changed=../.git/refs/tags");
    println!("cargo::rerun-if-changed=../.git/refs/heads");

    tauri_build::build()
}

fn git_version() -> String {
    // Try `git describe --tags --always --dirty`
    // Examples:
    //   Tagged commit:       "v0.2.0"    -> "0.2.0"
    //   After tagged commit: "v0.2.0-3-gabcdef7" -> "0.2.0-3-gabcdef7"
    //   No tags:             "abcdef7"   -> "0.0.0-abcdef7"
    //   Dirty working tree:  "v0.2.0-dirty" -> "0.2.0-dirty"
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // Strip leading 'v' prefix (e.g., "v0.2.0" -> "0.2.0")
            let version = raw.strip_prefix('v').unwrap_or(&raw);
            // Tag-based versions start with a digit and contain a dot (e.g., "0.2.0", "0.2.0-3-gabcdef7")
            // Hash-only versions don't (e.g., "abcdef7", "abcdef7-dirty")
            if version.starts_with(|c: char| c.is_ascii_digit()) && version.contains('.') {
                version.to_string()
            } else {
                format!("0.0.0-{version}")
            }
        }
        _ => "0.0.0-unknown".to_string(),
    }
}
