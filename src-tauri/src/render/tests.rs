use super::*;
use crate::render::terminal_grid_pass::CellInput;
use crate::render::theme::Rgb;
use crate::settings::AmbiguousWidthMode;
use term_core::cell::{STYLE_BOLD, STYLE_REVERSE};
use term_core::terminal_core::TerminalCore;

// ── task0001 AC-4: frame-event "any fired" predicate ────────────────

#[test]
fn ac4_frame_events_any_false_for_default_true_for_each_remaining_field() {
    // AC-4: the struct literal below has exactly the fields
    // `FrameEvents` is meant to carry after the sidebar's
    // clipboard-copy channel is removed. Compiles only once that
    // field is gone (catches an incomplete removal at compile time,
    // same idea as AC-3's exhaustive pattern in `ui::mux_sidebar`).
    let base = || FrameEvents {
        title: None,
        tab: None,
        scroll_to: None,
        search: None,
        profile: None,
        sftp: None,
    };
    assert!(
        !base().any(),
        "an all-None FrameEvents must report no event fired"
    );
    assert!(
        FrameEvents {
            title: Some(crate::ui::TitleBarEvent::Close),
            ..base()
        }
        .any()
    );
    assert!(
        FrameEvents {
            tab: Some(crate::ui::TabEvent::New),
            ..base()
        }
        .any()
    );
    assert!(
        FrameEvents {
            scroll_to: Some(0),
            ..base()
        }
        .any()
    );
    assert!(
        FrameEvents {
            search: Some(crate::ui::search_bar::SearchBarEvent::Close),
            ..base()
        }
        .any()
    );
    assert!(
        FrameEvents {
            profile: Some(crate::ui::profile_selector::ProfileSelectorEvent::Cancel),
            ..base()
        }
        .any()
    );
    assert!(
        FrameEvents {
            sftp: Some(SftpFrameEvent::CancelUpload),
            ..base()
        }
        .any()
    );
}

// ── task0006 AC-3: cursor/search origin carry no sidebar term ──────

/// Build an `App` whose single tab spawned no real shell process
/// ([`crate::tabs::Tab::test_shell_less`]), mirroring `app/tests.rs`'s
/// helper of the same shape (duplicated here rather than shared across
/// modules — both are private `#[cfg(test)]` helpers).
fn app_with_shell_less_tab() -> App {
    let mut app = App::new();
    let dims = app.cell_size;
    let tab = crate::tabs::Tab::test_shell_less(
        "shell",
        dims.cols,
        dims.rows,
        app.settings.scrollback_lines,
        app.settings.clone(),
        app.notification_sink.clone(),
    );
    app.tabs.push(tab);
    app.active = 0;
    app
}

/// Attach the active tab to a single-window mux session (mirrors
/// `window_host/tests.rs`'s helper of the same name). With the
/// overlay-mode setting at its default (`false`), this flips
/// `mux_sidebar_visibility()` from `Hidden` to `Persistent`.
fn attach_active_tab_to_mux_session(app: &mut App) {
    use mux_ipc::protocol::{MessageType, MuxMessage, SessionInfo, WelcomeMsg, WindowInfo};
    let windows = vec![WindowInfo {
        id: 1,
        name: "win".to_string(),
        active_pane_id: 10,
    }];
    let session = SessionInfo {
        id: 1,
        name: "main".to_string(),
        window_count: windows.len() as u32,
        pane_count: windows.len() as u32,
        active_window_index: 0,
        windows,
    };
    let welcome = MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![session],
        },
    );
    app.on_mux_message(0, welcome);
}

/// Run one egui frame that paints exactly the two AC-3 overlay passes
/// (`draw_cursor` + `draw_search_highlights`) and return every shape it
/// produced. A fresh `Context` + default `RawInput` per call keeps the
/// two invocations below comparable shape-for-shape.
fn overlay_pass_shapes(
    app: &App,
    core: &TerminalCore,
    theme: &Theme,
) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            super::overlays::draw_cursor(ui, core, theme, app);
            super::overlays::draw_search_highlights(ui, core, app);
        });
    });
    output.shapes
}

