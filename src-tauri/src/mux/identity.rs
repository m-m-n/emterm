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

use std::ffi::{CStr, OsStr};
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
/// (folds into [`Verdict::Undecidable`] at the [`check`] call site).
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
pub enum Verdict {
    /// The recorded `(device, inode)` equals the current stat of the
    /// recorded path.
    Unchanged,
    /// The recorded path's `(device, inode)` no longer matches, or the
    /// path no longer exists. Carries the recorded clean executable path.
    Updated(PathBuf),
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
/// [`Verdict::Undecidable`]; this is structured for the unit test
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
/// [`Verdict::Undecidable`], never distinguishing the reason (Design:
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
fn compare(recorded: &RecordedIdentity, current: Result<Option<(u64, u64)>, ()>) -> Verdict {
    match current {
        Ok(Some(dev_ino)) if dev_ino == (recorded.dev, recorded.ino) => Verdict::Unchanged,
        Ok(Some(_)) => Verdict::Updated(recorded.path.clone()),
        Ok(None) => Verdict::Updated(recorded.path.clone()),
        Err(()) => Verdict::Undecidable,
    }
}

/// Client-side check (Design "Comparison predicate"; NFR1 cost bound: at
/// most one small-file read plus one stat of the recorded path, nothing
/// else).
pub fn check(socket_path: &Path) -> Verdict {
    let Ok(recorded) = read_identity_file(socket_path) else {
        return Verdict::Undecidable;
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

// ============================================================================
// task0004 (mux-daemon-binary-update-detect, NFR3): candidate-validation --
// the daemon never probes or execs an upgrade candidate whose current
// on-disk state is not owner-controlled. Two layers, per the task plan's
// Design "Candidate validation": a pure decision function over captured
// attributes ([`validate_candidate`], table-testable without privileged
// fixtures), and a thin capture wrapper ([`validate_candidate_path`]) that
// reads the candidate's and its parent directory's metadata WITHOUT
// following a symlink at the inspected path itself.
// ============================================================================

/// Kind of filesystem entry inspected at a path, WITHOUT following any
/// symlink at that path itself (Design "Candidate validation" step 2: "the
/// check must describe the entry at that path, not a symlink target").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    RegularFile,
    Directory,
    Symlink,
    /// Anything else stat can report (device node, fifo, socket, ...).
    Other,
}

fn kind_description(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::RegularFile => "a regular file",
        EntryKind::Directory => "a directory",
        EntryKind::Symlink => "a symlink",
        EntryKind::Other => "a special file",
    }
}

/// Captured attributes of one filesystem entry -- either the upgrade
/// candidate itself or its parent directory -- needed by the pure decision
/// function [`validate_candidate`]. Captured WITHOUT following a symlink at
/// the inspected path by [`capture_entry`] / [`validate_candidate_path`].
#[derive(Debug, Clone, Copy)]
pub struct EntryAttributes {
    pub kind: EntryKind,
    pub uid: u32,
    /// The entry's owning group id (task0001, mux-hot-upgrade-alt-screen,
    /// FR1) -- captured by the same non-following stat that already
    /// provides `uid` and `mode`; no extra syscall.
    pub gid: u32,
    pub mode: u32,
}

/// Which role an [`EntryAttributes`] plays in [`validate_candidate`]'s rule
/// table -- only the expected [`EntryKind`] and the label used in a refusal
/// reason differ between the candidate and its parent directory.
#[derive(Debug, Clone, Copy)]
enum EntryRole {
    Candidate,
    ParentDirectory,
}

impl EntryRole {
    fn expected_kind(self) -> EntryKind {
        match self {
            EntryRole::Candidate => EntryKind::RegularFile,
            EntryRole::ParentDirectory => EntryKind::Directory,
        }
    }

    fn label(self) -> &'static str {
        match self {
            EntryRole::Candidate => "upgrade candidate",
            EntryRole::ParentDirectory => "upgrade candidate's parent directory",
        }
    }
}

