//! Discovery of live tmux sockets, and enumeration of the sessions
//! inside them, for the new-tab chooser's tmux-attach rows (SPEC A5;
//! socket discovery is the predecessor feature, session enumeration is
//! task0001).
//!
//! No UI knowledge (IMPLEMENTATION.md layer structure: App calls this,
//! UI never does). This module never returns an error to the caller —
//! every failure mode (missing directory, unreadable entry, non-socket
//! entry, a socket nobody is listening on, the tmux binary missing, a
//! spawn failure, a non-zero exit, a hung child, empty or name-less
//! output) degrades to "not present" / a fallback entry rather than
//! propagating a failure or panicking.
//!
//! Socket discovery (`discover`) spawns no external process; it is a
//! directory read plus a non-blocking Unix-domain connect probe per
//! candidate entry. Session enumeration (`enumerate`) spawns one
//! bounded `tmux list-sessions` child per already-proven-live socket
//! (task0001 D3) — never a shell, always an argument vector.

use std::ffi::CString;
use std::io;
use std::io::Read;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// Per-socket session-listing timeout (task0001 NFR1 / AC-3): short
/// enough that a hung tmux server never visibly stalls chooser opening.
/// On expiry the child is killed AND reaped ([`spawn_bounded`]), never
/// left running or as a zombie.
const ENUMERATE_TIMEOUT: Duration = Duration::from_millis(300);

/// tmux format string selecting only the session name, one per line
/// (`tmux list-sessions -F <fmt>`; see `tmux(1)` FORMATS).
const SESSION_NAME_FORMAT: &str = "#{session_name}";

/// tmux's exact-match target prefix (`tmux(1)` TARGET SPECIFICATION):
/// without it, `-t name` also matches by prefix/pattern, which silently
/// attaches to the wrong session when one name prefixes another
/// (task0001 D4).
const EXACT_MATCH_PREFIX: char = '=';

/// One row the new-tab chooser should show for tmux (task0001 D2):
/// either a live session (`session = Some(name)`) discovered on
/// `socket_name` / `socket_path`, or — when that socket's sessions
/// could not be listed — a fallback row for the socket itself
/// (`session = None`). [`enumerate`] emits exactly one of these per
/// live session, or exactly one fallback per un-enumerable socket, in
/// socket-name-then-session-name order (AC-1); a fallback entry
/// occupies its socket's position in that order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxEntry {
    pub socket_name: String,
    pub socket_path: PathBuf,
    pub session: Option<String>,
}

/// Discover every tmux row the chooser should show: [`discover`]'s live
/// sockets, each expanded into one entry per session, or one fallback
/// entry when its sessions could not be listed (AC-1 / AC-2). Never
/// panics and never returns an error regardless of the environment (no
/// tmux installed, a hung server, sessions mid-crash, ...) — this
/// module's existing "never fails" contract, extended to enumeration.
pub fn enumerate() -> Vec<TmuxEntry> {
    enumerate_sockets(&discover(), "tmux", ENUMERATE_TIMEOUT)
}

/// Turn one entry into the text the chooser row shows (AC-4): `tmux:
/// {session}` for a session on the socket named `default`, `tmux:
/// {socket}: {session}` for a session on any other socket, and `tmux:
/// {socket}` for a fallback entry. A pure function of the entry so the
/// renderer (in `ui::profile_selector`) and its tests never drift
/// apart.
pub fn label(entry: &TmuxEntry) -> String {
    match &entry.session {
        Some(session) if entry.socket_name == "default" => format!("tmux: {session}"),
        Some(session) => format!("tmux: {}: {session}", entry.socket_name),
        None => format!("tmux: {}", entry.socket_name),
    }
}

/// Turn one entry into the PTY spawn argv for the `tmux` executable
/// (AC-5, D4): a session entry attaches by exact-matched target
/// (`-S <path> attach-session -t =<session>`); a fallback entry attaches
/// plainly (`-S <path> attach`). Every value is a discrete argument —
/// never concatenated into a shell string.
pub fn attach_args(entry: &TmuxEntry) -> Vec<String> {
    let mut args = vec!["-S".to_string(), entry.socket_path.display().to_string()];
    match &entry.session {
        Some(session) => {
            args.push("attach-session".to_string());
            args.push("-t".to_string());
            args.push(format!("{EXACT_MATCH_PREFIX}{session}"));
        }
        None => args.push("attach".to_string()),
    }
    args
}

