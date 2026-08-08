use super::*;
use crate::status_bar::{OscRow, StatusBarViewModel};
use egui::RawInput;

fn make_text_run(text: &str) -> RichTextRun {
    RichTextRun {
        text: text.to_string(),
        bold: false,
        italic: false,
        underline: false,
        color: None,
        line_break: false,
    }
}

fn collected_text(items: &[egui::epaint::ClippedShape]) -> String {
    let mut out = String::new();
    for cs in items {
        walk_shape(&cs.shape, &mut out);
    }
    out
}

fn walk_shape(shape: &egui::epaint::Shape, out: &mut String) {
    use egui::epaint::Shape;
    match shape {
        Shape::Text(t) => {
            for row in &t.galley.rows {
                for g in &row.glyphs {
                    out.push(g.chr);
                }
                out.push('\n');
            }
        }
        Shape::Vec(v) => {
            for s in v {
                walk_shape(s, out);
            }
        }
        _ => {}
    }
}

/// Collect `(left_x, text)` for every text shape, sorted by screen
/// x. Lets a test assert the visual left-to-right ordering of the
/// segments a row paints, independent of paint order.
fn text_shapes_by_x(items: &[egui::epaint::ClippedShape]) -> Vec<(f32, String)> {
    let mut out: Vec<(f32, String)> = Vec::new();
    for cs in items {
        collect_text_shapes(&cs.shape, &mut out);
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    out
}

fn collect_text_shapes(shape: &egui::epaint::Shape, out: &mut Vec<(f32, String)>) {
    use egui::epaint::Shape;
    match shape {
        Shape::Text(t) => {
            let mut s = String::new();
            for row in &t.galley.rows {
                for g in &row.glyphs {
                    s.push(g.chr);
                }
            }
            if !s.is_empty() {
                out.push((t.pos.x, s));
            }
        }
        Shape::Vec(v) => {
            for s in v {
                collect_text_shapes(s, out);
            }
        }
        _ => {}
    }
}

fn run_one_frame(vm: &StatusBarViewModel) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    let mut input = RawInput::default();
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(800.0, 200.0),
    ));
    let output = ctx.run(input, |ctx| {
        draw(ctx, vm, None);
        egui::CentralPanel::default().show(ctx, |_ui| {});
    });
    output.shapes
}

fn run_with_central_rect(vm: &StatusBarViewModel) -> (Vec<egui::epaint::ClippedShape>, egui::Rect) {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0));
    let mut input = RawInput::default();
    input.screen_rect = Some(screen);
    let mut central_rect = egui::Rect::NOTHING;
    let output = ctx.run(input, |ctx| {
        draw(ctx, vm, None);
        egui::CentralPanel::default().show(ctx, |ui| {
            central_rect = ui.max_rect();
        });
    });
    (output.shapes, central_rect)
}

// TS-23 (replacement): disabled view model inserts no panel.
#[test]
fn disabled_view_model_does_not_insert_panel() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = false;
    let (_shapes, central_off) = run_with_central_rect(&vm);

    let mut vm_on = StatusBarViewModel::default();
    vm_on.enabled = true;
    vm_on.app_line1.left = vec![make_text_run("hi")];
    let (_shapes_on, central_on) = run_with_central_rect(&vm_on);

    assert!(
        central_off.height() > central_on.height(),
        "disabled status bar must leave the central panel taller \
         (off={central_off:?}, on={central_on:?})"
    );
}

// TS-24: App Line 2 hidden when empty (App Line 1 has content here,
// so it stays visible in both frames of this comparison).
#[test]
fn app_line2_auto_hides_when_empty() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    // app_line2 left/right are empty
    let (_shapes, central_one_row) = run_with_central_rect(&vm);

    let mut vm_two = vm.clone();
    vm_two.app_line2.left = vec![make_text_run("L2")];
    let (_shapes_two, central_two_row) = run_with_central_rect(&vm_two);

    // Adding a second row shrinks the central panel by ROW_HEIGHT.
    assert!(
        central_one_row.height() > central_two_row.height(),
        "Adding App Line 2 must shrink central panel; \
         one_row={central_one_row:?} two_row={central_two_row:?}"
    );
}

