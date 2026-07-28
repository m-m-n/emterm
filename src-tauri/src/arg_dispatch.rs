//! Top-level argument classification for the `emterm` binary entry point
//! (SPEC `feature-docs/unknown-flag-usage/SPEC.md`, FR1-FR6).
//!
//! [`classify`] is a pure function over the argument list (excluding the
//! program name), reached only after `src-tauri/src/main.rs` has already
//! declined to dispatch a bare-word subcommand (`markdown` / `json` / `yaml`
//! / `html` / `image` / `agent-status` / `mux`). It performs no I/O, no
//! logging and no process exit — `main.rs` maps its [`Classification`]
//! result onto the actual stdout / stderr / exit-code side effects (D1:
//! decision logic lives in the library, where `cargo test --lib` reaches
//! it; the binary target has no test harness of its own — the same
//! precedent as `emterm::backend_select`).
//!
//! [`RECOGNIZED_FLAGS`] is the single source of truth for which flags this
//! build accepts (D2 / NFR3): [`classify`] reads it to decide what is
//! recognized, and `main.rs`'s `run_gui` dispatches by iterating it too, so
//! the two can never drift out of the same set — there is only one list.
//! Not every entry has a child window to open: [`RecognizedFlag::target`]
//! (on the `gui` build) is `Option<GuiTarget>`, and `run_gui` skips entries
//! whose target is `None` (e.g. `--version`) rather than dispatching them.

/// Outcome of classifying the top-level argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// `--help` / `-h` was present anywhere in the list (D3: wins over an
    /// unrecognized argument seen earlier or later).
    Help,
    /// A `-`-leading argument was neither a recognized flag nor the value
    /// consumed by one. Carries the first such argument, verbatim, in
    /// left-to-right order (FR1).
    Unknown(String),
    /// Nothing was rejected; continue into the existing dispatch path.
    Proceed,
}

/// Which child-window entry point a recognized GUI flag dispatches to.
/// `main.rs`'s `run_gui` matches on this instead of hardcoding a second
/// list of flag names, so the flag-to-handler mapping exists in exactly one
/// place (D2 / NFR3).
#[cfg(feature = "gui")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiTarget {
    Viewer,
    ImageViewer,
    DataViewer,
    HtmlViewer,
    Settings,
}

/// A recognized top-level flag: its name, and whether it consumes the
/// immediately following argument as a value (D4 — that value is never
/// itself classified, even when it starts with `-`).
pub struct RecognizedFlag {
    pub name: &'static str,
    pub takes_value: bool,
    /// Which child window this flag opens, or `None` if it has no dispatch
    /// target (e.g. `--version`). Only meaningful on the `gui` build, where
    /// `run_gui` reads it to dispatch and skips entries with no target.
    #[cfg(feature = "gui")]
    pub target: Option<GuiTarget>,
}

/// The flags this build recognizes (FR3). GUI build: the five child-window
/// flags plus `--version` (which has no dispatch target). CLI-only build
/// (`--no-default-features`): just `--version`.
#[cfg(feature = "gui")]
pub const RECOGNIZED_FLAGS: &[RecognizedFlag] = &[
    RecognizedFlag {
        name: "--viewer",
        takes_value: true,
        target: Some(GuiTarget::Viewer),
    },
    RecognizedFlag {
        name: "--image-viewer",
        takes_value: true,
        target: Some(GuiTarget::ImageViewer),
    },
    RecognizedFlag {
        name: "--data-viewer",
        takes_value: true,
        target: Some(GuiTarget::DataViewer),
    },
    RecognizedFlag {
        name: "--html-viewer",
        takes_value: true,
        target: Some(GuiTarget::HtmlViewer),
    },
    RecognizedFlag {
        name: "--settings",
        takes_value: false,
        target: Some(GuiTarget::Settings),
    },
    RecognizedFlag {
        name: "--version",
        takes_value: false,
        target: None,
    },
];

