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
    assert!(
        identity_path.exists(),
        "identity file must exist next to the socket"
    );
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

    assert!(
        result.is_none(),
        "a non-stat-able capture must record nothing"
    );
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
    assert_eq!(compare(&r, Ok(None)), Verdict::Updated(r.path.clone()));
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
    std::fs::write(identity_file_path(&socket_path), bytes).expect("write garbage identity file");
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

    assert!(
        result.is_err(),
        "a pre-placed symlink at the destination must be refused"
    );
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

    assert!(
        result.is_err(),
        "a pre-placed symlink at the temp path must be refused"
    );
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
    EntryAttributes {
        kind,
        uid,
        gid: 0,
        mode,
    }
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
    let facts = owner_facts(
        "daemonuser",
        &["daemonuser"],
        "daemonuser",
        PRIVATE_GROUP_GID,
    );
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
    let err = validate_candidate(
        candidate,
        Some(&facts),
        conforming_parent(),
        None,
        DAEMON_UID,
    )
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
    let err = validate_candidate(
        candidate,
        Some(&facts),
        conforming_parent(),
        None,
        DAEMON_UID,
    )
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
    let err = validate_candidate(
        candidate,
        Some(&facts),
        conforming_parent(),
        None,
        DAEMON_UID,
    )
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
    let err = validate_candidate(
        candidate,
        Some(&facts),
        conforming_parent(),
        None,
        DAEMON_UID,
    )
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