// OSC row hidden when there is no content (mux-status-bar-removal
// task0001: formerly "TS-26 ... and no mux session" — the OSC row
// has no mux-conditional path left to test).
#[test]
fn osc_row_hidden_when_empty() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("only_app_row")];
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    // The text must show app row but no `[mux:` prefix.
    assert!(text.contains("only_app_row"));
    assert!(!text.contains("[mux:"));
}

// OSC row sourced from the dispatcher shows even without a session
// badge (mux-status-bar-removal task0001: formerly "TS-25 ...
// populated from mux state" — the OSC row is now always
// dispatcher-sourced, so this is the only remaining scenario).
#[test]
fn osc_row_from_dispatcher_renders_without_mux_badge() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    vm.osc = OscRow {
        left: "manual-left".to_string(),
        right: "manual-right".to_string(),
        forced_visible: Some(true),
    };
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(text.contains("manual-left"));
    assert!(text.contains("manual-right"));
    assert!(!text.contains("[mux:"));
}

// A run with an emoji adjacent to text (`🤖 5h`) is split into an
// emoji segment + a text segment. The boundary space is stripped
// from the galley and re-added as a layout advance, so the `5h`
// text shape must start further right than it would with no gap.
// Compare against the same run without the space: the gap variant's
// core text must sit to the right.
#[test]
fn emoji_adjacent_space_widens_layout() {
    fn core_x(run_text: &str) -> f32 {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = true;
        vm.app_line1.left = vec![make_text_run(run_text)];
        let shapes = run_one_frame(&vm);
        // `emoji: None` renders the robot through the text fallback,
        // so both the emoji and `5h` surface as text shapes. Find
        // the `5h` shape's x.
        text_shapes_by_x(&shapes)
            .into_iter()
            .find(|(_, s)| s.contains('5'))
            .map(|(x, _)| x)
            .expect("`5h` text shape missing")
    }
    let with_space = core_x("\u{1F916} 5h");
    let without_space = core_x("\u{1F916}5h");
    assert!(
        with_space > without_space,
        "boundary space must push `5h` right: with={with_space}, \
         without={without_space}"
    );
}

// An App row's right section (e.g. App Line 2 right =
// `{cmd:claude-usage} | {time}`, where the usage value leads with
// a 🤖 emoji) is painted inside a right-to-left layout. The
// leading emoji of a single run must sit at the left of the right
// cluster, not flip to the far right. Pins the per-run segment
// reversal in `emit_run`.
#[test]
fn app_row_right_section_segments_read_left_to_right() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    // One run carrying emoji + text, mirroring a `{cmd:…}` value
    // like `🤖 95%`. `emoji: None` routes the emoji segment through
    // the text fallback so both segments surface as orderable text
    // shapes.
    vm.app_line2.right = vec![make_text_run("\u{1F916} 95%")];
    let shapes = run_one_frame(&vm);
    let by_x = text_shapes_by_x(&shapes);
    let joined: String = by_x.iter().map(|(_, s)| s.as_str()).collect();
    let robot = joined.find('\u{1F916}').expect("robot emoji missing");
    let pct = joined.find("95%").expect("usage text missing");
    assert!(
        robot < pct,
        "leading emoji must sit left of the run's text in the App \
         row right section; got shapes-by-x = {by_x:?}"
    );
}

// The OSC row's right section is painted inside a right-to-left
// layout. Without reversing the segment walk the source-order
// leading segment lands furthest right; this test pins the
// source order reading left-to-right on screen so a leading emoji
// (e.g. `🤖 95% 12:34`) stays at the left of the right cluster.
#[test]
fn osc_right_section_segments_read_left_to_right() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    // `🤖` forms its own emoji segment; the trailing text is a
    // second segment. `emoji: None` routes the emoji segment
    // through the text fallback, so both segments surface as text
    // shapes we can order by x.
    vm.osc = OscRow {
        left: String::new(),
        right: "\u{1F916} END".to_string(),
        forced_visible: Some(true),
    };
    let shapes = run_one_frame(&vm);
    let by_x = text_shapes_by_x(&shapes);
    let joined: String = by_x.iter().map(|(_, s)| s.as_str()).collect();
    let robot = joined.find('\u{1F916}').expect("robot emoji missing");
    let end = joined.find("END").expect("trailing text missing");
    assert!(
        robot < end,
        "leading emoji must sit left of trailing text in the \
         right section; got shapes-by-x = {by_x:?}"
    );
}

