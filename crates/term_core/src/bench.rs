//! Lightweight bench harness for SlimCell scrollback. Gated behind
//! `#[cfg(test)]` and only runs as part of `cargo test --release`-style
//! invocations when the `slim_cell_bench` filter is supplied (since they
//! print measurements rather than asserting tight thresholds).
//!
//! The benches use `std::time::Instant`. They run on the host target
//! (x86 / aarch64), not on `wasm32-unknown-unknown`. Numbers here are
//! representative; production WASM will be different but trends carry
//! across.
//!
//! Usage:
//!     cargo test --lib --release slim_cell_bench -- --nocapture --include-ignored

#![allow(dead_code)]

use std::time::Instant;

use crate::cell::{Cell, PackedColor};
use crate::char_table::CharTable;
use crate::slim_cell::{SlimCell, cell_to_slim, slim_to_cell};
use crate::style_table::StyleTable;

#[cfg(test)]
mod benches;