/// Validate one captured entry against NFR3's rule table -- ALL must hold,
/// in order: the entry must be the expected kind (regular file for the
/// candidate, directory for its parent -- a symlink at either is refused
/// under this same check, never followed); its owner must be the daemon's
/// own effective uid or root; the world-write permission bit must be unset
/// (FR2, unconditional -- refuses regardless of any group facts); and the
/// group-write permission bit must be unset UNLESS the entry's owning group
/// is the entry owner's own private per-user group (FR1, checked by
/// [`is_private_per_user_group`] against `facts`). Returns a human-readable
/// reason naming the FIRST failed rule.
fn validate_entry(
    attrs: EntryAttributes,
    facts: Option<&GroupOwnerFacts>,
    role: EntryRole,
    daemon_uid: u32,
) -> Result<(), String> {
    let expected = role.expected_kind();
    if attrs.kind != expected {
        return Err(format!(
            "{} must be {}, but is {}",
            role.label(),
            kind_description(expected),
            kind_description(attrs.kind),
        ));
    }
    if attrs.uid != daemon_uid && attrs.uid != 0 {
        return Err(format!(
            "{} is owned by uid {} (neither the daemon's effective uid {} nor root)",
            role.label(),
            attrs.uid,
            daemon_uid,
        ));
    }
    if attrs.mode & libc::S_IWOTH as u32 != 0 {
        return Err(format!(
            "{} permission bits {:o} allow world write",
            role.label(),
            attrs.mode & 0o777,
        ));
    }
    if attrs.mode & libc::S_IWGRP as u32 != 0 && !is_private_per_user_group(facts, attrs.gid) {
        return Err(format!(
            "{} permission bits {:o} allow group write, and its owning group is not the \
             owner's private per-user group",
            role.label(),
            attrs.mode & 0o777,
        ));
    }
    Ok(())
}

/// Pure decision over captured attributes of the candidate AND its parent
/// directory (Design "Candidate validation", NFR3): ALL rules must hold for
/// BOTH entries. Table-testable without privileged fixtures -- callers
/// parameterize `daemon_uid`, both [`EntryAttributes`], and both optional
/// [`GroupOwnerFacts`] directly (a foreign-uid, root-owner, or non-private
/// group row is simply a value, never a file or a real user/group actually
/// created on the system). `candidate_facts` / `parent_facts` are consulted
/// ONLY when the corresponding entry's group-write bit is set (FR1); pass
/// `None` when it is not needed or could not be captured (NFR2 fail-closed).
pub fn validate_candidate(
    candidate: EntryAttributes,
    candidate_facts: Option<&GroupOwnerFacts>,
    parent: EntryAttributes,
    parent_facts: Option<&GroupOwnerFacts>,
    daemon_uid: u32,
) -> Result<(), String> {
    validate_entry(candidate, candidate_facts, EntryRole::Candidate, daemon_uid)?;
    validate_entry(parent, parent_facts, EntryRole::ParentDirectory, daemon_uid)?;
    Ok(())
}

/// Capture one filesystem entry's attributes WITHOUT following a symlink at
/// `path` itself (NFR3): `symlink_metadata`, never `metadata`.
fn capture_entry(path: &Path) -> io::Result<EntryAttributes> {
    let meta = std::fs::symlink_metadata(path)?;
    let file_type = meta.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::RegularFile
    } else {
        EntryKind::Other
    };
    Ok(EntryAttributes {
        kind,
        uid: meta.uid(),
        gid: meta.gid(),
        mode: meta.mode(),
    })
}

/// Capture wrapper (Design "Candidate validation" step 2): inspects
/// `candidate` and its parent directory -- each via `symlink_metadata`, so
/// neither read ever follows a symlink at the inspected path -- then applies
/// [`validate_candidate`]. An I/O failure capturing either entry is itself a
/// refusal (a candidate that cannot even be stat-ed is certainly not
/// verified-safe to exec). [`GroupOwnerFacts`] are fetched per entry ONLY
/// when that entry's group-write bit is set (task0001, mux-hot-upgrade-alt-
/// screen, NFR1: at most one group lookup and one user lookup per entry
/// decision).
pub fn validate_candidate_path(candidate: &Path, daemon_uid: u32) -> Result<(), String> {
    let candidate_attrs = capture_entry(candidate)
        .map_err(|e| format!("failed to inspect upgrade candidate {candidate:?}: {e}"))?;
    let parent = candidate
        .parent()
        .ok_or_else(|| format!("upgrade candidate {candidate:?} has no parent directory"))?;
    let parent_attrs = capture_entry(parent).map_err(|e| {
        format!("failed to inspect upgrade candidate's parent directory {parent:?}: {e}")
    })?;
    let candidate_facts = group_write_facts_if_needed(candidate_attrs);
    let parent_facts = group_write_facts_if_needed(parent_attrs);
    validate_candidate(
        candidate_attrs,
        candidate_facts.as_ref(),
        parent_attrs,
        parent_facts.as_ref(),
        daemon_uid,
    )
}