/// Regression guard for the right-edge placement update (AC-3):
/// `draw_cursor` and `draw_search_highlights` must not read any
/// mux-sidebar inset into their x-origin — the persistent sidebar
/// reserves grid WIDTH only. Verified behaviorally: both functions are
/// exercised for real with the sidebar `Hidden` and then `Persistent`,
/// and their full painted output must be identical. Any sidebar term
/// sneaking into the origin math would shift the painted coordinates
/// between the two states and fail the comparison. (Replaces a former
/// source-text scan that was coupled to `overlays.rs`'s file layout and
/// visibility spelling.)
#[test]
fn draw_cursor_and_search_highlights_paint_identically_with_persistent_sidebar() {
    let mut app = app_with_shell_less_tab();
    // Persistent (not overlay) sidebar mode — the variant that reserves
    // grid width and therefore the one AC-3 constrains. `app.settings`
    // is an `Arc<Settings>` — clone-and-flip to mutate, mirroring
    // `window_host/tests.rs`'s pattern for the same purpose.
    app.settings = std::sync::Arc::new({
        let mut s = (*app.settings).clone();
        s.mux.window_sidebar_overlay = false;
        s
    });
    // Unfocused window: `draw_cursor` holds the cursor at its steady
    // "on" phase (hollow outline), bypassing the blink clock so the
    // paint is wall-clock independent.
    app.window_focused = false;
    // Non-degenerate cell metrics so a sidebar term leaking into the
    // origin would move the shapes by a visible amount.
    app.cell_w_logical = 9.0;
    app.cell_h_logical = 18.0;
    // One in-viewport search match so `draw_search_highlights` paints.
    app.search.visible = true;
    app.search.current_index = 0;
    app.search.matches = vec![crate::search::SearchMatch {
        segments: vec![crate::search::MatchSegment {
            abs_row: 0,
            col_start: 2,
            col_end: 5,
        }],
    }];

    let mut core = TerminalCore::new(80, 24, 100);
    core.set_cursor_visible(true);
    core.set_cursor(3, 1);
    let theme = Theme::default();

    assert_eq!(
        app.mux_sidebar_visibility(),
        crate::app::MuxSidebarVisibility::Hidden,
        "precondition: an unattached tab shows no sidebar"
    );
    let shapes_hidden = overlay_pass_shapes(&app, &core, &theme);

    attach_active_tab_to_mux_session(&mut app);
    assert_eq!(
        app.mux_sidebar_visibility(),
        crate::app::MuxSidebarVisibility::Persistent,
        "precondition: mux attach with overlay mode off shows the persistent sidebar"
    );
    let shapes_persistent = overlay_pass_shapes(&app, &core, &theme);

    // Non-vacuousness: the overlay passes painted something beyond the
    // bare panel background — the cursor outline and the highlight rect
    // are actually present, so the equality below compares real output.
    let baseline = {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |_ui| {});
        });
        output.shapes
    };
    assert!(
        shapes_hidden.len() >= baseline.len() + 2,
        "expected the cursor outline and the search-highlight rect on \
         top of the {} bare-panel shape(s), got {} total",
        baseline.len(),
        shapes_hidden.len()
    );

    assert_eq!(
        shapes_hidden, shapes_persistent,
        "cursor / search-highlight paint must be identical with and \
         without the persistent sidebar (AC-3): the sidebar reserves \
         grid WIDTH only — no x-origin term"
    );
}

#[test]
fn visible_width_narrow_for_ascii() {
    assert_eq!(visible_width("A", AmbiguousWidthMode::Narrow), 1);
    assert_eq!(visible_width("a", AmbiguousWidthMode::Wide), 1);
}

#[test]
fn visible_width_wide_for_cjk() {
    // U+4E00 is "wide" unconditionally — both modes must report 2.
    assert_eq!(visible_width("一", AmbiguousWidthMode::Narrow), 2);
    assert_eq!(visible_width("一", AmbiguousWidthMode::Wide), 2);
}

#[test]
fn visible_width_respects_ambiguous_mode() {
    // U+25A0 (BLACK SQUARE) is in the Unicode "Ambiguous" East-Asian
    // width class.
    assert_eq!(visible_width("■", AmbiguousWidthMode::Narrow), 1);
    assert_eq!(visible_width("■", AmbiguousWidthMode::Wide), 2);
}

#[test]
fn visible_width_vs16_upgrades_text_default_to_wide() {
    // Text-default emoji codepoints have a bare width of 1, but a
    // trailing VS-16 (U+FE0F) explicitly requests emoji presentation
    // and must widen the cluster to 2 cells so the color-emoji glyph
    // gets the wide slot term_core reserved for it.
    // 🕊️ = U+1F54A DOVE + U+FE0F.
    assert_eq!(visible_width("\u{1F54A}", AmbiguousWidthMode::Narrow), 1);
    assert_eq!(
        visible_width("\u{1F54A}\u{FE0F}", AmbiguousWidthMode::Narrow),
        2
    );
    // ⬇️ = U+2B07 + U+FE0F.
    assert_eq!(visible_width("\u{2B07}", AmbiguousWidthMode::Narrow), 1);
    assert_eq!(
        visible_width("\u{2B07}\u{FE0F}", AmbiguousWidthMode::Narrow),
        2
    );
    // ⚠️ = U+26A0 + U+FE0F (also picks up emoji presentation under
    // VS-16 even though bare U+26A0 is ambiguous-width).
    assert_eq!(
        visible_width("\u{26A0}\u{FE0F}", AmbiguousWidthMode::Narrow),
        2
    );
    // Counter-check: 🥺 U+1F97A is emoji-presentation default, so it
    // is already 2 cells without VS-16.
    assert_eq!(visible_width("\u{1F97A}", AmbiguousWidthMode::Narrow), 2);
}

