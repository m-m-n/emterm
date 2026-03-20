//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` — Start/attach to default session
//! - `emterm mux --daemon` — Run as daemon process (internal)
//! - `emterm mux attach [session]` — Attach to existing session
//! - `emterm mux ls` — List sessions
//! - `emterm mux kill [session]` — Kill a session
//! - `emterm mux new [name]` — Create a new session

use super::daemon;

/// Check if running inside eMterm (TERM_PROGRAM=emterm).
fn check_emterm_environment() -> Result<(), String> {
    match std::env::var("TERM_PROGRAM") {
        Ok(val) if val == "emterm" => Ok(()),
        _ => Err("emterm mux must be run inside eMterm terminal".to_string()),
    }
}

/// Check for nesting (EMTERM_MUX=1).
fn check_nesting() -> Result<(), String> {
    if std::env::var("EMTERM_MUX").is_ok() {
        Err("Cannot nest mux sessions (EMTERM_MUX is set)".to_string())
    } else {
        Ok(())
    }
}

/// Execute the `emterm mux --daemon` command (runs the daemon).
pub fn execute_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon())?;
    Ok(())
}

/// Execute the `emterm mux` command (start/attach).
pub fn execute_mux() -> Result<(), Box<dyn std::error::Error>> {
    check_emterm_environment().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    check_nesting().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let sock_path = daemon::socket_path();

    // Start daemon if not running
    if !sock_path.exists() || !daemon::is_daemon_running(&sock_path) {
        // Spawn daemon as background process
        let exe = std::env::current_exe()?;
        let _child = std::process::Command::new(exe)
            .args(["mux", "--daemon"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        // Wait for daemon to start
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if daemon::is_daemon_running(&sock_path) {
                break;
            }
        }

        if !daemon::is_daemon_running(&sock_path) {
            return Err("Failed to start mux daemon".into());
        }
    }

    // Output OSC sequence to signal GUI
    let sock_str = sock_path.to_string_lossy();
    // session_id 0 = create/attach default session
    print!("\x1b]777;emterm;mux;attach;{};0\x1b\\", sock_str);

    // Connect to daemon for detach notification
    // (blocking until detached or daemon exits)
    #[cfg(unix)]
    {
        let stream = std::os::unix::net::UnixStream::connect(&sock_path)?;
        stream.set_read_timeout(None)?;

        // Simple blocking read — daemon will close our connection on detach
        let mut reader = std::io::BufReader::new(stream);
        let mut buf = [0u8; 1024];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) => break, // Connection closed (detach or daemon exit)
                Ok(_) => {}     // Ignore data (CLI doesn't process it)
                Err(_) => break,
            }
        }
    }

    #[cfg(windows)]
    {
        // TODO: Windows named pipe support for mux daemon connection
        return Err("Mux is not yet supported on Windows".into());
    }

    Ok(())
}

/// Execute the `emterm mux ls` command.
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    // TODO: Connect and request session list
    println!("(session listing not yet implemented)");
    Ok(())
}

/// Execute the `emterm mux kill` command.
pub fn execute_kill(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Connect and send kill request
    println!("(kill not yet implemented)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_nesting_not_set() {
        // In test environment, EMTERM_MUX should not be set
        unsafe { std::env::remove_var("EMTERM_MUX") };
        assert!(check_nesting().is_ok());
    }

    #[test]
    fn test_check_nesting_set() {
        unsafe { std::env::set_var("EMTERM_MUX", "1") };
        assert!(check_nesting().is_err());
        unsafe { std::env::remove_var("EMTERM_MUX") };
    }

    #[test]
    fn test_check_emterm_not_set() {
        unsafe { std::env::remove_var("TERM_PROGRAM") };
        assert!(check_emterm_environment().is_err());
    }

    #[test]
    fn test_check_emterm_set() {
        unsafe { std::env::set_var("TERM_PROGRAM", "emterm") };
        assert!(check_emterm_environment().is_ok());
        unsafe { std::env::remove_var("TERM_PROGRAM") };
    }

    #[test]
    fn test_check_emterm_wrong_value() {
        unsafe { std::env::set_var("TERM_PROGRAM", "other-terminal") };
        assert!(check_emterm_environment().is_err());
        unsafe { std::env::remove_var("TERM_PROGRAM") };
    }
}
