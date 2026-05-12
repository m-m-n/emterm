//! `term_images` — Kitty Graphics Protocol + SIXEL decoders + APC/DCS parsers.
//!
//! This crate is shared by the legacy Tauri build (`src-tauri/`) and the
//! native-poc build (`native-poc/`). It is intentionally `tauri`-free so that
//! native-poc can pull it in without dragging the WebView runtime along.
//!
//! The top-level layout mirrors what used to live in `src-tauri/src/`:
//!
//! - [`image_proc`]: the former `src-tauri/src/image/` module, renamed so the
//!   directory does not shadow the external `image` crate (note: this crate
//!   currently does not depend on `image`, but the rename keeps the door
//!   open for future use).
//! - [`ansi`]: the former `src-tauri/src/ansi/` module containing the APC and
//!   DCS parsers required by [`image_proc`].

pub mod ansi;
pub mod image_proc;