#[test]
fn visible_width_vs15_downgrades_emoji_default_to_narrow() {
    // Symmetric to the VS-16 upgrade: emoji-presentation default
    // codepoints have a bare width of 2, but a trailing VS-15
    // (U+FE0E) explicitly requests text presentation and must
    // narrow the cluster to 1 cell. Mirrors
    // `term_core::print_handler::flush_grapheme_buffer`'s
    // `has_fe0e` branch.
    // 😀 = U+1F600 (emoji-presentation default → 2 cells).
    assert_eq!(visible_width("\u{1F600}", AmbiguousWidthMode::Narrow), 2);
    // 😀︎ = U+1F600 + U+FE0E (text presentation → 1 cell).
    assert_eq!(
        visible_width("\u{1F600}\u{FE0E}", AmbiguousWidthMode::Narrow),
        1
    );
    // ⌚︎ = U+231A WATCH + U+FE0E. Bare U+231A is emoji-presentation
    // default so widening would otherwise hit; VS-15 narrows it.
    assert_eq!(visible_width("\u{231A}", AmbiguousWidthMode::Narrow), 2);
    assert_eq!(
        visible_width("\u{231A}\u{FE0E}", AmbiguousWidthMode::Narrow),
        1
    );
    // VS-15 wins over VS-16 when both appear, matching the
    // `if has_fe0e { 1 } else if has_fe0f { 2 }` ordering term_core
    // uses in `flush_grapheme_buffer`.
    assert_eq!(
        visible_width("\u{1F600}\u{FE0E}\u{FE0F}", AmbiguousWidthMode::Narrow),
        1
    );
    assert_eq!(
        visible_width("\u{1F600}\u{FE0F}\u{FE0E}", AmbiguousWidthMode::Narrow),
        1
    );
}

#[test]
fn visible_width_minimum_one_for_empty_or_combining() {
    assert_eq!(visible_width("", AmbiguousWidthMode::Narrow), 1);
    // U+0301 (combining acute accent) reports width 0 from
    // display_width; visible_width must floor to 1 so iteration
    // makes progress.
    assert_eq!(visible_width("\u{0301}", AmbiguousWidthMode::Narrow), 1);
}

#[test]
fn blend_toward_endpoints_match_inputs() {
    let a = Color32::from_rgb(0, 0, 0);
    let b = Color32::from_rgb(255, 255, 255);
    assert_eq!(blend_toward(a, b, 0.0), a);
    assert_eq!(blend_toward(a, b, 1.0).r(), 255);
}

#[test]
fn blend_toward_midpoint_is_average() {
    let a = Color32::from_rgb(0, 0, 0);
    let b = Color32::from_rgb(200, 100, 50);
    let m = blend_toward(a, b, 0.5);
    assert_eq!(m.r(), 100);
    assert_eq!(m.g(), 50);
    assert_eq!(m.b(), 25);
}

#[test]
fn bold_brighten_packed_promotes_indexed_0_7() {
    // tag=1 (indexed), index=3 (yellow) → index=11 (bright yellow)
    let packed_red = (1u32 << 24) | (1u32 << 16);
    assert_eq!(
        bold_brighten_packed(packed_red),
        (1u32 << 24) | (9u32 << 16)
    );

    let packed_yellow = (1u32 << 24) | (3u32 << 16);
    assert_eq!(
        bold_brighten_packed(packed_yellow),
        (1u32 << 24) | (11u32 << 16)
    );
}

#[test]
fn bold_brighten_packed_leaves_already_bright_alone() {
    // index 8..16 are already bright; pass through unchanged.
    let packed = (1u32 << 24) | (10u32 << 16);
    assert_eq!(bold_brighten_packed(packed), packed);
}

#[test]
fn bold_brighten_packed_leaves_truecolor_alone() {
    // tag=2 (truecolor); RGB bits live where the indexed-form `index`
    // byte does, so blindly adding 8 would corrupt the red channel.
    let packed = (2u32 << 24) | 0x00_AA_BB_CC;
    assert_eq!(bold_brighten_packed(packed), packed);
}

#[test]
fn bold_brighten_packed_leaves_default_tag_alone() {
    // tag=0 (default fg). bold_brighten must not mutate.
    let packed = 0u32;
    assert_eq!(bold_brighten_packed(packed), packed);
}

#[test]
fn packed_to_egui_default_returns_none() {
    let theme = Theme::default();
    assert!(packed_to_egui(0x00_00_00_00, Rgb::WHITE, &theme).is_none());
}

#[test]
fn packed_to_egui_indexed_uses_theme_palette() {
    let theme = Theme::default();
    // index = 1 (red) → palette16[1] = WezTerm scheme Rgb(0xff, 0x00, 0x00).
    let packed = 0x01_01_00_00; // tag=1, r=1
    let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
    assert_eq!(c.r(), 0xff);
    assert_eq!(c.g(), 0x00);
    assert_eq!(c.b(), 0x00);
}

