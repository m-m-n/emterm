use super::*;

// ── SFTP close-guard / identity-capture regression tests ──────

#[test]
fn close_guard_resolves_by_stable_id_after_reorder() {
    // #7: the guard holds a stable_id, so a roster change between arming
    // and confirming must not close the wrong (or a missing) tab.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_initial_tab();
    app.spawn_initial_tab();
    assert_eq!(app.tabs.len(), 3);

    // Arm the guard on the *middle* tab's stable_id.
    let target_id = app.tabs[1].stable_id;
    app.sftp_ui.close_guard = Some(target_id);

    // Reorder so the target is no longer at index 1.
    app.reorder_tab(1, 3); // middle tab moves to the end
    let new_idx = app
        .tabs
        .iter()
        .position(|t| t.stable_id == target_id)
        .expect("target still present");
    assert_eq!(new_idx, 2, "target moved to the end");

    // Confirming resolves by id and closes exactly the target tab.
    app.confirm_close_guard();
    assert_eq!(app.tabs.len(), 2);
    assert!(
        app.tabs.iter().all(|t| t.stable_id != target_id),
        "the guarded tab (by id) was the one closed"
    );
    assert!(app.sftp_ui.close_guard.is_none(), "guard cleared");
}

#[test]
fn close_guard_missing_tab_aborts_cleanly() {
    // If the guarded tab vanished, confirm must not panic or close an
    // unrelated tab.
    let mut app = App::new();
    app.spawn_initial_tab();
    let only_id = app.tabs[0].stable_id;
    // Arm the guard on a stable_id that does not exist.
    app.sftp_ui.close_guard = Some(only_id.wrapping_add(9999));

    app.confirm_close_guard();

    // The unrelated tab is untouched; guard is cleared.
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].stable_id, only_id);
    assert!(app.sftp_ui.close_guard.is_none());
}

#[test]
fn confirm_overwrite_uses_captured_tab_not_active() {
    // #4: confirm_overwrite_dialog must drive uploads against the dialog's
    // captured tab_id, and abort (no panic, error toast) when that tab is
    // gone instead of redirecting to the active tab.
    let mut app = App::new();
    app.spawn_initial_tab();
    let live_id = app.tabs[0].stable_id;

    // Overwrite dialog captured for a now-missing tab.
    app.sftp_ui.overwrite_dialog = Some(crate::sftp::ui::OverwriteDialog {
        paths: vec![std::path::PathBuf::from("/a/f.txt")],
        remote_dir: "/remote".to_string(),
        duplicates: vec!["f.txt".to_string()],
        tab_id: live_id.wrapping_add(7777),
        connection: crate::sftp::service::SftpConnection {
            hostname: "h".to_string(),
            port: 22,
            username: String::new(),
            identity_file: String::new(),
            ssh_options: Vec::new(),
        },
    });

    // Should abort with an error toast (the live non-SSH tab is not a
    // valid redirect target).
    app.confirm_overwrite_dialog(0.0);
    assert!(app.sftp_ui.overwrite_dialog.is_none(), "dialog consumed");
    assert!(
        app.sftp_ui
            .toasts
            .toasts
            .iter()
            .any(|t| t.status == crate::sftp::SftpUploadStatus::Failed),
        "an error toast was surfaced instead of redirecting the upload"
    );
}

/// SPEC FR6 #4: the bundled Inconsolata must be the chain's base
/// font when the host has no installed monospace family. The earlier
/// implementation fell through to the bundled CJK font, whose Latin
/// subset is not monospaced and visibly skews grid alignment.
///
/// The `build_font_stack` `#[cfg(not(test))]` gates skip every
/// system-family registration in the test build, so this test
/// exercises exactly the "no host monospace family available" path.
///
/// We assert on the family-name of the registered entry rather than
/// the bytes themselves — the bundled fixture in some dev trees
/// keeps placeholder duplicates of NotoSansCJKjp under the
/// Inconsolata file name until `fetch-fonts.sh` is run, so a byte
/// comparison would be ambiguous.
#[test]
fn build_font_stack_uses_bundled_base_when_host_missing() {
    use crate::render::font::resolver::FontRole;

    let settings = Settings::default();
    let (resolver, chain, _cache, _rasterizer, base_id) = App::build_font_stack(&settings);

    let base = resolver
        .font(base_id)
        .expect("base font must be registered");
    // The bundled Inconsolata is registered under
    // "Inconsolata (bundled)" with `FontRole::Base`. The bundled
    // CJK font is registered under "Noto Sans CJK JP (bundled)"
    // with `FontRole::Cjk`. The regression we're guarding against
    // is the previous `unwrap_or(bundled_cjk_id)` which would have
    // landed `base_id` on the `Cjk` entry.
    assert_eq!(
        base.role,
        FontRole::Base,
        "chain base must carry FontRole::Base; FontRole::Cjk indicates a regression"
    );
    assert_eq!(
        base.family, "Inconsolata (bundled)",
        "chain base must be the bundled Inconsolata when no host monospace family is available"
    );
    // The chain must reach the base font.
    assert!(
        chain.chain().contains(&base_id),
        "fallback chain must include the base font id"
    );
}