/// The flags this build recognizes (FR3). CLI-only build: just `--version`
/// — the child-window flags do not exist without the `gui` feature.
#[cfg(not(feature = "gui"))]
pub const RECOGNIZED_FLAGS: &[RecognizedFlag] = &[RecognizedFlag {
    name: "--version",
    takes_value: false,
}];

const HELP_FLAGS: &[&str] = &["--help", "-h"];

/// Classify the top-level argument list (excluding the program name).
///
/// Scans left to right (see IMPLEMENTATION.md "Classification flow"):
/// 1. `--help` / `-h` anywhere short-circuits to [`Classification::Help`]
///    (D3), regardless of any unrecognized argument already seen.
/// 2. A recognized value-taking flag consumes the following argument
///    unconditionally (D4) — that value is never classified.
/// 3. A recognized valueless flag is skipped.
/// 4. A `-`-leading argument that matched none of the above is remembered
///    as the candidate unrecognized argument, but only if no candidate has
///    been remembered yet (FR1: left-most wins) — scanning continues so a
///    later `--help` can still win.
/// 5. Anything else (a non-`-` argument) is ignored.
///
/// After the scan: [`Classification::Unknown`] carrying the remembered
/// candidate if there is one, otherwise [`Classification::Proceed`].
pub fn classify(args: &[String]) -> Classification {
    let mut candidate: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();

        if HELP_FLAGS.contains(&arg) {
            return Classification::Help;
        }

        if let Some(flag) = RECOGNIZED_FLAGS.iter().find(|f| f.name == arg) {
            i += if flag.takes_value { 2 } else { 1 };
            continue;
        }

        if candidate.is_none() && arg.starts_with('-') {
            candidate = Some(arg.to_string());
        }
        i += 1;
    }

    match candidate {
        Some(arg) => Classification::Unknown(arg),
        None => Classification::Proceed,
    }
}

/// Build-appropriate usage text (FR4). Lists the bare-word subcommands and,
/// on the `gui` build, the recognized child-window flags plus `-h, --help`.
/// Shared verbatim by all three call sites in `main.rs`: `--help` (stdout),
/// an unrecognized argument (stderr), and the CLI-only fallthrough (stderr)
/// — so the `Run \`emterm <subcommand> --help\` for details.` guidance line
/// exists in exactly one place.
#[cfg(feature = "gui")]
pub fn usage_text() -> String {
    "Usage: emterm [options]\n\
     \x20      emterm <markdown|json|yaml|html|image> <file> [options]\n\
     \x20      emterm agent-status <idle|working|blocked|done|clear> [--name <n>]\n\
     \x20      emterm mux <args>...\n\
     \n\
     Options:\n\
     \x20 --viewer <path>        Open the Markdown viewer window\n\
     \x20 --image-viewer <path>  Open the image viewer window\n\
     \x20 --data-viewer <path>   Open the JSON/YAML data viewer window\n\
     \x20 --html-viewer <path>   Open the HTML viewer window\n\
     \x20 --settings             Open the settings window\n\
     \x20 --version              Print version and exit\n\
     \x20 -h, --help             Print this help\n\
     \n\
     Run `emterm <subcommand> --help` for details."
        .to_string()
}