#[test]
fn packed_to_egui_truecolor_returns_exact_rgb() {
    let theme = Theme::default();
    let packed = 0x02_AA_BB_CC; // tag=2, r=AA, g=BB, b=CC
    let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
    assert_eq!((c.r(), c.g(), c.b()), (0xAA, 0xBB, 0xCC));
}

// ── sgr-reverse-default-color-swap: TS-1〜TS-6 ──────────────────────

/// TS-1: `\e[7m` 単独適用（fg/bg ともに DEFAULT）で reverse すると、
/// 最終 fg/bg は `theme.fg` / `theme.bg` がスワップされた値になる。
/// 修正前は両 DEFAULT が `packed_to_egui` で `None` を返し、`unwrap_or_else`
/// が `theme.fg` / `theme.bg` をそのまま採用していたため、スワップが NOP
/// となっていた。fallback を reverse に応じて入れ替えることで救済する。
#[test]
fn reverse_with_both_default_swaps_to_theme_bg_and_fg() {
    let theme = Theme::default();
    let style = resolve_cell_style_from_packed(
        &theme,
        0x00_00_00_00, // packed_fg = DEFAULT
        0x00_00_00_00, // packed_bg = DEFAULT
        STYLE_REVERSE,
        false,
    );
    assert_eq!(style.fg, rgb_to_egui(theme.bg));
    assert_eq!(style.bg, rgb_to_egui(theme.fg));
}

/// TS-2: reverse + indexed(1) fg + DEFAULT bg の組み合わせで、最終
/// fg は `theme.bg`（reverse 用 fallback）、最終 bg は indexed(1) の
/// palette16 解決色になる。indexed 側は `packed_to_egui` が `Some(...)`
/// を返しフォールバックを消費しないため、packed-level swap だけで反転。
#[test]
fn reverse_with_indexed_fg_default_bg_swaps() {
    let theme = Theme::default();
    let packed_fg_indexed1 = 0x01_01_00_00; // tag=1, index=1 (red)
    let style = resolve_cell_style_from_packed(
        &theme,
        packed_fg_indexed1,
        0x00_00_00_00, // packed_bg = DEFAULT
        STYLE_REVERSE,
        false,
    );
    assert_eq!(style.fg, rgb_to_egui(theme.bg));
    assert_eq!(style.bg, rgb_to_egui(theme.palette16[1]));
}

/// TS-3: reverse + truecolor 両指定で、最終 fg/bg の RGB が完全に
/// 入れ替わる。truecolor 側は `packed_to_egui` が `Some(...)` を返し
/// フォールバックは消費されないので packed-level swap がそのまま反転として働く。
#[test]
fn reverse_with_truecolor_swaps() {
    let theme = Theme::default();
    let packed_fg = 0x02_11_22_33; // tag=2, R=0x11 G=0x22 B=0x33
    let packed_bg = 0x02_44_55_66; // tag=2, R=0x44 G=0x55 B=0x66
    let style = resolve_cell_style_from_packed(&theme, packed_fg, packed_bg, STYLE_REVERSE, false);
    assert_eq!(
        (style.fg.r(), style.fg.g(), style.fg.b()),
        (0x44, 0x55, 0x66)
    );
    assert_eq!(
        (style.bg.r(), style.bg.g(), style.bg.b()),
        (0x11, 0x22, 0x33)
    );
}

/// TS-4: reverse + selection の同時適用は XOR で打ち消され、結果は
/// non-reverse / non-selected と一致する。FR3（selection swap の不変性）。
#[test]
fn reverse_then_selection_cancels() {
    let theme = Theme::default();
    let style =
        resolve_cell_style_from_packed(&theme, 0x00_00_00_00, 0x00_00_00_00, STYLE_REVERSE, true);
    assert_eq!(style.fg, rgb_to_egui(theme.fg));
    assert_eq!(style.bg, rgb_to_egui(theme.bg));
}

/// TS-5: reverse なし / selection なし / 両 DEFAULT のコントロールケース。
/// `theme.fg` / `theme.bg` がそのまま採用される。
#[test]
fn no_reverse_no_selection_uses_theme_defaults() {
    let theme = Theme::default();
    let style = resolve_cell_style_from_packed(&theme, 0x00_00_00_00, 0x00_00_00_00, 0, false);
    assert_eq!(style.fg, rgb_to_egui(theme.fg));
    assert_eq!(style.bg, rgb_to_egui(theme.bg));
}