/// Core of [`enumerate`], parameterized over the tmux binary (Test
/// Notes: exercising "tmux binary absent" via an unresolvable command
/// name rather than mutating the process search path globally) and the
/// per-socket timeout (tests that are not themselves exercising AC-3's
/// bound pass a generous margin so a loaded test machine can never turn
/// a fast, well-behaved stand-in script into a false timeout).
fn enumerate_sockets(sockets: &[TmuxSocket], tmux_bin: &str, timeout: Duration) -> Vec<TmuxEntry> {
    let mut out = Vec::new();
    for socket in sockets {
        match list_sessions(tmux_bin, &socket.path, timeout) {
            Some(mut names) if !names.is_empty() => {
                names.sort();
                out.extend(names.into_iter().map(|session| TmuxEntry {
                    socket_name: socket.name.clone(),
                    socket_path: socket.path.clone(),
                    session: Some(session),
                }));
            }
            _ => out.push(TmuxEntry {
                socket_name: socket.name.clone(),
                socket_path: socket.path.clone(),
                session: None,
            }),
        }
    }
    out
}

/// Ask `tmux_bin`'s server on `socket_path` for its session names,
/// bounded by `timeout` (AC-2 / AC-3). `None` covers every failure mode
/// (binary absent, spawn failure, non-zero exit, timeout) — the caller
/// ([`enumerate_sockets`]) degrades all of them to the fallback entry
/// alike. `Some(names)` may itself be empty when the server answered
/// but named no sessions; the caller treats that the same as `None`.
fn list_sessions(tmux_bin: &str, socket_path: &Path, timeout: Duration) -> Option<Vec<String>> {
    let mut cmd = Command::new(tmux_bin);
    cmd.arg("-S")
        .arg(socket_path)
        .arg("list-sessions")
        .arg("-F")
        .arg(SESSION_NAME_FORMAT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    match spawn_bounded(cmd, timeout) {
        Some(BoundedOutput::Exited {
            success: true,
            stdout,
        }) => Some(parse_session_names(&stdout)),
        Some(BoundedOutput::Exited { success: false, .. }) => {
            // Routine (e.g. a stale/crashed session file left in the
            // socket directory): must not spam the log on every chooser
            // open (IMPLEMENTATION.md Conventions).
            log::debug!("tmux list-sessions on {socket_path:?} exited non-zero");
            None
        }
        Some(BoundedOutput::TimedOut { .. }) => {
            log::debug!("tmux list-sessions on {socket_path:?} exceeded {timeout:?}");
            None
        }
        None => None,
    }
}

/// Parse `tmux list-sessions -F "#{session_name}"` output: one name per
/// line, empty lines and trailing whitespace ignored, every other
/// character preserved verbatim (AC-1).
fn parse_session_names(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Outcome of [`spawn_bounded`].
enum BoundedOutput {
    /// The child exited before `timeout` elapsed.
    Exited { success: bool, stdout: Vec<u8> },
    /// The child was killed and reaped after exceeding `timeout`.
    TimedOut {
        /// Exposed only so tests can confirm nothing survives (AC-3);
        /// production callers never read it.
        #[allow(dead_code)]
        pid: u32,
    },
}

/// Spawn `cmd` and wait up to `timeout` for it to exit, collecting its
/// stdout on a background thread so a chatty child can never deadlock
/// the bounded wait by filling its pipe before this function drains it.
/// On expiry the child is killed AND reaped (`Child::wait`), so no
/// zombie remains (AC-3). `None` only when the spawn itself failed
/// (binary absent, permission denied, ...).
fn spawn_bounded(mut cmd: Command, timeout: Duration) -> Option<BoundedOutput> {
    let mut child = cmd.spawn().ok()?;
    let pid = child.id();

    let (tx, rx) = std::sync::mpsc::channel();
    match child.stdout.take() {
        Some(mut pipe) => {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        None => {
            let _ = tx.send(Vec::new());
        }
    }

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // The child has exited, so its stdout is at EOF (or
                // about to be); the reader thread finishes almost
                // immediately. The timeout here only guards against a
                // pathological reader stall, not normal operation.
                let stdout = rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default();
                return Some(BoundedOutput::Exited {
                    success: status.success(),
                    stdout,
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Some(BoundedOutput::TimedOut { pid });
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    /// Write an executable shell script at `dir/name` with body `body`,
    /// returning its path. A test-only stand-in for the tmux binary
    /// (Test Notes: prefer a stand-in that sleeps / exits as scripted
    /// over depending on a real tmux install, and prefer pointing
    /// enumeration at an unresolvable command name over mutating the
    /// test process's search path globally).
    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write script");
        let mut perms = std::fs::metadata(&path).expect("stat script").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod script");
        path
    }

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

    // --- AC-4: label rule ---------------------------------------------

    #[test]
    fn label_session_on_default_socket_omits_socket_name() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: Some("work".to_string()),
        };
        assert_eq!(label(&entry), "tmux: work");
    }

    #[test]
    fn label_session_on_named_socket_includes_socket_name() {
        let entry = TmuxEntry {
            socket_name: "alt".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/alt"),
            session: Some("work".to_string()),
        };
        assert_eq!(label(&entry), "tmux: alt: work");
    }

    #[test]
    fn label_fallback_entry_shows_socket_name_only() {
        let entry = TmuxEntry {
            socket_name: "alt".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/alt"),
            session: None,
        };
        assert_eq!(label(&entry), "tmux: alt");
    }

    // A fallback entry on the socket literally named `default` still
    // uses the `None` branch (no session to gate the "default" special
    // case on).
    #[test]
    fn label_fallback_entry_on_default_socket_still_shows_socket_name() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: None,
        };
        assert_eq!(label(&entry), "tmux: default");
    }

    // --- AC-5: attach-argument rule -------------------------------------

    #[test]
    fn attach_args_session_entry_targets_exact_match() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: Some("work".to_string()),
        };
        assert_eq!(
            attach_args(&entry),
            vec![
                "-S".to_string(),
                "/tmp/tmux-1000/default".to_string(),
                "attach-session".to_string(),
                "-t".to_string(),
                "=work".to_string(),
            ]
        );
    }

    #[test]
    fn attach_args_fallback_entry_uses_plain_attach() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: None,
        };
        assert_eq!(
            attach_args(&entry),
            vec![
                "-S".to_string(),
                "/tmp/tmux-1000/default".to_string(),
                "attach".to_string(),
            ]
        );
    }

    // Edge case: "work" is a prefix of "work2"; only the exact-match
    // marker distinguishes the target, never the argument shape.
    #[test]
    fn attach_args_prefix_session_name_still_gets_exact_match_marker() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: Some("work".to_string()),
        };
        let other = TmuxEntry {
            session: Some("work2".to_string()),
            ..entry.clone()
        };
        assert_eq!(attach_args(&entry).last(), Some(&"=work".to_string()));
        assert_eq!(attach_args(&other).last(), Some(&"=work2".to_string()));
    }

    #[test]
    fn attach_args_session_name_with_space_is_one_argument() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: Some("my session".to_string()),
        };
        let args = attach_args(&entry);
        assert_eq!(args.last(), Some(&"=my session".to_string()));
        assert_eq!(
            args.len(),
            5,
            "the space must not split the name into extra argv elements"
        );
    }

    #[test]
    fn attach_args_non_ascii_session_name_preserved_verbatim() {
        let entry = TmuxEntry {
            socket_name: "default".to_string(),
            socket_path: PathBuf::from("/tmp/tmux-1000/default"),
            session: Some("作業".to_string()),
        };
        assert_eq!(attach_args(&entry).last(), Some(&"=作業".to_string()));
    }

    // --- AC-1: session-name parsing --------------------------------------

    #[test]
    fn parse_session_names_skips_blank_and_whitespace_only_lines() {
        let names = parse_session_names(b"zeta\n\n   \nbeta\n");
        assert_eq!(names, vec!["zeta".to_string(), "beta".to_string()]);
    }

    #[test]
    fn parse_session_names_trims_trailing_whitespace_verbatim_otherwise() {
        let names = parse_session_names(b"has space \r\nplain\n");
        assert_eq!(names, vec!["has space".to_string(), "plain".to_string()]);
    }

    #[test]
    fn parse_session_names_empty_output_is_empty() {
        assert!(parse_session_names(b"").is_empty());
    }

    // --- AC-1 / AC-2: enumerate_sockets -----------------------------------

    /// A generous per-socket timeout for tests that are not themselves
    /// exercising AC-3's bound: these stand-in scripts respond almost
    /// instantly under normal conditions, but a heavily loaded test
    /// machine running the full suite in parallel can occasionally delay
    /// scheduling past a tight bound, which would otherwise turn into a
    /// false timeout unrelated to what the test is checking.
    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn enumerate_orders_by_socket_then_session_and_skips_blank_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = write_script(
            dir.path(),
            "fake-tmux.sh",
            r#"#!/bin/sh
case "$2" in
    *alpha) printf 'zeta\n\n   \nbeta\n' ;;
    *zulu) printf 'only\n' ;;