/// Build-appropriate usage text (FR4), CLI-only build.
#[cfg(not(feature = "gui"))]
pub fn usage_text() -> String {
    "emterm: this build provides only CLI subcommands.\n\
     Usage: emterm <markdown|json|yaml|html|image> <file> [options]\n\
     \x20      emterm agent-status <idle|working|blocked|done|clear> [--name <n>]\n\
     \x20      emterm mux <args>...\n\
     \n\
     Options:\n\
     \x20 --version              Print version and exit\n\
     \x20 -h, --help             Print this help\n\
     \n\
     Run `emterm <subcommand> --help` for details."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // AC-1 / TS-1: `--help` and `-h` alone both yield Help.
    #[test]
    fn help_flags_yield_help() {
        assert_eq!(classify(&v(&["--help"])), Classification::Help);
        assert_eq!(classify(&v(&["-h"])), Classification::Help);
    }

    // AC-2 / TS-2: an unrecognized flag carries the argument verbatim.
    #[test]
    fn unrecognized_flag_carries_the_argument() {
        assert_eq!(
            classify(&v(&["--typo"])),
            Classification::Unknown("--typo".to_string())
        );
    }

    // AC-3 / TS-3: an empty argument list proceeds.
    #[test]
    fn empty_args_proceed() {
        assert_eq!(classify(&[]), Classification::Proceed);
    }

    // AC-5 / TS-7: help wins over an unrecognized argument in either order.
    #[test]
    fn help_wins_over_unrecognized_in_either_order() {
        assert_eq!(
            classify(&v(&["--typo", "--help"])),
            Classification::Help,
            "unrecognized then help"
        );
        assert_eq!(
            classify(&v(&["--help", "--typo"])),
            Classification::Help,
            "help then unrecognized"
        );
    }

    // AC-5 / TS-9: two unrecognized flags report the left-most one.
    #[test]
    fn two_unrecognized_flags_report_the_leftmost() {
        assert_eq!(
            classify(&v(&["--a", "--b"])),
            Classification::Unknown("--a".to_string())
        );
    }

    // AC-6 / TS-8: `-` alone and `--` alone are both Unknown.
    #[test]
    fn lone_dash_and_double_dash_are_unrecognized() {
        assert_eq!(
            classify(&v(&["-"])),
            Classification::Unknown("-".to_string())
        );
        assert_eq!(
            classify(&v(&["--"])),
            Classification::Unknown("--".to_string())
        );
    }

    // AC-9 / SPEC Error Handling table: usage text carries the guidance
    // line on every build, so all three call sites in `main.rs` share it.
    #[test]
    fn usage_text_carries_the_subcommand_help_guidance_line() {
        assert!(usage_text().contains("Run `emterm <subcommand> --help` for details."));
    }

    // version-flag-classification AC-4 / AC-5: the Options block lists
    // `--version` on both builds (each build compiles only its own
    // `usage_text()`, so this single test covers whichever is active).
    #[test]
    fn usage_text_lists_the_version_flag() {
        assert!(usage_text().contains("--version"));
    }

    // version-flag-classification AC-3: `--version` does not consume the
    // following argument — an unrecognized flag right after it is still
    // reported. Runs on both builds: `--version` is in both tables and
    // `--typo` is recognized by neither.
    #[test]
    fn version_flag_does_not_consume_the_following_argument() {
        assert_eq!(
            classify(&v(&["--version", "--typo"])),
            Classification::Unknown("--typo".to_string())
        );
    }

    // version-flag-classification AC-8: help still wins over `--version`
    // appearing anywhere in the argument list.
    #[test]
    fn help_wins_over_version_flag() {
        assert_eq!(
            classify(&v(&["--version", "--help"])),
            Classification::Help,
            "version then help"
        );
        assert_eq!(
            classify(&v(&["--help", "--version"])),
            Classification::Help,
            "help then version"
        );
    }

    #[cfg(feature = "gui")]
    mod gui {
        use super::*;

        // AC-4 / TS-4, TS-6: each recognized GUI flag proceeds.
        #[test]
        fn recognized_gui_flags_proceed() {
            assert_eq!(
                classify(&v(&["--viewer", "/tmp/p"])),
                Classification::Proceed
            );
            assert_eq!(
                classify(&v(&["--image-viewer", "/tmp/p"])),
                Classification::Proceed
            );
            assert_eq!(
                classify(&v(&["--data-viewer", "/tmp/p"])),
                Classification::Proceed
            );
            assert_eq!(
                classify(&v(&["--html-viewer", "/tmp/p"])),
                Classification::Proceed
            );
            assert_eq!(classify(&v(&["--settings"])), Classification::Proceed);
        }

        // AC-4 / TS-5: a value beginning with a dash is consumed, not
        // classified (D4).
        #[test]
        fn value_of_a_recognized_flag_is_never_classified() {
            assert_eq!(
                classify(&v(&["--viewer", "--weird"])),
                Classification::Proceed
            );
        }

        // version-flag-classification Test Notes "Edge case": a literal
        // `--version` supplied as the *value* of a value-taking flag must
        // still be consumed as that flag's value, never classified on its
        // own account.
        #[test]
        fn version_as_a_flags_value_is_consumed_not_classified() {
            assert_eq!(
                classify(&v(&["--viewer", "--version"])),
                Classification::Proceed
            );
        }

        // version-flag-classification AC-1: `--version` proceeds alone,
        // and after another recognized flag such as `--settings`.
        #[test]
        fn version_flag_proceeds_alone_and_after_another_recognized_flag() {
            assert_eq!(classify(&v(&["--version"])), Classification::Proceed);
            assert_eq!(
                classify(&v(&["--settings", "--version"])),
                Classification::Proceed
            );
        }

        // version-flag-classification AC-7: `--version` declares no
        // child-window dispatch target, so a future edit that gives it one
        // fails this test.
        #[test]
        fn version_entry_declares_no_dispatch_target() {
            let version = RECOGNIZED_FLAGS
                .iter()
                .find(|f| f.name == "--version")
                .expect("--version entry present in RECOGNIZED_FLAGS");
            assert_eq!(version.target, None);
        }

        // AC-8: the recognized-flag table is the only definition
        // `run_gui` dispatches from (it iterates `RECOGNIZED_FLAGS`
        // directly rather than hardcoding a second flag list), so this
        // test pins the table's contents — the same set `run_gui` accepts
        // by construction, not by convention. Changing the table without
        // updating `run_gui`'s handlers (or vice versa) is now
        // structurally impossible; this test fails if the table's shape
        // itself changes unexpectedly.
        //
        // version-flag-classification AC-6: updated to the new expected
        // contents (six entries, `--version` included) rather than loosened
        // to a containment check.
        #[test]
        fn recognized_flag_table_matches_the_six_gui_flags_including_version() {
            let names: Vec<&str> = RECOGNIZED_FLAGS.iter().map(|f| f.name).collect();
            assert_eq!(
                names,
                vec![
                    "--viewer",
                    "--image-viewer",
                    "--data-viewer",
                    "--html-viewer",
                    "--settings",
                    "--version",
                ]
            );
            let value_taking: Vec<&str> = RECOGNIZED_FLAGS
                .iter()
                .filter(|f| f.takes_value)
                .map(|f| f.name)
                .collect();
            assert_eq!(
                value_taking,
                vec!["--viewer", "--image-viewer", "--data-viewer", "--html-viewer"],
                "only --settings and --version should be valueless"
            );
        }
    }

    #[cfg(not(feature = "gui"))]
    mod cli_only {
        use super::*;

        // AC-7 / TS-10: on the CLI-only build, `--settings` is not
        // recognized (the recognized-flag table holds only `--version`).
        #[test]
        fn settings_flag_is_unrecognized_without_gui() {
            assert_eq!(
                classify(&v(&["--settings"])),
                Classification::Unknown("--settings".to_string())
            );
        }

        // version-flag-classification AC-2: `--version` alone proceeds on
        // the CLI-only build.
        #[test]
        fn version_flag_proceeds_without_gui() {
            assert_eq!(classify(&v(&["--version"])), Classification::Proceed);
        }

        // AC-8 (CLI-only side) / version-flag-classification AC-6: the
        // table holds exactly `--version` now that this build's flag list
        // is no longer empty, matching the fact that `run_gui` (and the
        // GUI-only flags) do not exist in this build. Updated to the new
        // expected contents rather than loosened to a containment check.
        #[test]
        fn recognized_flag_table_contains_only_version_without_gui() {
            let names: Vec<&str> = RECOGNIZED_FLAGS.iter().map(|f| f.name).collect();
            assert_eq!(names, vec!["--version"]);
            assert!(
                !RECOGNIZED_FLAGS[0].takes_value,
                "--version should be valueless"
            );
        }
    }
}
