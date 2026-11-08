//! mux-strip-concat-budget-overrun task0002 (FR3, NFR1, NFR2, NFR4, TM-2):
//! the scaling-ratio regression guard on the real write-path strip and the real
//! one-call write-filter feed.
//!
//! `strip_concat_alternations_and_chains_finish_within_the_budget` binds the
//! total thread CPU time of a fixed workload. It cannot tell a path that is
//! still linear from one that has started to grow faster than its input. These
//! tests run the same two paths at an input of N units and at one of 4N units
//! and judge the two thread CPU times with [`judge_scaling`]: the path is linear
//! when the time at 4N is strictly below 8 times the time at N.
//!
//! The unit is built as the target test builds its alternation: a lead followed
//! by a strip target. One unit holds, for every `TARGETS` entry, the open-CSI
//! lead (`ESC [ 6`) followed by that target and the lone-`ESC` lead followed by
//! that target. The expected output of each part is its lead, `CSI_CLOSING` and
//! the target's re-emitted C0 bytes; the expected output of an input is the
//! concatenation of its parts' expected outputs.
//!
//! Sizing (NFR1): a unit is a few hundred bytes. N units make each smaller
//! measurement about 100 ms or more of thread CPU time on a debug build, so the
//! coarser Windows thread-times clock does not dominate the ratio, and make the N
//! input longer than `SCROLLBACK_FILTER_PENDING_CAP`, so both sizes take the
//! overflow flush in the one-call feed. Each size is measured once per path;
//! the guard reads no wall clock, adds no budget and spawns no thread (NFR2).
//!
//! Each test writes its two times and the ratio to the test output, so the
//! sizing is read from one single-threaded debug run (`-- --nocapture`); a
//! failure message shows the same verdict.
//!
//! The guard exercises the production paths as they are. It passes as soon as
//! it is written when their cost is linear; the proof that the same judgment
//! rejects a quadratic cost is the surrogate case of `thread_cpu_time`
//! (`the_scaling_judgment_rejects_a_quadratic_surrogate_and_accepts_a_linear_one`).

use super::round3_write_path::DIMS;
use super::strip_concat_query::TARGETS;
use super::thread_cpu_time::{CpuMeter, judge_scaling};
use super::*;
use crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write;

const ESC: u8 = 0x1b;

/// The open CSI lead of a unit part.
const OPEN_CSI_LEAD: &[u8] = b"\x1b[6";

/// The units of the smaller input (N). The larger input (4N) is four times it.
const UNITS_AT_N: usize = 24_000;

/// An input of `units` units and the bytes the write-path strip and the write
/// filter write for it. Built outside every measured call.
struct ScalingInput {
    input: Vec<u8>,
    expected: Vec<u8>,
}

/// One unit: for every target, the open-CSI lead and the lone-`ESC` lead each
/// followed by the target. Returns the unit's input bytes and its expected
/// output bytes.
fn unit() -> (Vec<u8>, Vec<u8>) {
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for target in TARGETS {
        for lead in [OPEN_CSI_LEAD, &[ESC][..]] {
            input.extend_from_slice(lead);
            input.extend_from_slice(target.bytes);
            expected.extend_from_slice(lead);
            expected.extend_from_slice(CSI_CLOSING);
            expected.extend_from_slice(target.c0);
        }
    }
    (input, expected)
}

fn scaling_input(units: usize) -> ScalingInput {
    let (unit_input, unit_expected) = unit();
    ScalingInput {
        input: unit_input.repeat(units),
        expected: unit_expected.repeat(units),
    }
}

// AC-3 (FR3, NFR1, TM-2): the write-path strip.
#[test]
fn the_write_path_strip_cost_scales_linearly_from_n_to_4n() {
    let at_n_input = scaling_input(UNITS_AT_N);
    let at_4n_input = scaling_input(4 * UNITS_AT_N);

    let mut at_n = CpuMeter::new();
    let at_n_output = at_n.measure(|| strip_pty_output_for_scrollback_write(&at_n_input.input));
    let mut at_4n = CpuMeter::new();
    let at_4n_output = at_4n.measure(|| strip_pty_output_for_scrollback_write(&at_4n_input.input));

    let verdict = judge_scaling(at_n.total(), at_4n.total());
    println!(
        "strip_pty_output_for_scrollback_write, {} bytes at N and {} bytes at 4N: {verdict}",
        at_n_input.input.len(),
        at_4n_input.input.len()
    );
    assert!(
        at_n_output == at_n_input.expected,
        "the strip's output at N is not the expected bytes"
    );
    assert!(
        at_4n_output == at_4n_input.expected,
        "the strip's output at 4N is not the expected bytes"
    );
    assert!(
        verdict.is_linear(),
        "the write-path strip is not linear from N to 4N: {verdict}"
    );
}

// AC-4 (FR3, NFR1, TM-2): the write filter fed the whole input in one call.
#[test]
fn the_one_call_write_filter_feed_cost_scales_linearly_from_n_to_4n() {
    let at_n_input = scaling_input(UNITS_AT_N);
    let at_4n_input = scaling_input(4 * UNITS_AT_N);
    // Both sizes take the overflow flush of the one-call feed.
    assert!(
        at_n_input.input.len() > SCROLLBACK_FILTER_PENDING_CAP,
        "the input at N ({} bytes) does not exceed the pending cap ({} bytes)",
        at_n_input.input.len(),
        SCROLLBACK_FILTER_PENDING_CAP
    );

    let mut filter_at_n = ScrollbackWriteFilter::new();
    let mut at_n = CpuMeter::new();
    let at_n_output = at_n.measure(|| filter_at_n.feed(&at_n_input.input, DIMS)).1;
    let mut filter_at_4n = ScrollbackWriteFilter::new();
    let mut at_4n = CpuMeter::new();
    let at_4n_output = at_4n
        .measure(|| filter_at_4n.feed(&at_4n_input.input, DIMS))
        .1;

    let verdict = judge_scaling(at_n.total(), at_4n.total());
    println!(
        "ScrollbackWriteFilter::feed in one call, {} bytes at N and {} bytes at 4N: {verdict}",
        at_n_input.input.len(),
        at_4n_input.input.len()
    );
    assert!(
        at_n_output == at_n_input.expected,
        "the filter's output at N is not the expected bytes"
    );
    assert!(
        at_4n_output == at_4n_input.expected,
        "the filter's output at 4N is not the expected bytes"
    );
    assert_eq!(
        filter_at_n.csi_phase(),
        None,
        "the filter holds an open CSI after the input at N"
    );
    assert_eq!(
        filter_at_4n.csi_phase(),
        None,
        "the filter holds an open CSI after the input at 4N"
    );
    assert!(
        verdict.is_linear(),
        "the one-call write-filter feed is not linear from N to 4N: {verdict}"
    );
}
