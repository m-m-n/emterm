//! In-app settings panel (the "Settings" tab).
//!
//! Port of the WebView build's `src/settings/settings-panel.ts`
//! list-detail layout, styled per `doc/UI-DESIGN-GUIDELINES.yaml`:
//! a 300px nav column (`surface-container-low`, 48px pill items) and a
//! `surface` content column (24px padding, MD3 Title Large header).
//! All controls come from [`crate::ui::md3_widgets`] so later
//! categories (keybinds, mux, notifications, …) compose the same MD3
//! components.
//!
//! Phase 1 covers three categories (UI appearance / terminal
//! appearance / terminal behavior).
//!
//! Editing model: every widget mutates [`PanelState::draft`] (a working
//! copy of [`crate::settings::Settings`]) and latches a dirty flag. The
//! draw pass emits [`PanelEvent::Changed`] once the user stops
//! interacting (no pointer drag, no text-edit focus), mirroring the
//! WebView's per-control save-on-commit behavior without writing
//! `settings.json` on every drag tick. The caller (`window_host`)
//! persists the draft via [`crate::settings_store`] and applies it to
//! the live app via `App::apply_settings`.

use egui::{Color32, Rect, RichText, Rounding, Stroke, Vec2};

use crate::i18n::Locale;
use crate::settings::Settings;
use crate::ui::md3;
use crate::ui::md3_widgets as w;

/// Width of the category nav column (`.settings-nav` grid column).
const NAV_WIDTH: f32 = 300.0;
/// Nav column inner padding.
const NAV_PADDING: f32 = 12.0;
/// Content column padding (`.settings-content`).
const CONTENT_PADDING: f32 = 24.0;

/// Settings categories shown in the nav, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    UiAppearance,
    TerminalAppearance,
    TerminalBehavior,
}

impl Category {
    const ALL: [Category; 3] = [
        Category::UiAppearance,
        Category::TerminalAppearance,
        Category::TerminalBehavior,
    ];

    fn title(&self, locale: Locale) -> &'static str {
        match (self, locale) {
            (Category::UiAppearance, Locale::Ja) => "UI 外観",
            (Category::UiAppearance, _) => "UI Appearance",
            (Category::TerminalAppearance, Locale::Ja) => "ターミナル外観",
            (Category::TerminalAppearance, _) => "Terminal Appearance",
            (Category::TerminalBehavior, Locale::Ja) => "ターミナル動作",
            (Category::TerminalBehavior, _) => "Terminal Behavior",
        }
    }
}

/// Event emitted by [`draw`] when the draft settled into a new state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEvent {
    /// The draft changed and the user finished interacting: persist +
    /// apply it.
    Changed,
}

/// Per-tab state of the settings panel. Constructed when the Settings
/// tab is first opened, dropped when it is closed.
pub struct PanelState {
    pub category: Category,
    /// Working copy of the app settings. Kept in sync with the live
    /// `App::settings` by the caller after every `Changed` event.
    pub draft: Settings,
    /// Comma-separated text form of `draft.shell_args`, decoded on
    /// commit (the WebView edits the same CSV shape).
    shell_args_text: String,
    /// Set when any widget mutated the draft this session; cleared when
    /// `Changed` is emitted.
    dirty: bool,
    /// Last save failure surfaced by the caller, shown under the nav.
    pub save_error: Option<String>,
    /// Display locale captured at construction (follows the live app
    /// locale on apply).
    pub locale: Locale,
}

impl PanelState {
    pub fn new(settings: &Settings, locale: Locale) -> Self {
        Self {
            category: Category::UiAppearance,
            shell_args_text: settings.shell_args.join(", "),
            draft: settings.clone(),
            dirty: false,
            save_error: None,
            locale,
        }
    }

    /// Re-seed the draft from the (post-apply) live settings so the
    /// panel reflects any clamping the save path performed.
    pub fn sync_from(&mut self, settings: &Settings, locale: Locale) {
        self.draft = settings.clone();
        self.shell_args_text = settings.shell_args.join(", ");
        self.locale = locale;
    }
}

/// Localized label lookup. Japanese strings mirror
/// `src/i18n/locales/ja.json` where an equivalent exists.
fn t(locale: Locale, en: &'static str, ja: &'static str) -> &'static str {
    match locale {
        Locale::Ja => ja,
        Locale::En => en,
    }
}