// A left section longer than half the row is truncated with a
// trailing ellipsis: the prefix survives, the tail is dropped, and
// the `…` appears at the right edge of the kept text.
#[test]
fn left_section_truncates_with_trailing_ellipsis() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    // ~120 chars >> half of an 800px row at 12pt monospace.
    let long = "ABCDEFGHIJ".repeat(12);
    vm.app_line1.left = vec![make_text_run(&long)];
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(text.contains('\u{2026}'), "ellipsis missing: {text:?}");
    // Prefix kept, tail dropped.
    assert!(text.contains('A'), "prefix dropped: {text:?}");
    let kept_len = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    assert!(
        kept_len < long.len(),
        "nothing was truncated ({kept_len} of {})",
        long.len()
    );
    // The ellipsis follows the kept prefix in reading order (the
    // kept atoms coalesce into one galley `ABC…AB…`, so assert
    // the order within the text rather than by x position).
    let ell_pos = text.find('\u{2026}').expect("ellipsis missing");
    let first_alpha = text
        .find(|c: char| c.is_ascii_alphabetic())
        .expect("kept prefix missing");
    assert!(
        ell_pos > first_alpha,
        "trailing ellipsis must follow the kept prefix: {text:?}"
    );
}

// A short right section hugs the panel's right edge: its rightmost
// glyph sits near the available width, not floating at the centre.
#[test]
fn short_right_section_hugs_right_edge() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    vm.app_line1.right = vec![make_text_run("RR")];
    let shapes = run_one_frame(&vm);
    let by_x = text_shapes_by_x(&shapes);
    let rr_x = by_x
        .iter()
        .find(|(_, s)| s.contains("RR"))
        .map(|(x, _)| *x)
        .expect("right text missing");
    // 800px screen − 8px panel inset on each side ⇒ content area
    // ~784px, right edge ~792px. The 2-char run must start well past
    // the centre (~400px) to be right-aligned rather than centred.
    assert!(
        rr_x > 600.0,
        "short right section should hug the right edge, got x={rr_x}"
    );
}

// The right section is right-aligned and truncates its tail: its
// leading content (`🤖 5h …`, the most important part) survives and
// the overflow drops off the right behind a trailing `…`.
#[test]
fn right_section_truncates_with_trailing_ellipsis() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    // Distinct head vs tail so we can assert which side survives.
    let long = format!("HEAD{}", "TAIL".repeat(40));
    vm.app_line1.right = vec![make_text_run(&long)];
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(text.contains('\u{2026}'), "ellipsis missing: {text:?}");
    // Head (leading) kept; the tail is truncated.
    assert!(text.contains("HEAD"), "prefix dropped: {text:?}");
    let tail_count = text.matches("TAIL").count();
    assert!(
        tail_count < 40,
        "trailing content not truncated ({tail_count} TAIL blocks remain)"
    );
    // The ellipsis follows the kept prefix in reading order.
    let ell_pos = text.find('\u{2026}').expect("ellipsis missing");
    let head_pos = text.find("HEAD").expect("HEAD missing");
    assert!(
        ell_pos > head_pos,
        "trailing ellipsis must follow the kept prefix: {text:?}"
    );
}

// Left and right sections that would each overflow must not paint
// past the row centre into one another.
#[test]
fn left_and_right_sections_do_not_overlap() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    let long_l = "L".repeat(200);
    let long_r = "R".repeat(200);
    vm.app_line1.left = vec![make_text_run(&long_l)];
    vm.app_line1.right = vec![make_text_run(&long_r)];
    let shapes = run_one_frame(&vm);
    let by_x = text_shapes_by_x(&shapes);
    // Rightmost x of any 'L' shape must stay left of the leftmost x
    // of any 'R' shape (centre is ~400px on an 800px row).
    let max_l = by_x
        .iter()
        .filter(|(_, s)| s.contains('L'))
        .map(|(x, _)| *x)
        .fold(f32::MIN, f32::max);
    let min_r = by_x
        .iter()
        .filter(|(_, s)| s.contains('R'))
        .map(|(x, _)| *x)
        .fold(f32::MAX, f32::min);
    assert!(
        max_l <= min_r,
        "left and right sections overlap: max_L_x={max_l}, min_R_x={min_r}"
    );
}