esac
"#,
        );
        let sockets = vec![
            TmuxSocket {
                name: "alpha".to_string(),
                path: dir.path().join("alpha"),
            },
            TmuxSocket {
                name: "zulu".to_string(),
                path: dir.path().join("zulu"),
            },
        ];
        let entries = enumerate_sockets(&sockets, script.to_str().expect("utf8 path"), TEST_TIMEOUT);
        assert_eq!(
            entries,
            vec![
                TmuxEntry {
                    socket_name: "alpha".to_string(),
                    socket_path: dir.path().join("alpha"),
                    session: Some("beta".to_string()),
                },
                TmuxEntry {
                    socket_name: "alpha".to_string(),
                    socket_path: dir.path().join("alpha"),
                    session: Some("zeta".to_string()),
                },
                TmuxEntry {
                    socket_name: "zulu".to_string(),
                    socket_path: dir.path().join("zulu"),
                    session: Some("only".to_string()),
                },
            ]
        );
    }

    // AC-2: every failure mode (non-zero exit, empty output, output
    // with only blank lines) degrades that socket to exactly one
    // fallback entry, never a panic, never an error.
    #[test]
    fn enumerate_degrades_each_failure_mode_to_one_fallback_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = write_script(
            dir.path(),
            "fake-tmux.sh",
            r#"#!/bin/sh
case "$2" in
    *nonzero) exit 1 ;;
    *empty) exit 0 ;;
    *blank) printf '\n   \n' ;;
    *) exit 0 ;;