/// TS-6: reverse + bold + `bold_brightens_ansi_colors=true` で、
/// packed_fg=DEFAULT・packed_bg=indexed(1) 赤の組み合わせ。
/// reverse 後の perceived foreground（= packed_bg の indexed(1)）に
/// bold-brighten が作用し、最終 fg は indexed(9) bright red、最終 bg は
/// DEFAULT が reverse 用 fallback で解決された `theme.fg` になる。
#[test]
fn reverse_with_bold_brighten_promotes_perceived_fg() {
    let mut theme = Theme::default();
    theme.bold_brightens_ansi_colors = true;
    let packed_bg_indexed1 = 0x01_01_00_00; // tag=1, index=1 (red)
    let style = resolve_cell_style_from_packed(
        &theme,
        0x00_00_00_00, // packed_fg = DEFAULT
        packed_bg_indexed1,
        STYLE_REVERSE | STYLE_BOLD,
        false,
    );
    assert_eq!(style.fg, rgb_to_egui(theme.palette16[9]));
    assert_eq!(style.bg, rgb_to_egui(theme.fg));
}

// ── font-swash-migration: Theme dead_code resolution (FR10) ────────

/// TS-font-11: `Theme::default().font_family` is `"monospace"` and
/// `font_size_pt` is `13.0` (regression guard).
#[test]
fn theme_default_font_family_is_monospace() {
    let t = Theme::default();
    assert_eq!(t.font_family, "monospace");
    assert!((t.font_size_pt - 13.0).abs() < f32::EPSILON);
}

// ── Phase 4-H: collect_cell_inputs ────────────────────────────────

/// `collect_cell_inputs` produces exactly `cols * rows` entries —
/// one per logical cell — even when the grid is mostly blank. The
/// `TerminalGridPass::build_instances` consumer filters
/// whitespace / empty clusters internally, so the renderer can
/// pass the full grid through without an extra pre-filter pass.
#[test]
fn collect_cell_inputs_emits_one_entry_per_cell() {
    let mut core = TerminalCore::new(5, 2, 100);
    core.process_pty_data(b"ABCDE");
    let theme = Theme::default();
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    // 5 cols × 2 rows = 10 cell entries.
    assert_eq!(inputs.len(), 10);
    // Row 0 should carry the literal glyphs in column order.
    let row0: String = inputs
        .iter()
        .filter(|c| c.row == 0)
        .map(|c| c.glyph.as_str())
        .collect();
    assert_eq!(row0, "ABCDE");
}

/// Wide CJK cells advance the iterator by two columns and report
/// `width_cells = 2`. The cell at `col+1` would normally be the
/// trailing half of the wide glyph; `collect_cell_inputs` skips it
/// (`col` advances past it) so a single instance covers the whole
/// wide rectangle.
#[test]
fn collect_cell_inputs_handles_wide_cells() {
    let mut core = TerminalCore::new(4, 1, 100);
    core.process_pty_data("あA".as_bytes());
    let theme = Theme::default();
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    assert_eq!(inputs[0].glyph, "あ");
    assert_eq!(inputs[0].width_cells, 2);
    // Column 2 holds the 'A'; column 1 was skipped (trailing half of あ).
    let a = inputs.iter().find(|c| c.glyph == "A").expect("A present");
    assert_eq!(a.col, 2);
    assert_eq!(a.width_cells, 1);
}

/// Decoration flags propagate from `STYLE_UNDERLINE` /
/// `STYLE_STRIKETHROUGH` SGR bits onto the `CellInput`.
#[test]
fn collect_cell_inputs_propagates_decoration_flags() {
    let mut core = TerminalCore::new(3, 1, 100);
    // SGR 4 = underline; SGR 9 = strikethrough.
    core.process_pty_data(b"\x1b[4mU\x1b[0m\x1b[9mS\x1b[0mN");
    let theme = Theme::default();
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    let u = inputs.iter().find(|c| c.glyph == "U").expect("U present");
    let s = inputs.iter().find(|c| c.glyph == "S").expect("S present");
    let n = inputs.iter().find(|c| c.glyph == "N").expect("N present");
    assert!(u.underline);
    assert!(!u.strikethrough);
    assert!(s.strikethrough);
    assert!(!s.underline);
    assert!(!n.underline);
    assert!(!n.strikethrough);
}

/// Non-default background colors set `draw_background = true`; the
/// default-background cells leave it `false` so the wgpu pass can
/// skip the background quad (the swapchain clear covers it).
#[test]
fn collect_cell_inputs_draw_background_only_when_non_default() {
    let mut core = TerminalCore::new(3, 1, 100);
    // SGR 41 = red background.
    core.process_pty_data(b"\x1b[41mR\x1b[0mN");
    let theme = Theme::default();
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    let r = inputs.iter().find(|c| c.glyph == "R").expect("R present");
    let n = inputs.iter().find(|c| c.glyph == "N").expect("N present");
    assert!(r.draw_background);
    assert!(!n.draw_background);
}

