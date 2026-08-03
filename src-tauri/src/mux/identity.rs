//! `mux::identity` — binary-update identity check (client side).
//!
//! **task0002 (mux-daemon-binary-update-detect) stand-in.** This file
//! exists ONLY so this task's isolated worktree compiles and its own unit
//! tests can inject verdicts (IMPLEMENTATION.md D2). task0001 owns the real
//! implementation end to end: recording the running daemon's start-binary
//! identity to an on-disk file at daemon startup, reading and comparing it
//! back, file-format hardening (owner-only permissions, symlink refusal),
//! and that module's own unit tests. At integration this file is expected
//! to be replaced wholesale by task0001's real module, not merged
//! line-by-line — do not add real recording/reading logic here.
//!
//! Contract this stand-in must match exactly (IMPLEMENTATION.md Shared
//! Components, "check recorded identity (client side)"): a cheap (at most
//! one small-file read plus one stat, NFR1) check of the identity recorded
//! for a daemon socket, returning a three-valued [`Verdict`]. This stand-in
//! always reports [`Verdict::Undecidable`], which is contract-conformant
//! (a daemon predating identity recording, or any unreadable/missing
//! identity file, both fall to `Undecidable`) and safe: FR7 requires an
//! undecidable comparison to never fire the upgrade trigger.
//!
//! Unix only (`mux/mod.rs`), matching the `upgrade` / `inherited_pty`
//! precedent — the real module's rename-replacement / stat semantics have
//! no Windows equivalent.

use std::path::{Path, PathBuf};

/// Verdict of comparing the daemon's recorded start-binary identity against
/// its current on-disk state (Shared Components contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::mux) enum Verdict {
    /// The recorded (device, inode) still matches the current stat of the
    /// recorded path — the daemon's binary was not replaced.
    Unchanged,
    /// The recorded path's (device, inode) no longer matches, or the
    /// recorded path no longer exists. Carries the recorded clean
    /// executable path — the upgrade exec target (FR4, NFR3).
    Updated(PathBuf),
    /// The identity file is missing, unreadable, malformed, or truncated,
    /// or its stat failed with an error other than not-found. Never
    /// produced from a parse failure or a non-not-found stat error turning
    /// into a false `Updated` (contract guarantee) — a daemon that never
    /// recorded an identity (this stand-in, or a pre-feature daemon) always
    /// lands here.
    Undecidable,
}

/// Decide whether the daemon owning `sock_path` was replaced, by checking
/// its recorded identity file (Shared Components contract: at most one
/// small-file read plus one stat).
///
/// This stand-in always returns [`Verdict::Undecidable`] — see module docs.
/// task0002's own unit tests do not call this production function; they
/// inject their own verdict-producing closures directly into the
/// parameterized recovery-probe variant.
pub(in crate::mux) fn check(_sock_path: &Path) -> Verdict {
    Verdict::Undecidable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_always_reports_undecidable() {
        assert_eq!(
            check(Path::new("/nonexistent/mux-identity-stand-in.sock")),
            Verdict::Undecidable
        );
    }
}