/// Fetch [`GroupOwnerFacts`] for `attrs` only when its group-write bit is
/// set -- the world-write rule never consults facts (FR2 is unconditional)
/// and an entry with neither write bit set needs none either, so this keeps
/// [`validate_candidate_path`] at NFR1's "at most one lookup" bound.
fn group_write_facts_if_needed(attrs: EntryAttributes) -> Option<GroupOwnerFacts> {
    if attrs.mode & libc::S_IWGRP as u32 != 0 {
        capture_group_owner_facts(attrs.gid, attrs.uid)
    } else {
        None
    }
}

/// The daemon's own effective uid, used by [`validate_candidate_path`]'s
/// NFR3 ownership rule and by production call sites that need a real
/// `daemon_uid` to pass in.
pub fn effective_uid() -> u32 {
    // SAFETY: `geteuid(2)` takes no arguments, touches no memory this
    // process controls, and cannot fail.
    unsafe { libc::geteuid() }
}

// ============================================================================
// task0001 (mux-hot-upgrade-alt-screen, FR1/FR2/NFR1/NFR2): private
// per-user-group exemption in `validate_entry`'s group-write rule -- a
// umask 002 dev build's 0o775 binary/parent must still pass NFR3 when, and
// only when, the entry's owning group is the entry owner's own private
// per-user group. Two layers, matching the file's existing pattern above: a
// pure predicate ([`is_private_per_user_group`], table-testable over
// synthesized [`GroupOwnerFacts`]) and a thin capture wrapper
// ([`capture_group_owner_facts`]) that performs exactly one reentrant
// group-by-id lookup and one reentrant user-by-id lookup -- never
// `getpwent`/`getgrent` (no passwd- or group-database enumeration).
// ============================================================================

/// Identity facts about an entry's owning group and its owner, consulted
/// only by the group-write branch of [`validate_entry`] (FR1). Captured by
/// [`capture_group_owner_facts`]; absent (`None` at the call site) whenever
/// any needed fact could not be obtained -- [`is_private_per_user_group`]
/// then rejects (NFR2 fail-closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOwnerFacts {
    /// The entry's owning group's name (`getgrgid`'s `gr_name`).
    pub group_name: String,
    /// The entry's owning group's member list (`getgrgid`'s `gr_mem`) --
    /// per `/etc/group` semantics this does NOT include primary-group
    /// membership, only explicitly-listed supplementary members.
    pub group_members: Vec<String>,
    /// The entry owner's user name (`getpwuid`'s `pw_name`).
    pub owner_user_name: String,
    /// The entry owner's primary group id (`getpwuid`'s `pw_gid`).
    pub owner_primary_gid: u32,
}

/// Private per-user-group predicate (Design "Private per-user-group
/// predicate", FR1): accepts the group-write bit only when `facts` is
/// present AND all three conditions hold against `entry_gid` (the entry's
/// own owning group id -- the same id `facts.group_name`/`group_members`
/// were looked up by):
///
/// (a) the group's member list contains no name other than the owner's user
///     name (an empty member list satisfies this);
/// (b) the owner's primary group id equals `entry_gid`;
/// (c) the group's name equals the owner's user name.
///
/// `facts` is `None` (a fact could not be obtained) -> rejects (NFR2
/// fail-closed).
fn is_private_per_user_group(facts: Option<&GroupOwnerFacts>, entry_gid: u32) -> bool {
    let Some(facts) = facts else {
        return false;
    };
    let no_extra_members = facts
        .group_members
        .iter()
        .all(|member| *member == facts.owner_user_name);
    let primary_group_matches = facts.owner_primary_gid == entry_gid;
    let name_matches = facts.group_name == facts.owner_user_name;
    no_extra_members && primary_group_matches && name_matches
}

/// Grow-and-retry buffer cap for the reentrant lookups below (NSS backends,
/// e.g. LDAP/SSSD, can return very large records). A lookup that still
/// overflows this cap is treated the same as any other lookup failure
/// (`None`, NFR2 fail-closed) -- never panics and never loops unbounded.
const IDENTITY_LOOKUP_MAX_BUF_LEN: usize = 1 << 20; // 1 MiB

