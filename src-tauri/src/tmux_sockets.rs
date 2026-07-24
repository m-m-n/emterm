//! Discovery of live tmux sockets for the new-tab chooser's tmux-attach
//! rows (SPEC A5, task0001).
//!
//! Pure enumeration logic, no UI knowledge (IMPLEMENTATION.md layer
//! structure: App calls this, UI never does). Contract: input is the
//! process environment (the socket directory); output is a list of
//! (name, absolute path) pairs, possibly empty, and this module never
//! returns an error to the caller — a missing directory, an unreadable
//! entry, a non-socket entry, or a socket nobody is listening on all
//! degrade to "not present" rather than propagating a failure.
//!
//! No external process is spawned; discovery is a directory read plus a
//! non-blocking Unix-domain connect probe per candidate entry.

use std::ffi::CString;
use std::io;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Per-socket connect-probe timeout. Short enough that enumerating a
/// socket directory never visibly stalls chooser open, long enough to
/// tolerate ordinary scheduling jitter under load (IMPLEMENTATION.md risk:
/// "Connect probe blocks the UI thread on a pathological socket").
const PROBE_TIMEOUT: Duration = Duration::from_millis(200);

/// One discovered tmux socket: the file name tmux gave it under the
/// socket directory, and its absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSocket {
    pub name: String,
    pub path: PathBuf,
}

/// Discover live tmux sockets in the standard socket directory
/// (`$TMUX_TMPDIR`, falling back to the tmp dir named after the real
/// UID). Never fails: a missing directory yields an empty list (AC-2).
pub fn discover() -> Vec<TmuxSocket> {
    discover_in(&resolve_socket_dir())
}

/// Resolve tmux's socket directory: `$TMUX_TMPDIR` when set and
/// non-empty, else `/tmp/tmux-<real-uid>` (tmux's own fallback, per
/// `tmux(1)`).
fn resolve_socket_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TMUX_TMPDIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    // SAFETY: `getuid()` takes no arguments and cannot fail.
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/tmux-{uid}"))
}

/// Enumerate `dir` for socket-type entries that accept a Unix-domain
/// connection, returning name + absolute path pairs sorted by name for
/// deterministic ordering. A missing directory, an unreadable entry, a
/// non-socket entry, or a socket nobody is listening on are all skipped
/// silently (AC-1 / AC-2) rather than surfaced as an error.
fn discover_in(dir: &Path) -> Vec<TmuxSocket> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.file_type().is_socket() {
            continue;
        }
        if !probe_unix_socket(&path, PROBE_TIMEOUT) {
            continue;
        }
        out.push(TmuxSocket {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Attempt a non-blocking connect to the Unix-domain socket at `path`,
/// returning whether a listener accepted (or was ready to accept) the
/// connection within `timeout`. Never blocks past `timeout` regardless
/// of the target's state (missing file, stale/orphaned socket, or a
/// live listener) — the manual non-blocking connect + `poll` sequence is
/// deliberate: a plain blocking `connect(2)` to an abandoned Unix socket
/// normally fails immediately (`ECONNREFUSED`), but nothing guarantees
/// that for every pathological case, and enumerating a socket directory
/// must never risk stalling the caller (the chooser-open UI path).
fn probe_unix_socket(path: &Path, timeout: Duration) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let path_bytes = c_path.as_bytes_with_nul();

    let mut addr: libc::sockaddr_un = unsafe { mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if path_bytes.len() > addr.sun_path.len() {
        // Path too long for `sun_path`; cannot be a valid socket address.
        return false;
    }
    for (dst, src) in addr.sun_path.iter_mut().zip(path_bytes.iter()) {
        *dst = *src as libc::c_char;
    }

    // SAFETY: every raw syscall below operates on `fd`, a socket this
    // function just created, or plain-old-data buffers sized to match the
    // syscall's expectations (`addr`, `pfd`, `sock_err`). Every exit path
    // closes `fd` before returning.
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return false;
        }
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let addr_len = mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
        let ret = libc::connect(fd, (&raw const addr) as *const libc::sockaddr, addr_len);
        if ret == 0 {
            libc::close(fd);
            return true;
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EINPROGRESS) {
            libc::close(fd);
            return false;
        }

        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let poll_ret = libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int);
        if poll_ret <= 0 {
            libc::close(fd);
            return false;
        }

        let mut sock_err: libc::c_int = 0;
        let mut sock_err_len = mem::size_of::<libc::c_int>() as libc::socklen_t;
        let getopt_ret = libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&raw mut sock_err) as *mut libc::c_void,
            &mut sock_err_len,
        );
        libc::close(fd);
        getopt_ret == 0 && sock_err == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    // AC-1: a live listening socket, a stale (non-listening) socket file,
    // and a regular file are all present; only the live socket comes back.
    #[test]
    fn discover_returns_only_the_live_socket() {
        let dir = tempfile::tempdir().expect("tempdir");

        let live_path = dir.path().join("live");
        let listener = UnixListener::bind(&live_path).expect("bind live");

        let stale_path = dir.path().join("stale");
        {
            let stale_listener = UnixListener::bind(&stale_path).expect("bind stale");
            drop(stale_listener);
            // Dropping the listener closes the server side; the special
            // socket-type file `stale_path` remains on disk (bind() does
            // not unlink on close), matching a tmux server that died
            // without removing its socket.
        }
        assert!(
            stale_path.exists(),
            "stale socket file should remain on disk"
        );

        let plain_path = dir.path().join("plain.txt");
        std::fs::write(&plain_path, b"not a socket").expect("write plain file");

        let mut result = discover_in(dir.path());
        result.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(
            result,
            vec![TmuxSocket {
                name: "live".to_string(),
                path: live_path,
            }]
        );

        drop(listener);
    }

    // AC-2: a missing socket directory returns an empty list, not an error.
    #[test]
    fn discover_missing_directory_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        assert!(discover_in(&missing).is_empty());
    }

    #[test]
    fn discover_skips_non_socket_entries_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), b"x").expect("write");
        std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");
        assert!(discover_in(dir.path()).is_empty());
    }

    #[test]
    fn resolve_socket_dir_honors_tmux_tmpdir() {
        temp_env::with_var("TMUX_TMPDIR", Some("/custom/tmux/dir"), || {
            assert_eq!(resolve_socket_dir(), PathBuf::from("/custom/tmux/dir"));
        });
    }

    #[test]
    fn resolve_socket_dir_falls_back_to_uid_tmp_dir_when_unset() {
        temp_env::with_var::<&str, &str, _, _>("TMUX_TMPDIR", None, || {
            let dir = resolve_socket_dir();
            let uid = unsafe { libc::getuid() };
            assert_eq!(dir, PathBuf::from(format!("/tmp/tmux-{uid}")));
        });
    }

    #[test]
    fn resolve_socket_dir_falls_back_when_tmux_tmpdir_is_empty() {
        temp_env::with_var("TMUX_TMPDIR", Some(""), || {
            let dir = resolve_socket_dir();
            let uid = unsafe { libc::getuid() };
            assert_eq!(dir, PathBuf::from(format!("/tmp/tmux-{uid}")));
        });
    }
}