esac
"#,
        );
        let sockets = vec![
            TmuxSocket {
                name: "nonzero".to_string(),
                path: dir.path().join("nonzero"),
            },
            TmuxSocket {
                name: "empty".to_string(),
                path: dir.path().join("empty"),
            },
            TmuxSocket {
                name: "blank".to_string(),
                path: dir.path().join("blank"),
            },
        ];
        let entries = enumerate_sockets(&sockets, script.to_str().expect("utf8 path"), TEST_TIMEOUT);
        assert_eq!(
            entries,
            vec![
                TmuxEntry {
                    socket_name: "nonzero".to_string(),
                    socket_path: dir.path().join("nonzero"),
                    session: None,
                },
                TmuxEntry {
                    socket_name: "empty".to_string(),
                    socket_path: dir.path().join("empty"),
                    session: None,
                },
                TmuxEntry {
                    socket_name: "blank".to_string(),
                    socket_path: dir.path().join("blank"),
                    session: None,
                },
            ]
        );
    }

    // AC-2: tmux binary absent entirely (an unresolvable command name)
    // also degrades to a fallback entry, never a panic or error.
    #[test]
    fn enumerate_tmux_binary_absent_degrades_to_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = vec![TmuxSocket {
            name: "dev".to_string(),
            path: dir.path().join("dev"),
        }];
        let entries = enumerate_sockets(&sockets, "emterm-test-nonexistent-tmux-binary-xyz", TEST_TIMEOUT);
        assert_eq!(
            entries,
            vec![TmuxEntry {
                socket_name: "dev".to_string(),
                socket_path: dir.path().join("dev"),
                session: None,
            }]
        );
    }

    // AC-2: spawn failure (a path that exists but lacks the execute
    // bit) also degrades to a fallback entry.
    #[test]
    fn enumerate_spawn_failure_degrades_to_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let not_executable = dir.path().join("not-a-tmux");
        std::fs::write(&not_executable, b"not executable").expect("write");
        let sockets = vec![TmuxSocket {
            name: "dev".to_string(),
            path: dir.path().join("dev"),
        }];
        let entries = enumerate_sockets(&sockets, not_executable.to_str().expect("utf8 path"), TEST_TIMEOUT);
        assert_eq!(
            entries,
            vec![TmuxEntry {
                socket_name: "dev".to_string(),
                socket_path: dir.path().join("dev"),
                session: None,
            }]
        );
    }

    // Edge case: the same session name on two different sockets stays
    // two distinct entries, disambiguated by socket in the label.
    #[test]
    fn enumerate_keeps_same_session_name_on_two_sockets_distinct() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = write_script(dir.path(), "fake-tmux.sh", "#!/bin/sh\nprintf 'work\\n'\n");
        let sockets = vec![
            TmuxSocket {
                name: "alpha".to_string(),
                path: dir.path().join("alpha"),
            },
            TmuxSocket {
                name: "beta".to_string(),
                path: dir.path().join("beta"),
            },
        ];
        let entries = enumerate_sockets(&sockets, script.to_str().expect("utf8 path"), TEST_TIMEOUT);
        assert_eq!(
            entries,
            vec![
                TmuxEntry {
                    socket_name: "alpha".to_string(),
                    socket_path: dir.path().join("alpha"),
                    session: Some("work".to_string()),
                },
                TmuxEntry {
                    socket_name: "beta".to_string(),
                    socket_path: dir.path().join("beta"),
                    session: Some("work".to_string()),
                },
            ]
        );
        assert_eq!(label(&entries[0]), "tmux: alpha: work");
        assert_eq!(label(&entries[1]), "tmux: beta: work");
    }

    // Edge case: a session name containing a space survives enumeration
    // verbatim (parsing must not split on internal whitespace).
    #[test]
    fn enumerate_session_name_with_embedded_space_preserved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = write_script(
            dir.path(),
            "fake-tmux.sh",
            "#!/bin/sh\nprintf 'my session\\n'\n",
        );
        let sockets = vec![TmuxSocket {
            name: "alpha".to_string(),
            path: dir.path().join("alpha"),
        }];
        let entries = enumerate_sockets(&sockets, script.to_str().expect("utf8 path"), TEST_TIMEOUT);
        assert_eq!(entries[0].session.as_deref(), Some("my session"));
    }

    // --- AC-3: bounded wait, kill + reap ----------------------------------

    #[test]
    fn spawn_bounded_kills_and_reaps_a_child_that_never_answers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = write_script(dir.path(), "hang.sh", "#!/bin/sh\nsleep 5\n");
        let mut cmd = Command::new(&script);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let start = Instant::now();
        let outcome = spawn_bounded(cmd, Duration::from_millis(300));
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(1_000),
            "took too long: {elapsed:?}"
        );
        let BoundedOutput::TimedOut { pid } = outcome.expect("spawn succeeded") else {
            panic!("expected a timeout outcome, got an exit");
        };

        // The child must be fully reaped, not merely signaled: `wait()`
        // having already collected its exit status means the kernel has
        // released the pid entirely, so `kill(pid, 0)` reports ESRCH
        // rather than "found a zombie".
        let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
        let err = io::Error::last_os_error();
        assert_eq!(ret, -1, "expected the process to be gone");
        assert_eq!(err.raw_os_error(), Some(libc::ESRCH));
    }

    // AC-3 at the `list_sessions` level: a hung server yields `None`
    // (fallback) within the bound, exercised through the call path
    // enumeration actually uses.
    #[test]
    fn list_sessions_on_a_hung_server_returns_none_within_the_bound() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = write_script(dir.path(), "hang.sh", "#!/bin/sh\nsleep 5\n");
        let start = Instant::now();
        let result = list_sessions(
            script.to_str().expect("utf8 path"),
            Path::new("/nonexistent-socket"),
            Duration::from_millis(300),
        );
        let elapsed = start.elapsed();
        assert!(result.is_none());
        assert!(
            elapsed < Duration::from_millis(1_000),
            "took too long: {elapsed:?}"
        );
    }
}