/// Look up a group's name and member list by gid via the REENTRANT
/// `getgrgid_r` (never the non-reentrant `getgrgid`, which returns a
/// pointer into a static buffer that is unsafe to share across the daemon's
/// threads). `None` on "no such group" or any lookup failure, including a
/// record too large for [`IDENTITY_LOOKUP_MAX_BUF_LEN`].
fn lookup_group_by_gid(gid: u32) -> Option<(String, Vec<String>)> {
    let mut buf_len: usize = 1024;
    loop {
        let mut buf: Vec<libc::c_char> = vec![0; buf_len];
        // SAFETY: `zeroed()` is a valid bit pattern for `libc::group` (a
        // plain-old-data struct of pointers and integers); every field is
        // fully overwritten by `getgrgid_r` on success before being read.
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        // SAFETY: `buf` is valid for `buf_len` bytes and outlives this
        // call; `grp` and `result` are valid out-params for its duration.
        let ret = unsafe {
            libc::getgrgid_r(
                gid as libc::gid_t,
                &mut grp,
                buf.as_mut_ptr(),
                buf_len,
                &mut result,
            )
        };
        if ret == 0 {
            if result.is_null() {
                return None;
            }
            // SAFETY: a successful call with a non-null `result` guarantees
            // `grp.gr_name` and every pointer up to the first NULL entry of
            // `grp.gr_mem` are NUL-terminated strings living inside `buf`,
            // valid until `buf` is dropped at the end of this function.
            let name = unsafe { CStr::from_ptr(grp.gr_name) }
                .to_str()
                .ok()?
                .to_string();
            let mut members = Vec::new();
            let mut cursor = grp.gr_mem;
            unsafe {
                while !(*cursor).is_null() {
                    members.push(CStr::from_ptr(*cursor).to_str().ok()?.to_string());
                    cursor = cursor.add(1);
                }
            }
            return Some((name, members));
        }
        if ret == libc::ERANGE && buf_len < IDENTITY_LOOKUP_MAX_BUF_LEN {
            buf_len *= 2;
            continue;
        }
        return None;
    }
}

/// Look up a user's name and primary group id by uid via the REENTRANT
/// `getpwuid_r` (never the non-reentrant `getpwuid`, for the same reason as
/// [`lookup_group_by_gid`]). `None` on "no such user" or any lookup
/// failure, including a record too large for [`IDENTITY_LOOKUP_MAX_BUF_LEN`].
fn lookup_user_by_uid(uid: u32) -> Option<(String, u32)> {
    let mut buf_len: usize = 1024;
    loop {
        let mut buf: Vec<libc::c_char> = vec![0; buf_len];
        // SAFETY: same contract as `lookup_group_by_gid`'s `zeroed()` call,
        // for `libc::passwd`.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: same contract as `lookup_group_by_gid`'s `getgrgid_r` call.
        let ret = unsafe {
            libc::getpwuid_r(
                uid as libc::uid_t,
                &mut pwd,
                buf.as_mut_ptr(),
                buf_len,
                &mut result,
            )
        };
        if ret == 0 {
            if result.is_null() {
                return None;
            }
            // SAFETY: a successful call with a non-null `result` guarantees
            // `pwd.pw_name` is a NUL-terminated string living inside `buf`,
            // valid until `buf` is dropped at the end of this function.
            let name = unsafe { CStr::from_ptr(pwd.pw_name) }
                .to_str()
                .ok()?
                .to_string();
            return Some((name, pwd.pw_gid as u32));
        }
        if ret == libc::ERANGE && buf_len < IDENTITY_LOOKUP_MAX_BUF_LEN {
            buf_len *= 2;
            continue;
        }
        return None;
    }
}

/// Capture wrapper (Design "Fact-capture path", NFR1): exactly one
/// group-by-id lookup and one user-by-id lookup, never a passwd- or
/// group-database enumeration (no `getpwent`/`getgrent`). `None` if either
/// lookup fails -- the group-write branch of [`validate_entry`] then treats
/// the group-write bit as refused (NFR2 fail-closed).
fn capture_group_owner_facts(entry_gid: u32, entry_uid: u32) -> Option<GroupOwnerFacts> {
    let (group_name, group_members) = lookup_group_by_gid(entry_gid)?;
    let (owner_user_name, owner_primary_gid) = lookup_user_by_uid(entry_uid)?;
    Some(GroupOwnerFacts {
        group_name,
        group_members,
        owner_user_name,
        owner_primary_gid,
    })
}