/// Draw the settings panel into the central panel area. Returns
/// `Some(PanelEvent::Changed)` when the draft settled into a new state
/// this frame.
pub fn draw(ctx: &egui::Context, state: &mut PanelState) -> Option<PanelEvent> {
    let locale = state.locale;
    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(md3::surface())
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            w::apply_visuals(ui);
            let full = ui.max_rect();

            // ── Nav column (fixed 300px, surface-container-low) ──
            let nav_rect =
                Rect::from_min_max(full.min, egui::pos2(full.min.x + NAV_WIDTH, full.max.y));
            ui.painter()
                .rect_filled(nav_rect, 0.0, md3::surface_container_low());
            // 1px border-right (outline-variant).
            ui.painter().vline(
                nav_rect.right(),
                nav_rect.y_range(),
                Stroke::new(1.0, md3::outline_variant()),
            );
            let mut nav_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(nav_rect.shrink(NAV_PADDING))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            draw_nav(&mut nav_ui, state, locale);

            // ── Content column ──
            let content_rect =
                Rect::from_min_max(egui::pos2(nav_rect.right() + 1.0, full.min.y), full.max);
            let mut content_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            draw_content(&mut content_ui, state, locale);
        });

    // Commit once the user stops interacting: no active pointer press /
    // drag and no text-edit focus. This coalesces slider drags and
    // text typing into a single save instead of one per tick.
    let interacting = ctx.is_using_pointer() || ctx.memory(|m| m.focused().is_some());
    if state.dirty && !interacting {
        state.dirty = false;
        // Decode the CSV shell-args text into the draft on commit.
        state.draft.shell_args = state
            .shell_args_text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Some(PanelEvent::Changed)
    } else {
        None
    }
}

// ──────────────────────────────────────────────────────────────────────
// Nav column
// ──────────────────────────────────────────────────────────────────────

fn draw_nav(ui: &mut egui::Ui, state: &mut PanelState, locale: Locale) {
    ui.add_space(4.0);
    for cat in Category::ALL {
        let selected = state.category == cat;
        let resp = w::nav_pill(ui, cat.title(locale), selected, |painter, icon_box, ink| {
            draw_category_icon(painter, cat, icon_box, ink);
        });
        if resp.clicked() {
            state.category = cat;
        }
        ui.add_space(4.0);
    }
    if let Some(err) = &state.save_error {
        ui.add_space(12.0);
        ui.label(
            RichText::new(err)
                .size(11.0)
                .color(Color32::from_rgb(0xf8, 0x88, 0x88)),
        );
    }
}

/// Line-drawn 24px category glyphs (the WebView uses Material SVG
/// icons; these painter approximations follow the same motifs).
fn draw_category_icon(painter: &egui::Painter, cat: Category, b: Rect, color: Color32) {
    let c = b.center();
    match cat {
        // Palette: outer ring + four paint wells.
        Category::UiAppearance => {
            painter.circle_stroke(c, 9.0, Stroke::new(1.6, color));
            for (dx, dy) in [(-4.0, -3.5), (-0.5, -5.5), (3.5, -3.5), (5.5, 0.5)] {
                painter.circle_filled(c + Vec2::new(dx, dy), 1.7, color);
            }
        }
        // Text-format: "A" beside shrinking text lines.
        Category::TerminalAppearance => {
            painter.text(
                egui::pos2(b.left() + 4.0, c.y),
                egui::Align2::CENTER_CENTER,
                "A",
                egui::FontId::proportional(15.0),
                color,
            );
            for (i, w) in [(0, 10.0), (1, 8.0), (2, 6.0)] {
                let y = b.top() + 6.0 + i as f32 * 6.0;
                painter.line_segment(
                    [
                        egui::pos2(b.left() + 10.0, y),
                        egui::pos2(b.left() + 10.0 + w, y),
                    ],
                    Stroke::new(1.6, color),
                );
            }
        }
        // Terminal prompt: window outline + "> " chevron.
        Category::TerminalBehavior => {
            painter.rect_stroke(b.shrink(2.0), Rounding::same(2.0), Stroke::new(1.6, color));
            let p = b.left_top() + Vec2::new(6.0, 9.0);
            painter.line_segment([p, p + Vec2::new(4.0, 3.5)], Stroke::new(1.8, color));
            painter.line_segment(
                [p + Vec2::new(4.0, 3.5), p + Vec2::new(0.0, 7.0)],
                Stroke::new(1.8, color),
            );
            painter.line_segment(
                [
                    egui::pos2(c.x + 1.0, b.bottom() - 6.0),
                    egui::pos2(b.right() - 5.0, b.bottom() - 6.0),
                ],
                Stroke::new(1.8, color),
            );
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Content column
// ──────────────────────────────────────────────────────────────────────

fn draw_content(ui: &mut egui::Ui, state: &mut PanelState, locale: Locale) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(CONTENT_PADDING);
            ui.horizontal(|ui| {
                ui.add_space(CONTENT_PADDING);
                ui.vertical(|ui| {
                    ui.set_max_width(w::ROW_MAX_WIDTH + 80.0);
                    // Section header — MD3 Title Large (22px).
                    ui.label(
                        RichText::new(state.category.title(locale))
                            .size(22.0)
                            .color(md3::on_surface()),
                    );
                    ui.add_space(16.0);
                    match state.category {
                        Category::UiAppearance => draw_ui_appearance(ui, state, locale),
                        Category::TerminalAppearance => draw_terminal_appearance(ui, state, locale),
                        Category::TerminalBehavior => draw_terminal_behavior(ui, state, locale),
                    }
                    ui.add_space(CONTENT_PADDING);
                });
            });
        });
}

