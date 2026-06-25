//! User-directory font override.
//!
//! Resolves the platform-specific directory where end-users can drop
//! `.ttf` / `.otf` font files to be picked up by the resolver ahead of
//! both the system scan and the bundled fonts (FR6).
//!
//! Resolution order (Linux):
//! 1. `$XDG_DATA_HOME/net.laser5.app.emterm/fonts/`
//! 2. `$HOME/.local/share/net.laser5.app.emterm/fonts/`
//!
//! Windows:
//! 1. `%APPDATA%\net.laser5.app.emterm\fonts\`
//!
//! The directory is *optional*: when it does not exist (or no platform
//! var is reachable) [`user_font_dir`] returns `None` and the scan is
//! skipped silently. Non-`.ttf` / `.otf` entries are ignored. Unreadable
//! or invalid font files emit a single `warn` log line and are skipped.

use std::path::PathBuf;
use std::sync::Arc;

use super::resolver::{FontRole, Resolver};

const APP_ID: &str = "net.laser5.app.emterm";
const FONTS_SUBDIR: &str = "fonts";

/// Upper bound on a user-dir font file (64 MiB). Files larger than this
/// are skipped (with a `font.user_dir.invalid_file: oversized` warn);
/// no realistic single-face font ships anywhere near this size, and
/// reading an attacker-supplied multi-gigabyte file into memory at
/// startup would block the resolver scan for an unbounded amount of
/// time.
const MAX_USER_FONT_BYTES: u64 = 64 * 1024 * 1024;

/// Resolve the user font directory on the current platform.
///
/// Returns `None` on unsupported targets or when no platform variable
/// is reachable (e.g. neither `$XDG_DATA_HOME` nor `$HOME` is set).
pub fn user_font_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(base.join(APP_ID).join(FONTS_SUBDIR))
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
        Some(base.join(APP_ID).join(FONTS_SUBDIR))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Discover and register every eligible font file in `dir`. Internal
/// helper exposed so [`Resolver::scan_user_dir`] can dispatch to it.
///
/// Returns the number of fonts that were successfully registered.
pub fn scan_dir_into(resolver: &mut Resolver, dir: &std::path::Path) -> usize {
    let mut count = 0usize;
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            // ENOENT is the common "user does not use overrides" case
            // and must not pollute the log; only surface other errors.
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("font.user_dir.read_failed: dir={} err={}", dir.display(), e);
            }
            return 0;
        }
    };

    for entry in read {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!(
                    "font.user_dir.read_failed: dir={} entry_err={}",
                    dir.display(),
                    e
                );
                continue;
            }
        };
        let path = entry.path();
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(s) => s.to_ascii_lowercase(),
            None => continue,
        };
        if !matches!(ext.as_str(), "ttf" | "otf") {
            continue;
        }
        // Skip symlinks without traversing them. `file_type()` does NOT
        // follow the link (unlike `metadata()`), so a symlink that
        // points at e.g. `/dev/zero` cannot trick the loader into an
        // unbounded read or out-of-tree dependency.
        match entry.file_type() {
            Ok(ft) if ft.is_symlink() => {
                log::warn!(
                    "font.user_dir.invalid_file: path={} (symlink)",
                    path.display()
                );
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!(
                    "font.user_dir.invalid_file: path={} file_type_err={}",
                    path.display(),
                    e
                );
                continue;
            }
        }
        // Cap the on-disk size before we read the bytes into memory.
        // 64 MiB is generous for a single-face font and protects the
        // startup scan from an oversized / hostile file.
        match entry.metadata() {
            Ok(md) if md.len() > MAX_USER_FONT_BYTES => {
                log::warn!(
                    "font.user_dir.invalid_file: path={} (oversized: {} bytes > {} cap)",
                    path.display(),
                    md.len(),
                    MAX_USER_FONT_BYTES
                );
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!(
                    "font.user_dir.invalid_file: path={} metadata_err={}",
                    path.display(),
                    e
                );
                continue;
            }
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!(
                    "font.user_dir.read_failed: path={} err={}",
                    path.display(),
                    e
                );
                continue;
            }
        };
        if bytes.is_empty() {
            log::warn!(
                "font.user_dir.invalid_file: path={} (empty)",
                path.display()
            );
            continue;
        }

        // Family-name detection: parse the file via fontdb so the
        // resolver `by_family` lookup wins over bundled / system
        // entries for the same family.
        let mut tmp_db = fontdb::Database::new();
        let arc_bytes: Arc<[u8]> = Arc::<[u8]>::from(bytes.as_slice());
        // load_font_source consumes the bytes; clone via Source::Binary.
        tmp_db.load_font_source(fontdb::Source::Binary(Arc::new(arc_bytes.clone())));
        let family = tmp_db
            .faces()
            .next()
            .and_then(|face| {
                face.families
                    .iter()
                    .find(|(_, lang)| lang.primary_language() == "English")
                    .or_else(|| face.families.first())
                    .map(|(n, _)| n.clone())
            })
            .unwrap_or_else(|| {
                // Fall back to the file stem so the registration still
                // succeeds even when fontdb cannot parse the file.
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

        if tmp_db.faces().next().is_none() {
            log::warn!(
                "font.user_dir.invalid_file: path={} (no parseable face)",
                path.display()
            );
            continue;
        }

        resolver.register_bytes(FontRole::User, family, arc_bytes);
        count += 1;
    }
    count
}