/// Capture `path`'s current `(device, inode)` WITHOUT following a symlink at
/// `path` itself, for the repeat-refusal suppression key (NFR1, Design
/// "Repeat-refusal suppression"). `None` on any stat failure -- the caller
/// treats that the same as "the candidate's identity differs from the
/// recorded refusal" (Design: "If it differs, or the capture fails -> clear
/// the state").
pub fn capture_dev_ino(path: &Path) -> Option<(u64, u64)> {
    std::fs::symlink_metadata(path)
        .ok()
        .map(|m| (m.dev(), m.ino()))
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
            Verdict::Unchanged,
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
        assert_eq!(compare(&r, Ok(Some((7, 42)))), Verdict::Unchanged);
    }

    #[test]
    fn compare_reports_updated_when_dev_ino_differ() {
        let r = recorded(7, 42);
        assert_eq!(
            compare(&r, Ok(Some((7, 99)))),
            Verdict::Updated(r.path.clone())
        );
        assert_eq!(
            compare(&r, Ok(Some((8, 42)))),
            Verdict::Updated(r.path.clone())
        );
    }

    #[test]
    fn compare_reports_updated_when_recorded_path_not_found() {
        let r = recorded(7, 42);
        assert_eq!(
            compare(&r, Ok(None)),
            Verdict::Updated(r.path.clone())
        );
    }

    #[test]
    fn compare_reports_undecidable_on_any_other_stat_error() {
        let r = recorded(7, 42);
        assert_eq!(compare(&r, Err(())), Verdict::Undecidable);
    }

    // ── AC-4: missing / truncated / malformed identity files all yield
    // Undecidable, end-to-end through `check` ─────────────────────────────

    #[test]
    fn check_reports_undecidable_when_identity_file_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        assert_eq!(check(&socket_path), Verdict::Undecidable);
    }

    #[test]
    fn check_reports_undecidable_when_identity_file_is_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        // Magic marker present, but the fixed header is cut short.
        std::fs::write(identity_file_path(&socket_path), b"MUXID1\n\x01\x02\x03")
            .expect("write truncated identity file");
        assert_eq!(check(&socket_path), Verdict::Undecidable);
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
        assert_eq!(check(&socket_path), Verdict::Undecidable);
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
            Verdict::Undecidable,
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

    // ── task0004 AC-1/AC-3: `validate_candidate`'s full decision table
    // (pure -- no privileged fixtures; foreign-uid and root-owner rows are
    // parameterized attribute values, never created on disk) ─────────────

    const DAEMON_UID: u32 = 1000;
    const ROOT_UID: u32 = 0;
    const FOREIGN_UID: u32 = 2000;

    fn attrs(kind: EntryKind, uid: u32, mode: u32) -> EntryAttributes {
        // `gid` defaults to 0 -- irrelevant to every row below since none of
        // these rows exercise the group-write FR1 exemption (see the
        // task0001 (mux-hot-upgrade-alt-screen) block further down for the
        // gid-sensitive rows).
        EntryAttributes { kind, uid, gid: 0, mode }
    }

    fn conforming_candidate() -> EntryAttributes {
        attrs(EntryKind::RegularFile, DAEMON_UID, 0o644)
    }

    fn conforming_parent() -> EntryAttributes {
        attrs(EntryKind::Directory, DAEMON_UID, 0o755)
    }

    #[test]
    fn validate_candidate_accepts_owner_owned_regular_file_in_conforming_parent() {
        assert!(
            validate_candidate(
                conforming_candidate(),
                None,
                conforming_parent(),
                None,
                DAEMON_UID,
            )
            .is_ok()
        );
    }

    #[test]
    fn validate_candidate_accepts_root_owned_candidate_and_parent() {
        let candidate = attrs(EntryKind::RegularFile, ROOT_UID, 0o644);
        let parent = attrs(EntryKind::Directory, ROOT_UID, 0o755);
        assert!(
            validate_candidate(candidate, None, parent, None, DAEMON_UID).is_ok(),
            "a root-owned candidate and parent must be accepted even though the daemon's own \
             uid differs"
        );
    }

    #[test]
    fn validate_candidate_refuses_symlink_candidate() {
        let candidate = attrs(EntryKind::Symlink, DAEMON_UID, 0o777);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("a symlink candidate must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("symlink"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_non_regular_candidate() {
        let candidate = attrs(EntryKind::Other, DAEMON_UID, 0o644);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("a special-file candidate must be refused");
        assert!(err.contains("special file"), "{err}");
    }

    #[test]
    fn validate_candidate_refuses_directory_candidate() {
        let candidate = attrs(EntryKind::Directory, DAEMON_UID, 0o755);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("a directory candidate must be refused");
        assert!(err.contains("a regular file"), "{err}");
    }

    #[test]
    fn validate_candidate_refuses_group_writable_candidate_without_facts() {
        let candidate = attrs(EntryKind::RegularFile, DAEMON_UID, 0o664);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("a group-writable candidate without exemption facts must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("group write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_world_writable_candidate() {
        let candidate = attrs(EntryKind::RegularFile, DAEMON_UID, 0o646);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("a world-writable candidate must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("world write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_foreign_owner_candidate() {
        let candidate = attrs(EntryKind::RegularFile, FOREIGN_UID, 0o644);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("a candidate owned by neither the daemon's uid nor root must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("uid"),
            "{err}"
        );
    }

    // ── the same rows, applied to the PARENT directory ────────────────────

    #[test]
    fn validate_candidate_refuses_symlink_parent() {
        let parent = attrs(EntryKind::Symlink, DAEMON_UID, 0o777);
        let err = validate_candidate(conforming_candidate(), None, parent, None, DAEMON_UID)
            .expect_err("a symlink parent directory must be refused");
        assert!(
            err.contains("parent directory") && err.contains("symlink"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_non_directory_parent() {
        let parent = attrs(EntryKind::RegularFile, DAEMON_UID, 0o644);
        let err = validate_candidate(conforming_candidate(), None, parent, None, DAEMON_UID)
            .expect_err("a non-directory parent must be refused");
        assert!(
            err.contains("parent directory") && err.contains("a directory"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_group_writable_parent_without_facts() {
        let parent = attrs(EntryKind::Directory, DAEMON_UID, 0o775);
        let err = validate_candidate(conforming_candidate(), None, parent, None, DAEMON_UID)
            .expect_err("a group-writable parent without exemption facts must be refused");
        assert!(
            err.contains("parent directory") && err.contains("group write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_world_writable_parent() {
        let parent = attrs(EntryKind::Directory, DAEMON_UID, 0o757);
        let err = validate_candidate(conforming_candidate(), None, parent, None, DAEMON_UID)
            .expect_err("a world-writable parent must be refused");
        assert!(
            err.contains("parent directory") && err.contains("world write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_foreign_owner_parent() {
        let parent = attrs(EntryKind::Directory, FOREIGN_UID, 0o755);
        let err = validate_candidate(conforming_candidate(), None, parent, None, DAEMON_UID)
            .expect_err("a parent owned by neither the daemon's uid nor root must be refused");
        assert!(
            err.contains("parent directory") && err.contains("uid"),
            "{err}"
        );
    }

    // ── the capture wrapper (`validate_candidate_path`), against real files:
    // proves the symlink-refusal detail actually applies to a REAL symlink,
    // not just the pure table above ────────────────────────────────────────

    #[test]
    fn validate_candidate_path_accepts_a_conforming_real_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A permissive ambient umask (e.g. `002`) would otherwise leave a
        // freshly created tempdir group-writable, making this "conforming
        // parent" assumption environment-dependent -- harden explicitly
        // rather than relying on umask.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("harden tempdir to a conforming mode");
        let candidate = tempfile::NamedTempFile::new_in(dir.path()).expect("candidate file");
        assert!(
            validate_candidate_path(candidate.path(), effective_uid()).is_ok(),
            "a plain owner-only temp file in a conforming parent must be accepted"
        );
    }

    #[test]
    fn validate_candidate_path_refuses_a_symlink_at_the_candidate_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("real-binary");
        std::fs::write(&target, b"binary").expect("write real target");
        let link = dir.path().join("candidate-link");
        std::os::unix::fs::symlink(&target, &link).expect("pre-place symlink");

        let err = validate_candidate_path(&link, effective_uid())
            .expect_err("a symlink AT the candidate path itself must be refused");
        assert!(err.contains("symlink"), "{err}");
    }

    #[test]
    fn validate_candidate_path_refuses_a_world_writable_real_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let candidate = tempfile::NamedTempFile::new_in(dir.path()).expect("candidate file");
        std::fs::set_permissions(candidate.path(), std::fs::Permissions::from_mode(0o646))
            .expect("loosen candidate permissions");

        let err = validate_candidate_path(candidate.path(), effective_uid())
            .expect_err("a world-writable real file must be refused");
        assert!(err.contains("world write"), "{err}");
    }

    #[test]
    fn validate_candidate_path_reports_a_failure_to_inspect_a_missing_candidate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        let err = validate_candidate_path(&missing, effective_uid())
            .expect_err("a candidate that cannot be stat-ed must be refused, not panic");
        assert!(!err.is_empty());
    }

    // ── `capture_dev_ino` (NFR1 suppression key) ───────────────────────────

    #[test]
    fn capture_dev_ino_returns_none_for_a_missing_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(capture_dev_ino(&dir.path().join("does-not-exist")), None);
    }

    #[test]
    fn capture_dev_ino_returns_some_for_an_existing_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = tempfile::NamedTempFile::new_in(dir.path()).expect("file");
        assert!(capture_dev_ino(file.path()).is_some());
    }

    // ── task0001 (mux-hot-upgrade-alt-screen) AC-4: `is_private_per_user_group`'s
    // full FR1/NFR2 decision table (pure predicate, no privileged fixtures) ───

    fn owner_facts(
        group_name: &str,
        group_members: &[&str],
        owner_user_name: &str,
        owner_primary_gid: u32,
    ) -> GroupOwnerFacts {
        GroupOwnerFacts {
            group_name: group_name.to_string(),
            group_members: group_members.iter().map(|m| m.to_string()).collect(),
            owner_user_name: owner_user_name.to_string(),
            owner_primary_gid,
        }
    }

    const PRIVATE_GROUP_GID: u32 = 1000;

    #[test]
    fn is_private_per_user_group_accepts_when_all_three_conditions_hold() {
        let facts = owner_facts("daemonuser", &[], "daemonuser", PRIVATE_GROUP_GID);
        assert!(is_private_per_user_group(Some(&facts), PRIVATE_GROUP_GID));
    }

    #[test]
    fn is_private_per_user_group_accepts_a_member_list_naming_only_the_owner() {
        let facts = owner_facts("daemonuser", &["daemonuser"], "daemonuser", PRIVATE_GROUP_GID);
        assert!(is_private_per_user_group(Some(&facts), PRIVATE_GROUP_GID));
    }

    #[test]
    fn is_private_per_user_group_rejects_an_extra_member_name() {
        let facts = owner_facts(
            "daemonuser",
            &["daemonuser", "someoneelse"],
            "daemonuser",
            PRIVATE_GROUP_GID,
        );
        assert!(!is_private_per_user_group(Some(&facts), PRIVATE_GROUP_GID));
    }

    #[test]
    fn is_private_per_user_group_rejects_when_primary_gid_differs() {
        // e.g. gid 100 "users" as the owner's primary group.
        let facts = owner_facts("daemonuser", &[], "daemonuser", 100);
        assert!(!is_private_per_user_group(Some(&facts), PRIVATE_GROUP_GID));
    }

    #[test]
    fn is_private_per_user_group_rejects_when_group_name_differs_from_owner_name() {
        let facts = owner_facts("staff", &[], "daemonuser", PRIVATE_GROUP_GID);
        assert!(!is_private_per_user_group(Some(&facts), PRIVATE_GROUP_GID));
    }

    #[test]
    fn is_private_per_user_group_rejects_when_facts_are_unavailable() {
        assert!(!is_private_per_user_group(None, PRIVATE_GROUP_GID));
    }

    // ── AC-1: a 0o775 entry whose owning group satisfies FR1 is accepted,
    // in both the candidate and parent-directory roles ─────────────────────

    #[test]
    fn validate_candidate_accepts_group_writable_candidate_in_a_private_per_user_group() {
        let candidate = EntryAttributes {
            kind: EntryKind::RegularFile,
            uid: DAEMON_UID,
            gid: PRIVATE_GROUP_GID,
            mode: 0o775,
        };
        let candidate_facts = owner_facts("daemonuser", &[], "daemonuser", PRIVATE_GROUP_GID);
        assert!(
            validate_candidate(
                candidate,
                Some(&candidate_facts),
                conforming_parent(),
                None,
                DAEMON_UID,
            )
            .is_ok(),
            "a 0o775 candidate in the owner's private per-user group must be accepted"
        );
    }

    #[test]
    fn validate_candidate_accepts_group_writable_parent_in_a_private_per_user_group() {
        let parent = EntryAttributes {
            kind: EntryKind::Directory,
            uid: DAEMON_UID,
            gid: PRIVATE_GROUP_GID,
            mode: 0o775,
        };
        let parent_facts = owner_facts("daemonuser", &[], "daemonuser", PRIVATE_GROUP_GID);
        assert!(
            validate_candidate(
                conforming_candidate(),
                None,
                parent,
                Some(&parent_facts),
                DAEMON_UID,
            )
            .is_ok(),
            "a 0o775 parent directory in the owner's private per-user group must be accepted"
        );
    }

    // ── AC-2: each refusal scenario pinned by its own test ─────────────────

    fn group_writable_candidate(gid: u32) -> EntryAttributes {
        EntryAttributes {
            kind: EntryKind::RegularFile,
            uid: DAEMON_UID,
            gid,
            mode: 0o775,
        }
    }

    #[test]
    fn validate_candidate_refuses_group_writable_candidate_with_an_extra_group_member() {
        let candidate = group_writable_candidate(PRIVATE_GROUP_GID);
        let facts = owner_facts(
            "daemonuser",
            &["daemonuser", "someoneelse"],
            "daemonuser",
            PRIVATE_GROUP_GID,
        );
        let err =
            validate_candidate(candidate, Some(&facts), conforming_parent(), None, DAEMON_UID)
                .expect_err("an extra member in the owning group must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("group write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_group_writable_candidate_with_mismatched_primary_gid() {
        let candidate = group_writable_candidate(PRIVATE_GROUP_GID);
        // The owner's primary group is gid 100 "users", not this entry's
        // owning group -- condition (b) fails even though (a) and (c) hold.
        let facts = owner_facts("daemonuser", &[], "daemonuser", 100);
        let err =
            validate_candidate(candidate, Some(&facts), conforming_parent(), None, DAEMON_UID)
                .expect_err("a primary-gid mismatch must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("group write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_group_writable_candidate_with_mismatched_group_name() {
        let candidate = group_writable_candidate(PRIVATE_GROUP_GID);
        let facts = owner_facts("staff", &[], "daemonuser", PRIVATE_GROUP_GID);
        let err =
            validate_candidate(candidate, Some(&facts), conforming_parent(), None, DAEMON_UID)
                .expect_err("a group-name mismatch must be refused");
        assert!(
            err.contains("upgrade candidate") && err.contains("group write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_group_writable_candidate_when_facts_are_unavailable() {
        let candidate = group_writable_candidate(PRIVATE_GROUP_GID);
        let err = validate_candidate(candidate, None, conforming_parent(), None, DAEMON_UID)
            .expect_err("unavailable identity facts must fail closed");
        assert!(
            err.contains("upgrade candidate") && err.contains("group write"),
            "{err}"
        );
    }

    #[test]
    fn validate_candidate_refuses_world_writable_candidate_even_with_conforming_group_facts() {
        // Both write bits set; group facts alone would satisfy FR1, but the
        // world-write bit refuses unconditionally (FR2) and must be the
        // reported reason.
        let mut candidate = group_writable_candidate(PRIVATE_GROUP_GID);
        candidate.mode = 0o776;
        let facts = owner_facts("daemonuser", &[], "daemonuser", PRIVATE_GROUP_GID);
        let err =
            validate_candidate(candidate, Some(&facts), conforming_parent(), None, DAEMON_UID)
                .expect_err("world-write must refuse regardless of group facts");
        assert!(
            err.contains("upgrade candidate") && err.contains("world write"),
            "{err}"
        );
    }

    // ── the capture wrapper: real reentrant lookups against the current
    // process's own identity (no privileged fixtures needed -- self-lookup
    // always resolves) ──────────────────────────────────────────────────────

    #[test]
    fn capture_group_owner_facts_resolves_the_current_process_identity() {
        let uid = effective_uid();
        // SAFETY: `getegid(2)` takes no arguments and cannot fail.
        let gid = unsafe { libc::getegid() };
        let facts = capture_group_owner_facts(gid, uid);
        assert!(
            facts.is_some(),
            "looking up the current process's own uid/gid must succeed"
        );
    }
}