// ──────────────────────────────────────────────────────────────────────
// Categories
// ──────────────────────────────────────────────────────────────────────

fn draw_ui_appearance(ui: &mut egui::Ui, state: &mut PanelState, locale: Locale) {
    use crate::settings::{Language, UiTheme, UiThemePreset};
    let d = &mut state.draft;
    let mut dirty = false;

    w::subsection(ui, t(locale, "Language", "言語"));
    dirty |= w::form_row(ui, t(locale, "Language", "言語"), |ui| {
        w::outlined_select(
            ui,
            "language",
            &mut d.language,
            &[
                (
                    Language::Auto,
                    t(locale, "Auto (system)", "自動（システム）"),
                ),
                (Language::En, "English"),
                (Language::Ja, "日本語"),
            ],
        )
    });

    w::subsection(ui, t(locale, "Theme", "テーマ"));
    dirty |= w::form_row(ui, t(locale, "UI theme", "UI テーマ"), |ui| {
        w::outlined_select(
            ui,
            "ui_theme",
            &mut d.ui_theme,
            &[
                (UiTheme::System, t(locale, "System", "システム")),
                (UiTheme::Light, t(locale, "Light", "ライト")),
                (UiTheme::Dark, t(locale, "Dark", "ダーク")),
            ],
        )
    });
    dirty |= w::form_row(
        ui,
        t(locale, "Theme preset", "テーマプリセット"),
        |ui| {
            w::outlined_select(
                ui,
                "ui_theme_preset",
                &mut d.ui_theme_preset,
                &[
                    (UiThemePreset::Purple, t(locale, "Purple", "パープル")),
                    (UiThemePreset::Blue, t(locale, "Blue", "ブルー")),
                    (UiThemePreset::Green, t(locale, "Green", "グリーン")),
                    (UiThemePreset::Orange, t(locale, "Orange", "オレンジ")),
                    (UiThemePreset::Pink, t(locale, "Pink", "ピンク")),
                ],
            )
        },
    );

    w::subsection(ui, t(locale, "UI Font", "UI フォント"));
    dirty |= w::form_row(
        ui,
        t(locale, "UI font family", "UI フォントファミリ"),
        |ui| w::outlined_text_input(ui, &mut d.ui_font_family, w::CONTROL_WIDTH),
    );

    state.dirty |= dirty;
}

