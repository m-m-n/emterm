//! Terminal raw-mode control for the bridge process: termios (Unix) /
//! console-mode (Windows) save, raw switch, and RAII restore guards.

use super::*;

/// Global storage for original termios, so we can restore it before process::exit().
#[cfg(unix)]
static ORIGINAL_TERMIOS: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

/// Restore stdin from the global original termios (safe to call from any context).
#[cfg(unix)]
pub(in crate::mux::bridge) fn restore_stdin_global() {
    if let Some(orig) = ORIGINAL_TERMIOS.get() {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, orig);
        }
        log::info!("stdin restored from global termios");
    }
}

/// Set stdin to raw mode (non-canonical, no echo) so APC bytes arrive immediately.
/// Returns the original termios for restoration on exit.
#[cfg(unix)]
pub(in crate::mux::bridge) fn set_stdin_raw() -> Option<libc::termios> {
    use std::mem::MaybeUninit;
    unsafe {
        let mut orig = MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(libc::STDIN_FILENO, orig.as_mut_ptr()) != 0 {
            log::warn!("tcgetattr failed, stdin may not be a tty");
            return None;
        }
        let orig = orig.assume_init();
        // Store in global so process::exit() path can restore it
        let _ = ORIGINAL_TERMIOS.set(orig);
        let mut raw = orig;
        libc::cfmakeraw(&mut raw);
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
            log::warn!("tcsetattr failed");
            return None;
        }
        log::info!("stdin set to raw mode");
        Some(orig)
    }
}

/// Restore original termios settings.
#[cfg(unix)]
fn restore_stdin(orig: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, orig);
    }
    log::info!("stdin restored to original mode");
}

/// RAII guard that restores terminal settings on drop.
#[cfg(unix)]
pub(in crate::mux::bridge) struct RawModeGuard(pub(in crate::mux::bridge) Option<libc::termios>);

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let Some(ref orig) = self.0 {
            restore_stdin(orig);
        }
    }
}

/// Global storage for original console mode (Windows).
#[cfg(windows)]
static ORIGINAL_CONSOLE_MODE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

/// Restore stdin from the global console mode (Windows).
#[cfg(windows)]
pub(in crate::mux::bridge) fn restore_stdin_windows_global() {
    if let Some(&mode) = ORIGINAL_CONSOLE_MODE.get() {
        restore_stdin_windows(mode);
        log::info!("stdin restored from global console mode");
    }
}

/// Set stdin to raw mode on Windows (enable VT input processing).
/// Returns the original console mode for restoration on exit.
#[cfg(windows)]
pub(in crate::mux::bridge) fn set_stdin_raw_windows() -> Option<u32> {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE as _ {
            log::warn!("GetStdHandle failed, stdin may not be a console");
            return None;
        }
        let mut original_mode: u32 = 0;
        if GetConsoleMode(handle, &mut original_mode) == 0 {
            log::warn!("GetConsoleMode failed, stdin may not be a console");
            return None;
        }
        // Store in global so process::exit() path can restore it
        let _ = ORIGINAL_CONSOLE_MODE.set(original_mode);
        if SetConsoleMode(handle, ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
            log::warn!("SetConsoleMode failed");
            return None;
        }
        log::info!("stdin set to raw mode (VT input)");
        Some(original_mode)
    }
}

/// Restore original console mode on Windows.
#[cfg(windows)]
fn restore_stdin_windows(original_mode: u32) {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        SetConsoleMode(handle, original_mode);
    }
    log::info!("stdin restored to original mode");
}

/// RAII guard that restores console mode on drop (Windows).
#[cfg(windows)]
pub(in crate::mux::bridge) struct RawModeGuardWindows(pub(in crate::mux::bridge) Option<u32>);

#[cfg(windows)]
impl Drop for RawModeGuardWindows {
    fn drop(&mut self) {
        if let Some(mode) = self.0 {
            restore_stdin_windows(mode);
        }
    }
}
