//! Inline-subset HTML parser + sanitizer shared by the native status
//! bar and (in a follow-up phase) the native Markdown viewer.
//!
//! See `doc/tasks/status-bar-native-port/IMPLEMENTATION.md` Phase A
//! for the design notes — in particular the lenient mode contract
//! and the `Node` extensibility plan.

pub mod parser;
pub mod rich_text;
pub mod sanitizer;
pub mod tokenizer;

pub use parser::{CssColor, parse};
pub use rich_text::{RichTextRun, to_rich_text_runs};
pub use sanitizer::strip_html_tags;

// Facade re-exports consumed by the status-bar layer (template engine's
// `<font>` sanitizer) and reserved for the future Markdown viewer. Kept
// at the top-level module so downstream callers don't depend on the
// internal submodule layout — the shared foundation stays free to
// reorganize. `#[allow(unused_imports)]` keeps the bin target
// warning-clean when a given consumer build doesn't touch all of them.
#[allow(unused_imports)]
pub use parser::{Node, parse_css_color};
#[allow(unused_imports)]
pub use tokenizer::{Token, tokenize};