fn draw_terminal_appearance(ui: &mut egui::Ui, state: &mut PanelState, locale: Locale) {
    use crate::settings::ScrollbarMode;
    let d = &mut state.draft;
    let mut dirty = false;

    w::subsection(ui, t(locale, "Font", "フォント"));
    dirty |= w::form_row(
        ui,
        t(locale, "Font size (pt)", "フォントサイズ (pt)"),
        |ui| {
            ui.add(
                egui::Slider::new(&mut d.font_size, 8.0..=32.0)
                    .step_by(0.5)
                    .fixed_decimals(1),
            )
            .changed()
        },
    );
    // The flat primary/secondary keys are modeled as fallback[0] / [1].
    if d.font_family_fallback.len() < 2 {
        d.font_family_fallback.resize(2, String::new());
    }
    dirty |= w::form_row(ui, t(locale, "Primary font", "主フォント"), |ui| {
        w::outlined_text_input(ui, &mut d.font_family_fallback[0], w::CONTROL_WIDTH)
    });
    dirty |= w::form_row(ui, t(locale, "Secondary font", "副フォント"), |ui| {
        w::outlined_text_input(ui, &mut d.font_family_fallback[1], w::CONTROL_WIDTH)
    });
    let mut emoji = d.emoji_font.clone().unwrap_or_default();
    dirty |= w::form_row(ui, t(locale, "Emoji font", "絵文字フォント"), |ui| {
        let changed = w::outlined_text_input(ui, &mut emoji, w::CONTROL_WIDTH);
        if changed {
            d.emoji_font = if emoji.trim().is_empty() {
                None
            } else {
                Some(emoji.clone())
            };
        }
        changed
    });

    w::subsection(ui, t(locale, "Color", "カラー"));
    dirty |= w::form_row(
        ui,
        t(locale, "Color scheme", "カラースキーム"),
        |ui| {
            let mut changed = false;
            let current = if d.terminal_color_scheme.is_empty() {
                t(locale, "(default)", "（デフォルト）").to_string()
            } else {
                d.terminal_color_scheme.clone()
            };
            w::outlined_select_frame(ui, |ui| {
                egui::ComboBox::from_id_salt("terminal_color_scheme")
                    .selected_text(RichText::new(current).size(14.0).color(md3::on_surface()))
                    .width(w::CONTROL_WIDTH - 24.0)
                    .show_ui(ui, |ui| {
                        w::apply_popup_visuals(ui);
                        if ui
                            .selectable_value(
                                &mut d.terminal_color_scheme,
                                String::new(),
                                t(locale, "(default)", "（デフォルト）"),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        for name in crate::render::theme::color_scheme_preset_names() {
                            if ui
                                .selectable_value(
                                    &mut d.terminal_color_scheme,
                                    name.to_string(),
                                    name,
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                        // Collect the clicked custom-scheme name first:
                        // `selectable_value` would need `&mut` on the field
                        // while `&d.custom_color_schemes` is still borrowed.
                        let mut picked: Option<String> = None;
                        for scheme in &d.custom_color_schemes {
                            if ui
                                .selectable_label(
                                    d.terminal_color_scheme == scheme.name,
                                    &scheme.name,
                                )
                                .clicked()
                            {
                                picked = Some(scheme.name.clone());
                            }
                        }
                        if let Some(name) = picked {
                            if d.terminal_color_scheme != name {
                                d.terminal_color_scheme = name;
                                changed = true;
                            }
                        }
                    });
            });
            changed
        },
    );
    dirty |= w::toggle_row(
        ui,
        t(
            locale,
            "Bold brightens ANSI colors",
            "太字で ANSI 色を明るくする",
        ),
        &mut d.bold_brightens_ansi_colors,
    );
    ui.add_space(w::ROW_MARGIN * 0.5);

    w::subsection(ui, t(locale, "Layout", "レイアウト"));
    dirty |= w::form_row(ui, t(locale, "Padding (px)", "パディング (px)"), |ui| {
        ui.add(egui::Slider::new(&mut d.padding, 0..=32)).changed()
    });
    dirty |= w::form_row(
        ui,
        t(locale, "Scrollback lines", "スクロールバック行数"),
        |ui| {
            ui.add(
                egui::DragValue::new(&mut d.scrollback_lines)
                    .range(0..=100_000)
                    .speed(100),
            )
            .changed()
        },
    );
    dirty |= w::form_row(ui, t(locale, "Scrollbar", "スクロールバー"), |ui| {
        w::outlined_select(
            ui,
            "show_scrollbar",
            &mut d.show_scrollbar,
            &[
                (ScrollbarMode::Auto, t(locale, "Auto", "自動")),
                (ScrollbarMode::Always, t(locale, "Always", "常に表示")),
                (ScrollbarMode::Never, t(locale, "Never", "表示しない")),
            ],
        )
    });
    w::hint(
        ui,
        t(
            locale,
            "Scrollback lines apply to new tabs.",
            "スクロールバック行数は新しいタブから適用されます。",
        ),
    );

    state.dirty |= dirty;
}

fn draw_terminal_behavior(ui: &mut egui::Ui, state: &mut PanelState, locale: Locale) {
    use crate::settings::{BellAction, CursorStyle};
    let mut dirty = false;

    {
        let d = &mut state.draft;
        w::subsection(ui, t(locale, "Cursor", "カーソル"));
        dirty |= w::form_row(ui, t(locale, "Cursor style", "カーソル形状"), |ui| {
            w::outlined_select(
                ui,
                "cursor_style",
                &mut d.cursor_style,
                &[
                    (CursorStyle::Block, t(locale, "Block", "ブロック")),
                    (CursorStyle::Underline, t(locale, "Underline", "下線")),
                    (CursorStyle::Bar, t(locale, "Bar", "バー")),
                ],
            )
        });
        dirty |= w::toggle_row(
            ui,
            t(locale, "Cursor blink", "カーソル点滅"),
            &mut d.cursor_blink,
        );
        ui.add_space(w::ROW_MARGIN * 0.5);

        w::subsection(ui, t(locale, "Shell", "シェル"));
        dirty |= w::form_row(ui, t(locale, "Shell path", "シェルパス"), |ui| {
            w::outlined_text_input(ui, &mut d.shell_path, w::CONTROL_WIDTH)
        });
    }
    dirty |= w::form_row(
        ui,
        t(
            locale,
            "Shell args (comma-separated)",
            "シェル引数（カンマ区切り）",
        ),
        |ui| w::outlined_text_input(ui, &mut state.shell_args_text, w::CONTROL_WIDTH),
    );
    w::hint(
        ui,
        t(
            locale,
            "Shell settings apply to new tabs.",
            "シェル設定は新しいタブから適用されます。",
        ),
    );
    ui.add_space(w::ROW_MARGIN * 0.5);

    let d = &mut state.draft;
    w::subsection(ui, t(locale, "Behavior", "動作"));
    dirty |= w::form_row(
        ui,
        t(locale, "Scroll speed", "スクロール速度"),
        |ui| {
            ui.add(egui::Slider::new(&mut d.scroll_speed, 1..=10))
                .changed()
        },
    );
    dirty |= w::form_row(ui, t(locale, "Bell action", "ベル動作"), |ui| {
        w::outlined_select(
            ui,
            "bell_action",
            &mut d.bell_action,
            &[
                (
                    BellAction::Visual,
                    t(locale, "Visual flash", "画面フラッシュ"),
                ),
                (BellAction::Sound, t(locale, "Sound", "サウンド")),
                (BellAction::None, t(locale, "None", "なし")),
            ],
        )
    });
    dirty |= w::form_row(
        ui,
        t(locale, "Editor command", "エディタコマンド"),
        |ui| w::outlined_text_input(ui, &mut d.editor_command, w::CONTROL_WIDTH),
    );
    dirty |= w::toggle_row(
        ui,
        t(locale, "URL detection", "URL 検出"),
        &mut d.url_detection,
    );
    dirty |= w::toggle_row(
        ui,
        t(locale, "File path detection", "ファイルパス検出"),
        &mut d.file_path_detection,
    );
    dirty |= w::toggle_row(
        ui,
        t(locale, "Copy on select", "選択時にコピー"),
        &mut d.copy_on_select,
    );
    dirty |= w::toggle_row(
        ui,
        t(locale, "Middle-click paste", "中クリックペースト"),
        &mut d.middle_click_paste,
    );
    dirty |= w::toggle_row(
        ui,
        t(
            locale,
            "Shift+Enter as Alt+Enter",
            "Shift+Enter を Alt+Enter として送信",
        ),
        &mut d.shift_enter_as_alt_enter,
    );
    dirty |= w::toggle_row(ui, t(locale, "SKK mode", "SKK モード"), &mut d.skk_mode);
    dirty |= w::toggle_row(
        ui,
        t(locale, "Output folding", "出力の折りたたみ"),
        &mut d.fold_enabled,
    );
    ui.add_space(w::ROW_MARGIN * 0.5);

    w::subsection(ui, t(locale, "Clipboard", "クリップボード"));
    dirty |= w::toggle_row(
        ui,
        t(
            locale,
            "Allow OSC 52 clipboard read",
            "OSC 52 クリップボード読取を許可",
        ),
        &mut d.clipboard_read_osc52,
    );
    let mut osc52_mb = (d.clipboard_max_size_osc52 / (1024 * 1024)).max(1);
    dirty |= w::form_row(
        ui,
        t(locale, "OSC 52 max size (MiB)", "OSC 52 最大サイズ (MiB)"),
        |ui| {
            let changed = ui.add(egui::Slider::new(&mut osc52_mb, 1..=50)).changed();
            if changed {
                d.clipboard_max_size_osc52 = osc52_mb * 1024 * 1024;
            }
            changed
        },
    );

    state.dirty |= dirty;
}