impl Resolver {
    /// Scan the platform user font directory (if any) and register
    /// every `.ttf` / `.otf` file as [`FontRole::User`]. Idempotent
    /// only when called once at resolver-build time — re-calling will
    /// double-register entries.
    pub fn scan_user_dir(&mut self) {
        let Some(dir) = user_font_dir() else {
            return;
        };
        let _ = scan_dir_into(self, &dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[cfg(target_os = "linux")]
    fn user_font_dir_prefers_xdg_data_home() {
        // SAFETY: env mutation is process-wide; we restore in a guard.
        let prev_xdg = std::env::var_os("XDG_DATA_HOME");
        let prev_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", "/tmp/xdg-test");
            std::env::set_var("HOME", "/tmp/home-test");
        }
        let dir = user_font_dir().expect("Linux resolves");
        assert_eq!(
            dir,
            PathBuf::from("/tmp/xdg-test/net.laser5.app.emterm/fonts")
        );
        unsafe {
            match prev_xdg {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn user_font_dir_falls_back_to_home_local_share() {
        let prev_xdg = std::env::var_os("XDG_DATA_HOME");
        let prev_home = std::env::var_os("HOME");
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
            std::env::set_var("HOME", "/tmp/home-test");
        }
        let dir = user_font_dir().expect("Linux fallback resolves");
        assert_eq!(
            dir,
            PathBuf::from("/tmp/home-test/.local/share/net.laser5.app.emterm/fonts")
        );
        unsafe {
            match prev_xdg {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    /// TS-10: scan_user_dir against a tempdir with one valid .ttf, one
    /// .otf, one .txt, and one corrupt font; resolver must register the
    /// two real fonts only.
    #[test]
    fn scan_dir_into_filters_by_extension_and_skips_corrupt() {
        // Build a tempdir.
        let tmp =
            std::env::temp_dir().join(format!("emterm-user-fonts-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("mkdir tmp");

        // Place fixtures: copy the bundled fonts (which are real .ttf /
        // .otf) so the parser can derive a family name.
        let real_ttf = tmp.join("real.ttf");
        fs::write(&real_ttf, super::super::resolver::BUNDLED_EMOJI_COLOR_FONT)
            .expect("write real.ttf");
        let real_otf = tmp.join("real.otf");
        fs::write(&real_otf, super::super::resolver::BUNDLED_CJK_FONT).expect("write real.otf");
        let txt = tmp.join("README.txt");
        fs::write(&txt, b"not a font").expect("write README.txt");
        let corrupt = tmp.join("corrupt.ttf");
        fs::write(&corrupt, b"NOT A REAL FONT").expect("write corrupt.ttf");

        let mut r = Resolver::new();
        let n = scan_dir_into(&mut r, &tmp);
        assert_eq!(n, 2, "exactly two valid fonts were registered");
        // Both should be present under FontRole::User.
        assert_eq!(r.by_role(FontRole::User).count(), 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Missing user dir → silent no-op (no warn spam, no registrations).
    #[test]
    fn scan_dir_into_missing_dir_is_silent_noop() {
        let mut r = Resolver::new();
        let missing =
            std::env::temp_dir().join(format!("emterm-user-fonts-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        let n = scan_dir_into(&mut r, &missing);
        assert_eq!(n, 0);
        assert_eq!(r.by_role(FontRole::User).count(), 0);
    }

    /// Empty user dir → zero registrations.
    #[test]
    fn scan_dir_into_empty_dir_is_zero_registrations() {
        let tmp =
            std::env::temp_dir().join(format!("emterm-user-fonts-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("mkdir tmp");

        let mut r = Resolver::new();
        let n = scan_dir_into(&mut r, &tmp);
        assert_eq!(n, 0);
        assert_eq!(r.by_role(FontRole::User).count(), 0);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Symlinks under the user-dir must NOT be traversed even when they
    /// carry a `.ttf` / `.otf` extension. This guards the startup scan
    /// against a hostile link to e.g. `/dev/zero` or a giant out-of-tree
    /// file.
    #[test]
    #[cfg(unix)]
    fn scan_dir_into_skips_symlinks() {
        use std::os::unix::fs::symlink;

        let tmp =
            std::env::temp_dir().join(format!("emterm-user-fonts-symlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("mkdir tmp");

        // Real font: target of the symlink. Place it outside the scan
        // directory so the only path into the loader is via the link.
        let outside = tmp.join("outside");
        fs::create_dir_all(&outside).expect("mkdir outside");
        let real_target = outside.join("real.ttf");
        fs::write(
            &real_target,
            super::super::resolver::BUNDLED_EMOJI_COLOR_FONT,
        )
        .expect("write target");

        // Symlinked entry inside the scan dir.
        let link = tmp.join("link.ttf");
        symlink(&real_target, &link).expect("symlink");

        // Also place a real font in the scan dir to confirm the loop
        // continues after skipping the symlink.
        let real_in_dir = tmp.join("real.otf");
        fs::write(&real_in_dir, super::super::resolver::BUNDLED_CJK_FONT).expect("write real.otf");

        let mut r = Resolver::new();
        let n = scan_dir_into(&mut r, &tmp);
        assert_eq!(n, 1, "only the non-symlink real font registers");
        assert_eq!(r.by_role(FontRole::User).count(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Oversized entries (> 64 MiB) are skipped before the read so the
    /// resolver scan can't be stalled by a hostile multi-GiB file.
    #[test]
    fn scan_dir_into_skips_oversized() {
        let tmp = std::env::temp_dir().join(format!(
            "emterm-user-fonts-oversized-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("mkdir tmp");

        // Write a sparse "font" just over the 64 MiB cap. `set_len`
        // avoids actually emitting 64 MiB of bytes on filesystems that
        // support sparse files (ext4, tmpfs, NTFS), and where it does
        // not the test still runs but slower — both outcomes are
        // acceptable for a single CI invocation.
        let oversized = tmp.join("big.ttf");
        {
            let f = fs::File::create(&oversized).expect("create big.ttf");
            f.set_len(super::MAX_USER_FONT_BYTES + 1)
                .expect("set_len oversized");
        }

        // A normal-size real font must still register so we know the
        // skip path didn't poison the loop.
        let real = tmp.join("real.ttf");
        fs::write(&real, super::super::resolver::BUNDLED_EMOJI_COLOR_FONT).expect("write real.ttf");

        let mut r = Resolver::new();
        let n = scan_dir_into(&mut r, &tmp);
        assert_eq!(n, 1, "only the under-cap font registers");
        assert_eq!(r.by_role(FontRole::User).count(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// TS-11: a user-dir entry registered with the same family name as
    /// a bundled font wins on `by_family` lookup (because we register
    /// the user copy first and `register_bytes` short-circuits on the
    /// first family-name match).
    #[test]
    fn user_dir_entry_wins_family_lookup_over_bundle() {
        let tmp =
            std::env::temp_dir().join(format!("emterm-user-fonts-override-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("mkdir tmp");
        let override_path = tmp.join("noto.ttf");
        fs::write(
            &override_path,
            super::super::resolver::BUNDLED_EMOJI_COLOR_FONT,
        )
        .expect("write override");

        let mut r = Resolver::new();
        // User dir first, then bundle (mimics the resolver build order
        // documented in resolver.rs).
        let _ = scan_dir_into(&mut r, &tmp);
        let _ = r.register_bundled();

        // The user copy carries the same family name as the bundle's
        // NotoColorEmoji ("Noto Color Emoji"). Family lookup must
        // resolve to the User entry, not the bundled `(bundled)` one.
        let entry = r
            .by_family("Noto Color Emoji")
            .expect("user entry by family");
        assert_eq!(entry.role, FontRole::User);

        let _ = fs::remove_dir_all(&tmp);
    }
}
