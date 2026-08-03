//! Daemon start-binary identity: record-or-invalidate at startup, and the
//! client-side comparison verdict used to detect a binary-update (task0001;
//! IMPLEMENTATION.md Shared Components "mux::identity").
//!
//! **Why this exists**: after a `rename(2)` binary replacement (e.g.
//! `apt`/`dpkg`), a running process's own `current_exe()` resolution starts
//! returning a `(deleted)` path (see `crate::self_exec`'s module doc for the
//! same phenomenon on the GUI side). Fresh executable-path resolution can
//! therefore never be the daemon's hot-upgrade exec target once the old
//! binary has been replaced — it would resolve to the deleted path and
//! re-launch the SAME old image. This module instead records the daemon's
//! executable identity (clean path + `(device, inode)`) once, at every
//! startup, to a small file next to the listen socket; that recorded value
//! — never a fresh resolution — is what a client-side probe compares
//! against to decide "has the on-disk binary changed since this daemon
//! started", and what the daemon's own upgrade branch resolves its exec
//! candidate from (Design D3/D4).
//!
//! Everything in this module is Unix-only (gated at the `mod identity;`
//! declaration in `mux::mod`) — device/inode identity and the hardening
//! primitives it depends on (`O_NOFOLLOW`, `libc::mode_t` permissions) have
//! no Windows equivalent, matching the `upgrade` / `inherited_pty` precedent.

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

/// Identity file name, placed alongside the daemon's listen socket (same
/// owner-only 0o700 directory — see `daemon::socket_path` / `daemon::run_daemon`).
const IDENTITY_FILE_NAME: &str = "mux-identity.txt";

/// Format-version marker, checked byte-for-byte at the start of the file.
/// Any content that doesn't begin with this exact magic is malformed
/// (folds into [`CheckVerdict::Undecidable`] at the [`check`] call site).
const IDENTITY_MAGIC: &[u8] = b"MUXID1\n";

/// Size, in bytes, of the fixed-width header following the magic marker:
/// `dev` (8 bytes) + `ino` (8 bytes) + `path_len` (8 bytes).
const HEADER_LEN: usize = 24;

/// The daemon's own start-binary identity: recorded to disk at startup by
/// [`record_or_invalidate`] and kept in-process (by the caller) for the
/// daemon's lifetime, so the upgrade branch can resolve its exec candidate
/// from it without re-reading the file (Design D3/D4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedIdentity {
    /// The clean executable path resolved at the daemon's own startup.
    pub path: PathBuf,
    pub dev: u64,
    pub ino: u64,
}

/// Verdict returned by [`check`] (client side; Design "Comparison
/// predicate"). A verdict of `Updated` is NEVER produced from a parse
/// failure or a non-not-found stat error — both of those always fold into
/// `Undecidable` (FR7: an undecidable comparison never fires an upgrade).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckVerdict {
    /// The recorded `(device, inode)` equals the current stat of the
    /// recorded path.
    Unchanged,
    /// The recorded path's `(device, inode)` no longer matches, or the
    /// path no longer exists. Carries the recorded clean executable path.
    Updated { path: PathBuf },
    /// The identity file is missing, unreadable, malformed, or truncated;
    /// or stat-ing the recorded path failed with something other than
    /// "not found".
    Undecidable,
}

/// Absolute path of the identity file for the daemon whose listen socket
/// lives at `socket_path` (pure function of the socket path — the sibling
/// file named [`IDENTITY_FILE_NAME`] in the socket's own directory).
pub fn identity_file_path(socket_path: &Path) -> PathBuf {
    socket_path.with_file_name(IDENTITY_FILE_NAME)
}

/// The same-directory temp path used for atomic replacement (mirrors
/// `mux::upgrade::rewrite_handoff_file`'s `.tmp` sibling-file convention).
fn temp_identity_file_path(socket_path: &Path) -> PathBuf {
    let path = identity_file_path(socket_path);
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    tmp_name.push(".tmp");
    path.with_file_name(tmp_name)
}