// Enabled view model with content reserves panel height.
#[test]
fn enabled_status_bar_reserves_panel_height() {
    let mut vm_off = StatusBarViewModel::default();
    vm_off.enabled = false;
    let mut vm_on = StatusBarViewModel::default();
    vm_on.enabled = true;
    vm_on.app_line1.left = vec![make_text_run("x")];
    let (_, central_off) = run_with_central_rect(&vm_off);
    let (_, central_on) = run_with_central_rect(&vm_on);
    assert!(
        central_off.height() > central_on.height(),
        "enabling the status bar must shrink the central panel \
         (off={central_off:?}, on={central_on:?})"
    );
}

// Both forced_visible=Some(false) skips OSC even when content is
// present.
#[test]
fn osc_force_hide_skips_row() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1")];
    vm.osc = OscRow {
        left: "hidden".to_string(),
        right: String::new(),
        forced_visible: Some(false),
    };
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(!text.contains("hidden"));
}

// AC-1: enabled with OSC row, App Line 1, and App Line 2 all empty
// yields 0 visible rows and 0 panel height (full collapse).
#[test]
fn ac1_all_rows_empty_yields_zero_count_and_zero_height() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    // app_line1 / app_line2 / osc stay at their empty defaults.
    assert_eq!(visible_row_count(&vm), 0);
    assert_eq!(panel_height_logical(&vm), 0.0);
}

// AC-2: App Line 1 empty, OSC row forced visible with content ->
// exactly one visible row, and only the OSC row's text is drawn.
#[test]
fn ac2_only_osc_visible_counts_as_one_row_and_draws_only_osc() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    // app_line1 / app_line2 stay empty.
    vm.osc = OscRow {
        left: "osc-only".to_string(),
        right: String::new(),
        forced_visible: Some(true),
    };
    assert_eq!(visible_row_count(&vm), 1);
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(text.contains("osc-only"), "OSC text missing: {text:?}");
}

// AC-3 (regression guard): App Line 1 with resolved content is
// counted and drawn.
#[test]
fn ac3_app_line1_with_content_is_counted_and_drawn() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("L1-content")];
    assert_eq!(visible_row_count(&vm), 1);
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(text.contains("L1-content"), "L1 text missing: {text:?}");
}

// AC-4: App Line 1 empty, App Line 2 has content -> App Line 1
// stays hidden while App Line 2 shows.
#[test]
fn ac4_app_line1_hidden_app_line2_shown_when_only_line2_has_content() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    // app_line1 stays empty.
    vm.app_line2.left = vec![make_text_run("L2-content")];
    assert_eq!(visible_row_count(&vm), 1);
    let shapes = run_one_frame(&vm);
    let text = collected_text(&shapes);
    assert!(text.contains("L2-content"), "L2 text missing: {text:?}");
    assert!(
        !text.contains("L1-content"),
        "unexpected L1 text present: {text:?}"
    );
}

// AC-5: a disabled view model yields 0 rows even when every row
// has content.
#[test]
fn ac5_disabled_view_model_yields_zero_rows_regardless_of_content() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = false;
    vm.app_line1.left = vec![make_text_run("L1")];
    vm.app_line2.left = vec![make_text_run("L2")];
    vm.osc = OscRow {
        left: "osc".to_string(),
        right: String::new(),
        forced_visible: Some(true),
    };
    assert_eq!(visible_row_count(&vm), 0);
    assert_eq!(panel_height_logical(&vm), 0.0);
}

// Edge case (Test Notes): a run list containing only empty-text,
// non-line-break runs resolves to "no content" — same predicate
// App Line 2 already relies on — so App Line 1 stays hidden.
#[test]
fn app_line1_with_only_empty_text_run_is_hidden() {
    let mut vm = StatusBarViewModel::default();
    vm.enabled = true;
    vm.app_line1.left = vec![make_text_run("")];
    // app_line2 / osc stay empty too, so a wrongly-counted App
    // Line 1 would be the only thing keeping the count above 0.
    assert_eq!(visible_row_count(&vm), 0);
    assert_eq!(panel_height_logical(&vm), 0.0);
}