/// AC-1: `collect_cell_inputs` no longer accepts a cursor parameter, so
/// its output cannot depend on where the terminal's cursor sits —
/// there is no code path left that could special-case "the cursor
/// cell". Regression guard: a styled cell that the real terminal
/// cursor is parked on top of still reports its plain resolved fg/bg
/// (no more fg/bg swap for "the cursor cell"; that inversion is now
/// the egui overlay's job — `cursor::draw_block_cursor`).
#[test]
fn collect_cell_inputs_never_inverts_the_cell_under_the_cursor() {
    let mut core = TerminalCore::new(3, 1, 100);
    // SGR 42 = green background.
    core.process_pty_data(b"\x1b[42mX");
    // Move the cursor back onto the styled 'X' cell (row 1, col 1 in
    // 1-based CUP addressing = absolute (0, 0)).
    core.process_pty_data(b"\x1b[1;1H");
    assert_eq!((core.get_cursor_col(), core.get_cursor_row()), (0, 0));

    let theme = Theme::default();
    let packed_fg = core.get_cell_fg(0, 0);
    let packed_bg = core.get_cell_bg(0, 0);
    let flags = core.get_cell_flags(0, 0);
    let expected = resolve_cell_style_from_packed(&theme, packed_fg, packed_bg, flags, false);

    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    let cell = inputs
        .iter()
        .find(|c| c.col == 0 && c.row == 0)
        .expect("cell at cursor position present");
    assert_eq!(cell.fg_rgba, color32_to_rgba(expected.fg));
    assert_eq!(cell.bg_rgba, color32_to_rgba(expected.bg));
}

// ── Scrollback rendering (scroll_offset) ──────────────────────────

/// Helper: collect the on-screen glyph for a given row in reading order,
/// trimming trailing blanks so the assertions read cleanly.
fn row_text(inputs: &[CellInput], row: u16) -> String {
    let mut cells: Vec<&CellInput> = inputs.iter().filter(|c| c.row == row).collect();
    cells.sort_by_key(|c| c.col);
    cells
        .iter()
        .map(|c| c.glyph.as_str())
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// `scroll_offset == 0` produces output identical to the pre-scrollback
/// path: the live viewport is read row-for-row regardless of how much
/// scrollback exists behind it.
#[test]
fn collect_cell_inputs_offset_zero_matches_live() {
    let mut core = TerminalCore::new(5, 2, 100);
    // Push "L0".."L3" so L0/L1 land in scrollback and L2/L3 are live.
    core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3");
    assert!(core.get_scrollback_length() >= 2);
    let theme = Theme::default();
    let live = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    // Live viewport shows the last two logical lines.
    assert_eq!(row_text(&live, 0), "L2");
    assert_eq!(row_text(&live, 1), "L3");
}

/// A non-zero offset surfaces scrollback rows: scrolling back by the full
/// viewport height shows the oldest two rows that had scrolled off.
#[test]
fn collect_cell_inputs_offset_shows_scrollback() {
    let mut core = TerminalCore::new(5, 2, 100);
    core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3");
    let scrollback_len = core.get_scrollback_length();
    assert_eq!(scrollback_len, 2, "L0 and L1 evicted into scrollback");
    let theme = Theme::default();
    // Offset = 2 (one full viewport back) → top of view is absolute row
    // `scrollback_len - 2 = 0`, so rows 0/1 show the scrollback L0/L1.
    let scrolled = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        2,
        None,
        None,
    );
    assert_eq!(row_text(&scrolled, 0), "L0");
    assert_eq!(row_text(&scrolled, 1), "L1");
}

/// An offset that straddles the scrollback↔viewport seam shows a
/// scrollback row on top and a live viewport row below it.
#[test]
fn collect_cell_inputs_offset_spans_boundary() {
    let mut core = TerminalCore::new(5, 2, 100);
    core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3");
    assert_eq!(core.get_scrollback_length(), 2);
    let theme = Theme::default();
    // Offset = 1: top visible absolute row = scrollback_len - 1 = 1
    // (scrollback L1), bottom = absolute row 2 (live L2).
    let scrolled = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        1,
        None,
        None,
    );
    assert_eq!(row_text(&scrolled, 0), "L1");
    assert_eq!(row_text(&scrolled, 1), "L2");
}

/// A wide CJK glyph in a scrollback row reports `width_cells = 2` and the
/// following cell starts at `col + 2` (the width-0 continuation half is
/// dropped by the term_core accessor), matching the live-viewport path.
#[test]
fn collect_cell_inputs_scrollback_handles_wide_cells() {
    let mut core = TerminalCore::new(4, 1, 100);
    // Row 0 carries "あA"; printing a second line scrolls it off into
    // scrollback (1-row viewport).
    core.process_pty_data("あA\r\nX".as_bytes());
    assert_eq!(core.get_scrollback_length(), 1);
    let theme = Theme::default();
    let scrolled = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        1,
        None,
        None,
    );
    let wide = scrolled
        .iter()
        .find(|c| c.glyph == "あ")
        .expect("あ present in scrollback row");
    assert_eq!(wide.col, 0);
    assert_eq!(wide.width_cells, 2);
    let a = scrolled
        .iter()
        .find(|c| c.glyph == "A")
        .expect("A present in scrollback row");
    // Column 2 holds the 'A'; column 1 (trailing half of あ) was skipped.
    assert_eq!(a.col, 2);
    assert_eq!(a.width_cells, 1);
}