/// Serialize a recorded identity: magic marker, `dev` (8 bytes little-
/// endian), `ino` (8 bytes little-endian), `path` byte length (8 bytes
/// little-endian), then the raw path bytes. A length-prefixed path (rather
/// than a newline- or NUL-delimited one) round-trips any valid Unix path
/// byte-for-byte, including embedded whitespace, without needing an escape
/// scheme.
fn encode(identity: &RecordedIdentity) -> Vec<u8> {
    let path_bytes = identity.path.as_os_str().as_bytes();
    let mut out = Vec::with_capacity(IDENTITY_MAGIC.len() + HEADER_LEN + path_bytes.len());
    out.extend_from_slice(IDENTITY_MAGIC);
    out.extend_from_slice(&identity.dev.to_le_bytes());
    out.extend_from_slice(&identity.ino.to_le_bytes());
    out.extend_from_slice(&(path_bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(path_bytes);
    out
}

/// Decode failure detail. Kept private -- [`check`]'s callers only ever see
/// [`CheckVerdict::Undecidable`]; this is structured for the unit test
/// table proving each rejection reason is actually reached.
#[derive(Debug, PartialEq, Eq)]
enum DecodeError {
    /// Fewer bytes than the magic marker, the fixed header, or the
    /// declared path length require.
    Truncated,
    /// The leading bytes don't match [`IDENTITY_MAGIC`].
    BadMagic,
    /// More bytes are present than the declared path length accounts for.
    TrailingGarbage,
}

/// Deserialize [`encode`]'s format. Rejects a short read, a wrong magic
/// marker, and any trailing bytes after the declared path length (Design
/// "Identity file": "must reject truncated or trailing-garbage content
/// into the Undecidable verdict").
fn decode(bytes: &[u8]) -> Result<RecordedIdentity, DecodeError> {
    if bytes.len() < IDENTITY_MAGIC.len() {
        return Err(DecodeError::Truncated);
    }
    let (magic, rest) = bytes.split_at(IDENTITY_MAGIC.len());
    if magic != IDENTITY_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    if rest.len() < HEADER_LEN {
        return Err(DecodeError::Truncated);
    }
    let (dev_bytes, rest) = rest.split_at(8);
    let (ino_bytes, rest) = rest.split_at(8);
    let (len_bytes, rest) = rest.split_at(8);
    let dev = u64::from_le_bytes(dev_bytes.try_into().expect("split_at(8) yields 8 bytes"));
    let ino = u64::from_le_bytes(ino_bytes.try_into().expect("split_at(8) yields 8 bytes"));
    let path_len = u64::from_le_bytes(len_bytes.try_into().expect("split_at(8) yields 8 bytes"))
        as usize;
    if rest.len() < path_len {
        return Err(DecodeError::Truncated);
    }
    if rest.len() > path_len {
        return Err(DecodeError::TrailingGarbage);
    }
    let path = PathBuf::from(OsStr::from_bytes(rest));
    Ok(RecordedIdentity { path, dev, ino })
}

/// Refuse to proceed if `path` currently names a symlink, WITHOUT following
/// it and WITHOUT removing it (NFR3 hardening). `Ok(())` when `path` does
/// not exist, exists as something other than a symlink, or its metadata
/// could not be read for some other reason (the caller's own subsequent
/// operation then surfaces that error itself).
fn refuse_if_symlink(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to follow or replace a symlink at {path:?}"),
        )),
        _ => Ok(()),
    }
}

/// Clear a stale, non-symlink temp file possibly left behind by a crashed
/// previous write (this module's own writer always leaves a clean 0o600
/// regular file there — see [`create_identity_temp_file`]). A pre-placed
/// SYMLINK at the temp path is refused rather than removed or followed
/// (NFR3 hardening / AC-5): only a genuine leftover regular file from this
/// module's own prior run is cleared here.
fn clear_stale_temp_file(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("refusing to remove a symlink at temp path {path:?}"),
                ));
            }
            std::fs::remove_file(path)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Open the identity temp file fresh: owner-only permission, refusing to