/// Scrollback cells carry their SGR style: a bold-underlined cell that
/// scrolled off keeps `bold` / `underline` set on its `CellInput`.
#[test]
fn collect_cell_inputs_scrollback_preserves_style() {
    let mut core = TerminalCore::new(5, 1, 100);
    // SGR 1 = bold, 4 = underline; then scroll the styled row off.
    core.process_pty_data(b"\x1b[1;4mB\x1b[0m\r\nX");
    assert_eq!(core.get_scrollback_length(), 1);
    let theme = Theme::default();
    let scrolled = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        1,
        None,
        None,
    );
    let b = scrolled
        .iter()
        .find(|c| c.glyph == "B")
        .expect("B present in scrollback row");
    assert!(b.bold);
    assert!(b.underline);
}

// ── Fold-aware rendering ──────────────────────────────────────────

/// With a fold layout, `collect_cell_inputs` reads each screen row's
/// *actual* buffer row from the layout (not the linear scrollback
/// model), and emits no cells for a summary row.
#[test]
fn collect_cell_inputs_fold_layout_maps_rows_and_skips_summary() {
    // 5 logical lines L0..L4; 5-row viewport so nothing evicts (all
    // live). Collapse a region over actual rows 1..3 (L1, L2).
    let mut core = TerminalCore::new(5, 5, 100);
    core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3\r\nL4");
    assert_eq!(core.get_scrollback_length(), 0);

    let mut fm = crate::fold::FoldManager::new();
    // Region 1..3 (rows L1,L2) collapsed → summary at display line 1,
    // hides 1 body row (line_count 2 - 1).
    fm.register_osc133_region(1, 3, "cmd".to_string(), Some(0));
    fm.toggle_fold(1);
    // total_actual = 5, hidden = 1, total_display = 4.
    // viewport = 5, offset 0 → display_start = max(0, 4-5-0) = 0.
    let layout = fm.build_layout(0, 5, 0);

    let theme = Theme::default();
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        Some(&layout),
        None,
    );

    // Screen row 0 = actual L0.
    assert_eq!(row_text(&inputs, 0), "L0");
    // Screen row 1 = summary → no cells emitted by collect_cell_inputs.
    assert_eq!(row_text(&inputs, 1), "");
    assert!(
        !inputs.iter().any(|c| c.row == 1),
        "summary row must emit no CellInput (overlay draws it)"
    );
    // Screen row 2 = actual L3 (the body L1/L2 collapsed onto the
    // summary, so the next visible buffer row is L3).
    assert_eq!(row_text(&inputs, 2), "L3");
    // Screen row 3 = actual L4.
    assert_eq!(row_text(&inputs, 3), "L4");
}

/// Without a fold layout (`None`), the row mapping is the unchanged
/// linear path even when collapsed regions exist in the manager — the
/// caller gates passing `Some` on `has_collapsed_regions`, so `None`
/// must reproduce the pre-fold behavior exactly.
#[test]
fn collect_cell_inputs_none_layout_is_linear() {
    let mut core = TerminalCore::new(5, 3, 100);
    core.process_pty_data(b"L0\r\nL1\r\nL2");
    let theme = Theme::default();
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    assert_eq!(row_text(&inputs, 0), "L0");
    assert_eq!(row_text(&inputs, 1), "L1");
    assert_eq!(row_text(&inputs, 2), "L2");
}

// ── task0003 AC-1/AC-6: `only_rows` row-subset mode ────────────────

/// `only_rows = Some(subset)` emits cells for exactly the requested
/// rows — none of the excluded rows' content leaks in.
#[test]
fn collect_cell_inputs_only_rows_restricts_output_to_subset() {
    let mut core = TerminalCore::new(5, 3, 100);
    core.process_pty_data(b"L0\r\nL1\r\nL2");
    let theme = Theme::default();
    let subset = [1u16];
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        Some(&subset),
    );
    assert!(
        inputs.iter().all(|c| c.row == 1),
        "only row 1 should be present: {inputs:?}"
    );
    assert_eq!(row_text(&inputs, 1), "L1");
}