/// follow an existing path — `O_NOFOLLOW` + `create_new` + `mode(0o600)`,
/// mirroring `mux::upgrade::create_handoff_file`'s hardening convention for
/// files in the same directory.
fn create_identity_temp_file(path: &Path) -> io::Result<std::fs::File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    opts.mode(0o600);
    opts.custom_flags(libc::O_NOFOLLOW);
    opts.open(path)
}

/// Write `bytes` to `file`, removing `path` if the write itself fails
/// (mirrors `mux::upgrade::write_bytes_or_remove`: a partially written temp
/// file never survives its own failure).
fn write_bytes_or_remove(mut file: std::fs::File, path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Err(e) = file.write_all(bytes) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(())
}

/// Atomically replace the identity file next to `socket_path` with
/// `identity`'s serialized form (Design "Identity file" hardening:
/// owner-only 0o600, symlink refusal at both the temp and final path,
/// rename-into-place so a concurrent reader of [`check`] never observes a
/// torn write — it sees either the old complete content or the new
/// complete content).
fn write_identity_file(identity: &RecordedIdentity, socket_path: &Path) -> io::Result<()> {
    let path = identity_file_path(socket_path);
    let tmp_path = temp_identity_file_path(socket_path);

    clear_stale_temp_file(&tmp_path)?;
    let file = create_identity_temp_file(&tmp_path)?;
    let bytes = encode(identity);
    write_bytes_or_remove(file, &tmp_path, &bytes)?;

    if let Err(e) = refuse_if_symlink(&path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// Record-or-invalidate (Design D4): called once per daemon startup, on
/// EVERY startup route (fresh bind, post-execve handoff start, failed-exec
/// re-entry), after the socket parent directory is already ensured by the
/// caller.
///
/// `resolved_exe` is the running process's executable path, already
/// resolved by the caller (typically `std::env::current_exe().ok()`) --
/// taking it as a parameter, rather than resolving it internally, is what
/// keeps this function unit-testable without manipulating `/proc`: a path
/// that does not exist simulates a resolution/stat failure exactly as well
/// as a genuine `current_exe()` failure would (e.g. the post-rename
/// `(deleted)` state during a failed-exec re-entry).
///
/// `resolved_exe` stats cleanly -> the identity file is (re)written and the
/// recorded identity is returned for the caller to keep in-process.
/// `resolved_exe` is `None`, or the stat fails -> any existing identity
/// file is removed and `None` is returned (the fire-loop breaker for a
/// failed-exec re-entry, D4 rule 2: the next probe reports Undecidable
/// instead of re-firing a doomed upgrade).
///
/// A persistence failure (the write itself erroring, e.g. a pre-placed
/// symlink at the identity file path) is logged at warn and does NOT
/// propagate -- daemon startup must never abort over this (best-effort
/// policy); the existing file is best-effort removed in that case too, so
/// a write failure never leaves a stale, now-wrong identity file behind.
pub fn record_or_invalidate(
    resolved_exe: Option<&Path>,
    socket_path: &Path,
) -> Option<RecordedIdentity> {
    let captured =
        resolved_exe.and_then(|p| std::fs::metadata(p).ok().map(|meta| (p.to_path_buf(), meta)));

    let Some((path, meta)) = captured else {
        let _ = std::fs::remove_file(identity_file_path(socket_path));
        return None;
    };

    let identity = RecordedIdentity {
        path,
        dev: meta.dev(),
        ino: meta.ino(),
    };

    if let Err(e) = write_identity_file(&identity, socket_path) {
        log::warn!(
            "mux identity: failed to record daemon identity ({e}); continuing with no \
             recorded identity"
        );
        let _ = std::fs::remove_file(identity_file_path(socket_path));
        return None;
    }

    Some(identity)
}

/// Read and decode the identity file at `socket_path`'s sibling location.
/// Any I/O failure or decode failure collapses into a single opaque
/// "unusable" outcome -- [`check`] maps that straight to
/// [`CheckVerdict::Undecidable`], never distinguishing the reason (Design:
/// "identity file missing / unreadable / malformed / truncated" are all one
/// verdict).
fn read_identity_file(socket_path: &Path) -> Result<RecordedIdentity, ()> {
    let path = identity_file_path(socket_path);
    let bytes = std::fs::read(&path).map_err(|_| ())?;
    decode(&bytes).map_err(|_| ())
}

/// Pure comparison predicate (Design "Comparison predicate", table-tested
/// in the style of `self_exec`'s `is_missing` test group). `recorded` is
/// the identity read from the identity file; `current` is the result of
/// stat-ing the recorded path's CURRENT `(device, inode)`: `Ok(Some(_))` on
/// a successful stat, `Ok(None)` when the path was not found, `Err(())`
/// when the stat failed for any other reason (never used to produce
/// `Updated` -- NFR2-style caution, mirroring `self_exec::is_missing`'s own
/// "any other stat error falls back" policy, except here the safe fallback
/// is `Undecidable` rather than `false`).
fn compare(recorded: &RecordedIdentity, current: Result<Option<(u64, u64)>, ()>) -> CheckVerdict {
    match current {
        Ok(Some(dev_ino)) if dev_ino == (recorded.dev, recorded.ino) => CheckVerdict::Unchanged,
        Ok(Some(_)) => CheckVerdict::Updated {
            path: recorded.path.clone(),
        },
        Ok(None) => CheckVerdict::Updated {
            path: recorded.path.clone(),
        },
        Err(()) => CheckVerdict::Undecidable,
    }
}

/// Client-side check (Design "Comparison predicate"; NFR1 cost bound: at
/// most one small-file read plus one stat of the recorded path, nothing
/// else).
pub fn check(socket_path: &Path) -> CheckVerdict {
    let Ok(recorded) = read_identity_file(socket_path) else {
        return CheckVerdict::Undecidable;
    };
    let current = match std::fs::metadata(&recorded.path) {
        Ok(meta) => Ok(Some((meta.dev(), meta.ino()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    };
    compare(&recorded, current)
}

/// Resolve the upgrade exec candidate from the daemon's in-process recorded
/// identity (Design D3: the exec target comes EXCLUSIVELY from the
/// recorded identity, never from fresh executable-path resolution -- that
/// route resolves to a "(deleted)" path after a rename-replacement).
/// `recorded` is `None` -> refused with a human-readable reason (AC-6); the
/// daemon's upgrade-signal branch sends this reason through the existing
/// refusal reply channel and keeps serving with panes intact. Kept as a
/// small pure helper (no daemon state, no I/O) so the "recorded identity or
/// refusal reason" decision is table-testable without a live daemon.
pub fn resolve_upgrade_candidate(recorded: Option<&RecordedIdentity>) -> Result<PathBuf, String> {
    match recorded {
        Some(identity) => Ok(identity.path.clone()),
        None => Err(
            "no daemon identity was recorded at startup; refusing the upgrade request"
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    // ── AC-1: after a clean startup record, the identity file exists next
    // to the socket with mode 0o600 and round-trips through `check` ───────

    #[test]
    fn record_or_invalidate_writes_owner_only_file_that_round_trips_through_check() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let exe_stand_in = tempfile::NamedTempFile::new_in(dir.path()).expect("exe stand-in");

        let recorded = record_or_invalidate(Some(exe_stand_in.path()), &socket_path)
            .expect("a cleanly stat-able path must record an identity");
        assert_eq!(recorded.path, exe_stand_in.path());

        let identity_path = identity_file_path(&socket_path);
        assert!(identity_path.exists(), "identity file must exist next to the socket");
        let mode = std::fs::metadata(&identity_path)
            .expect("identity file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "identity file must be owner-only 0o600");

        assert_eq!(
            check(&socket_path),
            CheckVerdict::Unchanged,
            "an untouched recorded path must round-trip as Unchanged"
        );
    }

    // ── AC-2: a non-stat-able capture removes any pre-existing identity
    // file and records nothing, without aborting (returns `None`) ─────────

    #[test]
    fn record_or_invalidate_removes_existing_file_and_records_nothing_when_capture_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let exe_stand_in = tempfile::NamedTempFile::new_in(dir.path()).expect("exe stand-in");

        record_or_invalidate(Some(exe_stand_in.path()), &socket_path)
            .expect("first startup must record cleanly");
        assert!(identity_file_path(&socket_path).exists());

        // Simulate a failed-exec re-entry: the resolved path no longer
        // stats cleanly (parameterized directly, rather than manipulating
        // /proc, per the task plan's test note).
        let gone = dir.path().join("emterm (deleted)");
        let result = record_or_invalidate(Some(&gone), &socket_path);

        assert!(result.is_none(), "a non-stat-able capture must record nothing");
        assert!(
            !identity_file_path(&socket_path).exists(),
            "a pre-existing identity file must be removed"
        );
    }

    #[test]
    fn record_or_invalidate_removes_existing_file_when_resolution_itself_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let exe_stand_in = tempfile::NamedTempFile::new_in(dir.path()).expect("exe stand-in");

        record_or_invalidate(Some(exe_stand_in.path()), &socket_path)
            .expect("first startup must record cleanly");
        assert!(identity_file_path(&socket_path).exists());

        // `None` simulates `current_exe()` itself returning `Err`.
        let result = record_or_invalidate(None, &socket_path);

        assert!(result.is_none());
        assert!(!identity_file_path(&socket_path).exists());
    }

    // ── AC-3: the comparison predicate's full table ───────────────────────

    fn recorded(dev: u64, ino: u64) -> RecordedIdentity {
        RecordedIdentity {
            path: PathBuf::from("/usr/bin/emterm"),
            dev,
            ino,
        }
    }

    #[test]
    fn compare_reports_unchanged_when_dev_ino_match() {
        let r = recorded(7, 42);
        assert_eq!(compare(&r, Ok(Some((7, 42)))), CheckVerdict::Unchanged);
    }

    #[test]
    fn compare_reports_updated_when_dev_ino_differ() {
        let r = recorded(7, 42);
        assert_eq!(
            compare(&r, Ok(Some((7, 99)))),
            CheckVerdict::Updated { path: r.path.clone() }
        );
        assert_eq!(
            compare(&r, Ok(Some((8, 42)))),
            CheckVerdict::Updated { path: r.path.clone() }
        );
    }

    #[test]
    fn compare_reports_updated_when_recorded_path_not_found() {
        let r = recorded(7, 42);
        assert_eq!(
            compare(&r, Ok(None)),
            CheckVerdict::Updated { path: r.path.clone() }
        );
    }

    #[test]
    fn compare_reports_undecidable_on_any_other_stat_error() {
        let r = recorded(7, 42);
        assert_eq!(compare(&r, Err(())), CheckVerdict::Undecidable);
    }

    // ── AC-4: missing / truncated / malformed identity files all yield
    // Undecidable, end-to-end through `check` ─────────────────────────────

    #[test]
    fn check_reports_undecidable_when_identity_file_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        assert_eq!(check(&socket_path), CheckVerdict::Undecidable);
    }

    #[test]
    fn check_reports_undecidable_when_identity_file_is_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        // Magic marker present, but the fixed header is cut short.
        std::fs::write(identity_file_path(&socket_path), b"MUXID1\n\x01\x02\x03")
            .expect("write truncated identity file");
        assert_eq!(check(&socket_path), CheckVerdict::Undecidable);
    }

    #[test]
    fn check_reports_undecidable_when_identity_file_is_garbage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        // An otherwise well-formed, correctly-sized encoding (a real path
        // that would decode successfully) with ONLY the magic marker
        // corrupted -- isolates the wrong-magic rejection from the
        // truncation checks (a short garbage blob would trip those instead
        // without ever exercising the magic comparison).
        let mut bytes = encode(&recorded(1, 2));
        bytes[0] = b'X';
        std::fs::write(identity_file_path(&socket_path), bytes)
            .expect("write garbage identity file");
        assert_eq!(check(&socket_path), CheckVerdict::Undecidable);
    }

    #[test]
    fn check_reports_undecidable_when_identity_file_has_trailing_garbage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let identity = recorded(7, 42);
        let mut bytes = encode(&identity);
        bytes.extend_from_slice(b"trailing garbage");
        std::fs::write(identity_file_path(&socket_path), bytes)
            .expect("write identity file with trailing garbage");
        assert_eq!(
            check(&socket_path),
            CheckVerdict::Undecidable,
            "trailing garbage must never be parsed as an Updated verdict"
        );
    }

    // ── AC-1 (continued): the path field round-trips whitespace ──────────

    #[test]
    fn encode_decode_round_trips_a_path_containing_whitespace() {
        let identity = RecordedIdentity {
            path: PathBuf::from("/usr/local/bin/em term with spaces\tand a tab"),
            dev: 123,
            ino: 456,
        };
        let bytes = encode(&identity);
        let decoded = decode(&bytes).expect("must decode a whitespace-containing path");
        assert_eq!(decoded, identity);
    }

    // ── AC-5: a symlink pre-placed at the identity file path is refused ──

    #[test]
    fn write_identity_file_refuses_a_symlink_at_the_destination_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let identity_path = identity_file_path(&socket_path);

        let target = dir.path().join("attacker-target");
        std::fs::write(&target, b"untouched").expect("write attacker target");
        std::os::unix::fs::symlink(&target, &identity_path).expect("pre-place symlink");

        let identity = recorded(1, 2);
        let result = write_identity_file(&identity, &socket_path);

        assert!(result.is_err(), "a pre-placed symlink at the destination must be refused");
        assert_eq!(
            std::fs::read(&target).expect("read attacker target"),
            b"untouched",
            "the symlink's target must never be written through"
        );
    }

    // ── AC-5: a symlink pre-placed at the temp path is refused ────────────

    #[test]
    fn write_identity_file_refuses_a_symlink_at_the_temp_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let tmp_path = temp_identity_file_path(&socket_path);

        let target = dir.path().join("attacker-target");
        std::fs::write(&target, b"untouched").expect("write attacker target");
        std::os::unix::fs::symlink(&target, &tmp_path).expect("pre-place symlink at temp path");

        let identity = recorded(1, 2);
        let result = write_identity_file(&identity, &socket_path);

        assert!(result.is_err(), "a pre-placed symlink at the temp path must be refused");
        assert_eq!(
            std::fs::read(&target).expect("read attacker target"),
            b"untouched",
            "the symlink's target must never be written through"
        );
        assert!(
            !identity_file_path(&socket_path).exists(),
            "the final identity path must not have been created either"
        );
    }

    #[test]
    fn record_or_invalidate_is_best_effort_and_continues_when_persistence_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let identity_path = identity_file_path(&socket_path);
        let exe_stand_in = tempfile::NamedTempFile::new_in(dir.path()).expect("exe stand-in");

        let target = dir.path().join("attacker-target");
        std::fs::write(&target, b"untouched").expect("write attacker target");
        std::os::unix::fs::symlink(&target, &identity_path).expect("pre-place symlink");

        let result = record_or_invalidate(Some(exe_stand_in.path()), &socket_path);

        assert!(
            result.is_none(),
            "a persistence failure must degrade to no recorded identity, not abort"
        );
        assert_eq!(
            std::fs::read(&target).expect("read attacker target"),
            b"untouched",
            "the symlink's target must never be written through even via the \
             best-effort cleanup path"
        );
    }

    // ── AC-6: the upgrade-candidate resolution table ──────────────────────

    #[test]
    fn resolve_upgrade_candidate_returns_the_recorded_path_when_present() {
        let identity = recorded(7, 42);
        let candidate =
            resolve_upgrade_candidate(Some(&identity)).expect("a recorded identity must resolve");
        assert_eq!(candidate, identity.path);
    }

    #[test]
    fn resolve_upgrade_candidate_refuses_with_a_reason_when_absent() {
        let err = resolve_upgrade_candidate(None)
            .expect_err("no recorded identity must refuse rather than resolve");
        assert!(!err.is_empty(), "the refusal reason must be human-readable");
    }
}