/// AC-1 (equivalence, mod.rs half): for every row in a subset, the
/// `CellInput`s produced by `only_rows = Some(subset)` are identical to
/// the corresponding rows filtered out of a full-grid (`None`) call —
/// the row-subset path must not diverge in per-cell content from the
/// full-grid path it is meant to replace for dirty rows.
#[test]
fn collect_cell_inputs_only_rows_matches_full_grid_filtered() {
    let mut core = TerminalCore::new(6, 4, 100);
    core.process_pty_data(b"\x1b[41mAAAAAA\x1b[0m\r\nBBBBBB\r\nCCCCCC\r\nDDDDDD");
    let theme = Theme::default();
    let full = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    );
    let subset = [0u16, 2u16];
    let partial = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        Some(&subset),
    );
    let mut full_filtered: Vec<&CellInput> =
        full.iter().filter(|c| subset.contains(&c.row)).collect();
    let mut partial_refs: Vec<&CellInput> = partial.iter().collect();
    let key = |c: &&CellInput| (c.row, c.col);
    full_filtered.sort_by_key(key);
    partial_refs.sort_by_key(key);
    assert_eq!(full_filtered.len(), partial_refs.len());
    for (a, b) in full_filtered.iter().zip(partial_refs.iter()) {
        assert_eq!(a.row, b.row);
        assert_eq!(a.col, b.col);
        assert_eq!(a.glyph, b.glyph);
        assert_eq!(a.fg_rgba, b.fg_rgba);
        assert_eq!(a.bg_rgba, b.bg_rgba);
        assert_eq!(a.draw_background, b.draw_background);
    }
}

/// `only_rows = Some(&[])` (an empty subset) walks nothing and returns
/// an empty `Vec` — the AC-3 "clean frame" boundary case at the
/// `collect_cell_inputs` level.
#[test]
fn collect_cell_inputs_only_rows_empty_subset_returns_empty() {
    let mut core = TerminalCore::new(5, 3, 100);
    core.process_pty_data(b"L0\r\nL1\r\nL2");
    let theme = Theme::default();
    let subset: [u16; 0] = [];
    let inputs = collect_cell_inputs(
        &core,
        &theme,
        None,
        AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        Some(&subset),
    );
    assert!(inputs.is_empty());
}

// ── fold_summary_texts ────────────────────────────────────────────

fn osc133_region(cmd: &str, exit: Option<i32>, lines: u32) -> crate::fold::FoldRegion {
    crate::fold::FoldRegion {
        id: format!("osc133:{lines}"),
        start_line: 0,
        end_line: lines,
        collapsed: true,
        source: crate::fold::FoldSource::Osc133,
        command_text: Some(cmd.to_string()),
        label: None,
        exit_code: exit,
        line_count: lines,
    }
}

#[test]
fn fold_summary_texts_osc133_with_exit_zero() {
    let r = osc133_region("ls -la", Some(0), 12);
    let (left, right, err) = fold_summary_texts(&r);
    assert_eq!(left, "\u{25B6} ls -la");
    assert_eq!(right, "\u{2014} 12 lines (exit 0)");
    assert!(!err);
}

#[test]
fn fold_summary_texts_osc133_nonzero_exit_is_error() {
    let r = osc133_region("make", Some(2), 40);
    let (_, right, err) = fold_summary_texts(&r);
    assert_eq!(right, "\u{2014} 40 lines (exit 2)");
    assert!(err);
}

#[test]
fn fold_summary_texts_osc133_no_exit_omits_suffix() {
    let r = osc133_region("running", None, 5);
    let (_, right, err) = fold_summary_texts(&r);
    assert_eq!(right, "\u{2014} 5 lines");
    assert!(!err);
}

#[test]
fn fold_summary_texts_custom_uses_label() {
    let r = crate::fold::FoldRegion {
        id: "custom:0".to_string(),
        start_line: 0,
        end_line: 3,
        collapsed: true,
        source: crate::fold::FoldSource::Custom,
        command_text: None,
        label: Some("Build Output".to_string()),
        exit_code: None,
        line_count: 3,
    };
    let (left, right, err) = fold_summary_texts(&r);
    assert_eq!(left, "\u{25B6} Build Output");
    assert_eq!(right, "\u{2014} 3 lines");
    assert!(!err);
}

#[test]
fn fold_summary_texts_truncates_long_name() {
    // 100-char command → "▶ " + 77 chars + "...".
    let cmd = "a".repeat(100);
    let r = osc133_region(&cmd, Some(0), 1);
    let (left, _, _) = fold_summary_texts(&r);
    let expected_name = format!("{}...", "a".repeat(77));
    assert_eq!(left, format!("\u{25B6} {expected_name}"));
    // The displayed name is exactly 80 chars (77 + "...").
    let name = left.strip_prefix("\u{25B6} ").unwrap();
    assert_eq!(name.chars().count(), 80);
}

#[test]
fn fold_summary_texts_short_name_not_truncated() {
    let cmd = "a".repeat(80);
    let r = osc133_region(&cmd, Some(0), 1);
    let (left, _, _) = fold_summary_texts(&r);
    // Exactly 80 chars is the boundary: not truncated.
    assert_eq!(left, format!("\u{25B6} {cmd}"));
}
