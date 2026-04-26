# Implementation Plan: WASM Slim Cell

## Overview
Reduce per-cell memory in the WASM scrollback region from 34 bytes to 8 bytes by introducing a `SlimCell` that references styles via a `StyleTable` (u16 ID, intern + refcount + free_list) and large graphemes via a `CharTable` (u32 ID, intern + refcount). The active viewport keeps the existing `Cell`; only rows evicted from the viewport are compressed. WASM public API and packed binary format remain unchanged.

## Objectives
- Achieve `size_of::<SlimCell>() == 8` (FR1, NFR1).
- Reduce 10,000 × 200 scrollback total memory by at least 50% versus the current `Cell`-based ring (NFR1).
- Preserve all WASM public API entry points and the JS-facing packed binary format (NFR6).
- Keep p99 scroll-render regression within 5% and reflow latency within 2× (NFR2, NFR5).
- Use safe Rust only; no new `unsafe` blocks (NFR7).
- All existing wasm tests continue to pass; add new unit tests for slim/style/char/refcount/reflow/saturation (NFR8).

## Prerequisites

### Development Environment
- Rust toolchain with `wasm32-unknown-unknown` target (already configured).
- Docker + docker-compose for tests and E2E (`docker-compose.e2e.yml`).
- `bun` for frontend toolchain (not directly required by this task but kept for end-to-end builds).

### Dependencies
- No new external crates. `std::collections::HashMap`, `serde`, `bincode`, `wasm_bindgen`, `web_sys` are already in use.
- Internal: `wasm/src/cell.rs`, `ring_buffer.rs`, `terminal_core.rs`, `reflow.rs`, `terminal_cells.rs`, `terminal_rows.rs`, `snapshot.rs` (all exist).

## Architecture Overview

### Technology Stack
- **Language**: Rust (compiled to WASM `wasm32-unknown-unknown`).
- **Framework**: `wasm-bindgen` for JS interop.
- **Key Libraries**:
  - `serde` + `bincode` — snapshot serialization.
  - `web_sys` — `console::warn_1` for saturation warnings.
- **Test runner**: `cargo test` inside Docker (`docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && cargo test"`).

### Design Approach
- **Hybrid storage per ring line**: Each ring slot is tagged as either `Viewport(Vec<Cell>)` or `Scrollback(Vec<SlimCell>)`. The viewport is always materialized as `Cell`; lines are compressed to `SlimCell` exactly when they cross the viewport→scrollback boundary, and decompressed on read.
- **Intern tables**: `StyleTable` (u16 ID) for the (fg, bg, flags, underline_*, hyperlink_id) tuple; `CharTable` (u32 ID) for graphemes that do not fit inline (>4 bytes UTF-8). Both use `HashMap` dedup + parallel refcount Vec + free_list Vec.
- **Refcount discipline**: On every scrollback write that overwrites an existing `SlimCell`, decrement the old cell's `style_id` (and `char_ref` if CharTable mode) and increment the new one's. On reflow, perform a full table rebuild from the ring to guarantee invariants.
- **Read path**: Render / copy / search / packed-row generation call `slim_to_cell(&SlimCell, &StyleTable, &CharTable) -> Cell` and continue with existing logic on the materialized `Cell`. Refcounts are not modified by reads.
- **Snapshot**: New format V2 with separate viewport and scrollback sections plus serialized tables. V1 snapshots load as viewport-only (scrollback discarded — accepted degradation).

### Component Interaction
1. PTY data → existing parser → `set_cell` writes into the viewport row (still `Cell`).
2. `ring_push_blank` advances ring; the row leaving the viewport is compressed (per-cell `cell_to_slim`) into a new `Scrollback(Vec<SlimCell>)` slot. The previous content of that ring slot (if it was already a scrollback row) has its refcounts decremented before the new row is interned.
3. Read paths (`get_scrollback_row_packed`, `get_scrollback_text`, reflow, copy/search helpers) decompress per-cell on demand. The materialized `Cell` is returned by value (cheap stack copy, no allocation).
4. `resize_reflow` decompresses scrollback rows into temporary `Vec<Cell>`, runs existing reflow on the combined viewport + scrollback `Cell` set, then re-allocates fresh `StyleTable` / `CharTable` and re-compresses all scrollback rows from scratch.
5. `wasm_debug_slim_stats()` exposes counters to JS for verification.

## Implementation Phases

---

### Phase 1: SlimCell + intern tables (foundations)

**Goal**: Define `SlimCell`, `StyleTable`, `CharTable`, and the bidirectional compression bridge as standalone modules with full unit-test coverage. No integration into the ring yet.

**Files to Create**:
- `wasm/src/slim_cell.rs` — `SlimCell` struct, flag constants, `cell_to_slim` and `slim_to_cell` bridges, size assertion.
- `wasm/src/style_table.rs` — `StyleEntry`, `StyleTable` with intern / get / inc_ref / dec_ref / saturation handling.
- `wasm/src/char_table.rs` — `CharTable` with intern / get / inc_ref / dec_ref over `&str`.

**Files to Modify**:
- `wasm/src/lib.rs` — register new modules.
- `wasm/src/cell.rs` — no struct changes; export `StyleEntry` only if needed.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `SlimCell` | 8-byte cell record holding style ID, char reference, width, internal flags | None | Layout fixed: char_ref u32, width u8, flags u8, style_id u16 |
| `SLIM_FLAG_*` | Bit constants for flags byte: `INLINE_ASCII`, `CHAR_TABLE`, `WIDE_CONT` | None | Mutually consistent: at most one of INLINE_ASCII / CHAR_TABLE set |
| `StyleEntry` | Hashable record of style attributes (fg, bg, flags, underline_style/color, hyperlink_id) | All component fields populated | Deterministic equality + hash |
| `StyleTable` | Intern style entries → u16 ID; reserve ID 0 for default; manage refcount and free_list; handle saturation | `new()` initialises ID 0 with refcount = max | `intern` returns stable ID for equal entries; `dec_ref(0)` is no-op; saturation falls back to ID 0 with rate-limited warn |
| `CharTable` | Intern grapheme strings → u32 ID; manage refcount and free_list | `new()` empty | `intern` returns stable ID for equal strings; `dec_ref` to 0 frees slot |
| `cell_to_slim` | Map a `Cell` (+ optional overflow string passed by caller) into a `SlimCell` while interning style and char | Caller provides the overflow string when `cell.is_overflow()` | Returns `SlimCell`; intern side-effects on tables |
| `slim_to_cell` | Materialize a `Cell` from `SlimCell` + tables; does not modify refcounts | `slim.style_id` valid; `char_ref` valid for current flags | Returns `Cell` whose visible attributes equal the original |

**Processing Flow (cell_to_slim)**:
1. Build `StyleEntry` from `Cell` → call `StyleTable::intern` → obtain `style_id`.
2. Inspect cell character bytes:
   - Length ≤ 4 and not overflow → pack bytes into `char_ref` little-endian, set `INLINE_ASCII` flag.
   - Length 5..=16 (inline) → call `CharTable::intern(&str)` → store ID in `char_ref`, set `CHAR_TABLE` flag.
   - Overflow (`char_len == 0xFF`) → caller-supplied overflow string is interned → `CHAR_TABLE` flag.
3. If `cell.width == 0` (right-half continuation marker) → set `WIDE_CONT` flag and skip char interning.
4. Compose `SlimCell { char_ref, width, flags, style_id }`.

**Processing Flow (slim_to_cell)**:
1. Look up `StyleEntry` via `StyleTable::get(style_id)` (with bounds check → fallback to default style on miss).
2. Initialize `Cell::EMPTY`, copy style fields.
3. Branch on flags:
   - `INLINE_ASCII` → unpack `char_ref` bytes into `char_data`; compute `char_len`.
   - `CHAR_TABLE` → fetch string from `CharTable`; if ≤16 bytes inline copy, else mark as overflow (`char_len = 0xFF`) and let caller resolve via the table.
   - `WIDE_CONT` → leave char data empty, set `width = 0`.
4. Return `Cell` by value.

**Implementation Steps**:
1. **Define `SlimCell` and flag constants** in a new module with `#[repr(C)]` and a compile-time-equivalent test asserting size == 8.
2. **Define `StyleEntry`** (Hash + Eq + Copy) and `StyleTable` with the intern / dec_ref / get / saturation helper methods.
3. **Define `CharTable`** with intern / dec_ref / get methods over owned `String`.
4. **Implement `cell_to_slim` and `slim_to_cell`** as free functions in `slim_cell.rs`, taking explicit table references.
5. **Implement `wasm_debug_slim_stats` skeleton** (returning placeholder counts; wired later in Phase 4).
6. **Write unit tests** covering: size assertion; intern returning equal IDs for equal entries; refcount lifecycle; ASCII / CJK / emoji / ZWJ family round-trip; saturation fallback (forge 65,535 unique entries).

**Dependencies**: None. **Blocks**: Phase 2.

**Testing Approach**:
- Unit: round-trip for ASCII / CJK / 8-byte emoji / ZWJ family ≤ 16 bytes; intern dedup; refcount → 0 frees slot; free_list reuse; saturation → ID 0 + warn (warn invocation tested via a counter or a feature-gated hook).
- Integration: deferred to Phase 2.
- E2E (Docker): N/A this phase.
- Manual: N/A.

**Acceptance Criteria**:
- [ ] `size_of::<SlimCell>() == 8` test passes.
- [ ] All new unit tests pass.
- [ ] Existing `cargo test` suite still passes (no integration yet, only additions).
- [ ] No new `unsafe` blocks introduced.

**Estimated Effort**: medium

---

### Phase 2: Ring buffer integration (per-line storage + compression on eviction)

**Goal**: Replace the flat `ring_cells: Vec<Cell>` with a per-line storage that holds `Cell` rows for the viewport and `SlimCell` rows for scrollback. Compression happens exactly when a row crosses the boundary; refcount accounting is correct under all ring transitions (grow, evict, clear).

**Files to Create**: None.

**Files to Modify**:
- `wasm/src/ring_buffer.rs` — replace flat ring with per-row storage; rewrite `ring_push_blank`, `viewport_cell_offset`, `pack_row_abs`, `line_text_abs`, `clear_scrollback`, `resize_no_reflow`.
- `wasm/src/terminal_core.rs` — replace `ring_cells: Vec<Cell>` with the new storage (e.g. `ring_rows: Vec<RingRow>`); add `styles: StyleTable` and `chars: CharTable` fields; adjust constructor.
- `wasm/src/terminal_cells.rs` — read paths for scrollback go through `slim_to_cell`; viewport paths unchanged.
- `wasm/src/terminal_rows.rs` — `get_packed_row` and related materialize `Cell` from `SlimCell` for scrollback; viewport unchanged.
- `wasm/src/csi_*` and other modules touching `ring_cells` directly — switch to per-row accessors (`viewport_row_mut(row) -> &mut [Cell]`, `scrollback_row(idx) -> &[SlimCell]`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `RingRow` | Tagged enum: `Viewport(Vec<Cell>)` or `Scrollback(Vec<SlimCell>)`; both vectors length == cols | `cols > 0` | Caller can dispatch on the variant for read/write |
| `RingBuffer` (logical, kept inside `TerminalCore`) | Owns `Vec<RingRow>` plus `head` / `size` / `capacity` indexing | Constructor sized to `scrollback_lines + rows` | Provides absolute-index → `&RingRow` lookups |
| `compress_row_into_scrollback` | Convert a viewport row into a scrollback row; intern all cells; return new `Scrollback(Vec<SlimCell>)` | Row is in viewport mode; tables reachable | Returns `Vec<SlimCell>` with refcounts incremented; original `Vec<Cell>` consumed |
| `decompress_row_into_viewport` | Reverse direction (used only on rare reflow that grows viewport into prior scrollback) | Row is in scrollback mode | Returns `Vec<Cell>`; refcounts decremented |
| `release_scrollback_row` | Decrement table refcounts for every `SlimCell` in a row about to be overwritten | Row is in scrollback mode | All cells' style_id / char_id refcounts decremented |
| Viewport accessors (`viewport_row(row)` / `viewport_row_mut(row)`) | Borrow the `Vec<Cell>` for a viewport row without exposing the enum | Row index < rows | Returns `&[Cell]` / `&mut [Cell]` |
| Scrollback accessors (`scrollback_row(idx)`) | Borrow the `Vec<SlimCell>` for a scrollback row | Index < scrollback_count | Returns `&[SlimCell]` |

**Processing Flow (`ring_push_blank` rewritten)**:
1. Compute the new absolute slot index.
2. Determine whether the slot currently holds a previous scrollback row.
   - If yes → call `release_scrollback_row` to decrement all refcounts, then drop that `Vec<SlimCell>`.
3. Identify which existing viewport row is being pushed out (the one that was at viewport row 0 *before* the push).
   - Take the `Vec<Cell>` out of that ring slot, run `compress_row_into_scrollback`, and store the resulting `Vec<SlimCell>` in the same slot — now tagged `Scrollback`.
   - For overflow strings keyed by `(col, abs_row)`, look them up in the `OverflowTable` and pass them to `cell_to_slim` so they can be re-interned in `CharTable`. Then remove the overflow entries (they are now inside `CharTable`).
4. Allocate a new `Viewport(Vec<Cell>)` filled with BCE blanks at the new slot's logical position (the new viewport bottom).
5. Update `ring_head` / `ring_size` and dirty / scroll-event bookkeeping (semantics unchanged from current implementation).

**Processing Flow (`pack_row_abs` rewritten)**:
1. Resolve `abs` to a `&RingRow`.
2. If `Viewport(cells)` → existing packing logic over `&[Cell]` (with `OverflowTable` lookup as today).
3. If `Scrollback(slim_cells)` → for each `SlimCell`, materialize a `Cell` via `slim_to_cell`. If the resulting `Cell` is overflow (`char_len == 0xFF`), fetch the full string from the `CharTable` rather than the legacy `OverflowTable`.

**Implementation Steps**:
1. **Introduce `RingRow` enum and rewrite the storage field** in `TerminalCore`. Update the constructor to allocate `rows + scrollback_lines` slots, with the first `rows` slots as `Viewport(Vec<Cell::EMPTY>)` and the rest unallocated / lazily created as `Viewport` placeholders (so the existing invariant "ring_size always >= rows" is preserved).
2. **Add intern table fields** (`styles`, `chars`) and initialize in `new()`.
3. **Replace direct `ring_cells[base + col]` accesses** project-wide with per-row accessors. Keep `viewport_cell_offset` as a logical helper that returns `(slot_index, col)` instead of a flat index, or replace its callers.
4. **Rewrite `ring_push_blank`** per the flow above. Migrate the existing BCE fast-path (default-bg memset) to operate on the new viewport `Vec<Cell>`.
5. **Rewrite `pack_row_abs` and `line_text_abs`** to dispatch on `RingRow` and call `slim_to_cell` for scrollback rows.
6. **Update `clear_scrollback` and `resize_no_reflow`** to work with the new storage and to release table refcounts for scrollback rows being discarded.
7. **Migrate the `OverflowTable` integration**: scrollback overflow goes through `CharTable`. Viewport overflow continues to use `(col, abs_row)`-keyed `OverflowTable` (limited blast radius). The compression step pulls overflow strings out of `OverflowTable` and into `CharTable`; the decompression step (rare) pushes back into `OverflowTable` if the materialized `Cell` is overflow-sized.
8. **Run the full existing test suite** and fix breakages caused by the storage change. Most existing tests should pass unmodified because they go through the public API.

**Dependencies**: Requires Phase 1. **Blocks**: Phase 3, Phase 4.

**Testing Approach**:
- Unit (added):
  - Eviction lifecycle: push 100 rows into scrollback, then push 10,000 more — first 100 evicted, table refcounts back at zero for those styles (assert via debug stats).
  - Same-style spam: 1,000,000 cells with the same fg/bg → `StyleTable.live_entries() == 2` (default + the one new style); `style.refcount` == 1,000,000 (or saturating cap).
  - Overflow round-trip: ZWJ family in scrollback recovered correctly via `get_scrollback_text`.
  - Render parity: `get_scrollback_row_packed` of a `SlimCell`-backed row equals `get_row_packed` of the same content while still in viewport.
- Integration: existing scrollback tests in `ring_buffer.rs` continue to pass.
- E2E (Docker): deferred to Phase 5.
- Manual: N/A.

**Acceptance Criteria**:
- [ ] All existing wasm tests pass without changes (except those that directly inspected `ring_cells` raw layout).
- [ ] New eviction / dedup / overflow tests pass.
- [ ] No new `unsafe` blocks beyond what already exists in `ring_push_blank` (the existing `write_bytes` fast-path stays inside the `Vec<Cell>` allocation, which is still safe-equivalent).

**Estimated Effort**: large

---

### Phase 3: Reflow integration

**Goal**: Make `resize_reflow` (and its same-width variant) correctly handle rings that mix `Viewport(Vec<Cell>)` and `Scrollback(Vec<SlimCell>)` rows, preserving every cell's content and style across resize, with correct refcount accounting after the operation.

**Files to Create**: None.

**Files to Modify**:
- `wasm/src/reflow.rs` — extend `reflow_drain` and the `resize_*` family to materialize `Cell` rows from `SlimCell` slots before reflow logic runs; recompress at the end.
- `wasm/src/terminal_core.rs` — provide a `rebuild_intern_tables_from_ring` helper used post-reflow.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `reflow_drain` (modified) | Convert every ring row to a `PhysicalLine` of `Cell` for reflow logic | Ring is in mixed state | Returns `Vec<PhysicalLine>` with all overflow data populated |
| `rebuild_intern_tables_from_ring` | Allocate fresh `StyleTable` / `CharTable`, walk every scrollback row, re-intern, replace the existing tables | Reflow has finished and ring rows are in their final positions | Tables hold exactly the entries needed by the current ring; refcounts equal the cell counts |
| `resize_*` variants | Preserve cursor location and wrapped flags exactly as today, while operating on `Cell` for the duration of the reflow | Ring is in mixed state | Final ring has viewport rows as `Cell` and scrollback rows as freshly compressed `SlimCell` |

**Processing Flow**:
1. Drain phase:
   - For each ring slot from oldest to newest: if `Scrollback`, decompress every cell into a `Cell` (look up CharTable for non-inline chars, materialize as overflow into the `PhysicalLine.overflow_data` if >16 bytes). If `Viewport`, copy `Cell` directly.
2. Existing reflow logic (`build_logical_lines`, `split_logical_to_physical`, etc.) runs unchanged on `Vec<PhysicalLine>`.
3. Repopulate phase:
   - Allocate fresh `StyleTable` and `CharTable` on the side.
   - Allocate a new ring with new dimensions; place reflowed lines:
     - Viewport rows → `Viewport(Vec<Cell>)`.
     - Scrollback rows → compress per-cell into `SlimCell`, interning into the *new* tables.
   - Atomically swap the new ring + new tables into `TerminalCore`. The old tables are dropped (and with them, all stale refcounts).

**Implementation Steps**:
1. **Adjust `reflow_drain`** to iterate over `RingRow` and decompress on the fly. Make sure overflow strings come from either `OverflowTable` (viewport) or `CharTable` (scrollback) depending on the source slot.
2. **Adjust `resize_same_width`** to repopulate via the new path: for each kept line, decide whether it lands in viewport or scrollback (based on its position relative to `vp_start`) and store as the appropriate `RingRow` variant. Recompress scrollback rows during this step using newly allocated tables.
3. **Adjust `resize_full_reflow`** symmetrically.
4. **Add `rebuild_intern_tables_from_ring`** as a fallback / safety net for tests (used by an assertion in debug builds: after reflow, rebuild and compare table content + refcounts to detect bugs early).
5. **Run the existing reflow tests** and fix breakages. Add new tests covering: rich scrollback (multiple colors + emoji + hyperlink) reflow round-trip; `dec_ref → 0` consistency after reflow.

**Dependencies**: Requires Phase 2. **Blocks**: Phase 4 (snapshot V2 needs rebuild helper).

**Testing Approach**:
- Unit (added): reflow on a 200×200 scrollback with 10 distinct colors, 5 hyperlinks, 3 ZWJ family emoji → all visible attributes preserved; `live_entries()` matches expectations.
- Integration: existing reflow tests continue to pass.
- Performance: a small Rust bench (gated behind `cargo bench` or a `--release` test) measuring 10,000 × 200 reflow latency before/after.
- E2E (Docker): deferred to Phase 5.

**Acceptance Criteria**:
- [ ] All existing reflow tests pass.
- [ ] New reflow-with-rich-content tests pass.
- [ ] Reflow latency on 10,000 × 200 within 2× of baseline (NFR5).
- [ ] Post-reflow `rebuild_intern_tables_from_ring` produces tables structurally equal to the live tables (debug-only assertion).

**Estimated Effort**: medium

---

### Phase 4: Snapshot V2 + debug stats export

**Goal**: Bump snapshot serialization to V2, capable of round-tripping mixed `Cell` / `SlimCell` rings plus the intern tables. V1 snapshots remain readable in viewport-only mode (scrollback discarded). Expose `wasm_debug_slim_stats()` to JS.

**Files to Create**: None.

**Files to Modify**:
- `wasm/src/snapshot.rs` — define `SnapshotV2` schema (viewport rows as `Vec<Cell>`, scrollback rows as `Vec<SlimCell>`, serialized `StyleTable` and `CharTable`); read both V1 and V2; write V2 only.
- `wasm/src/style_table.rs`, `char_table.rs` — `to_serialized` / `from_serialized` helpers (storage Vec, refcounts, free_list) using `serde::Serialize` / `Deserialize` derives.
- `wasm/src/lib.rs` — export `wasm_debug_slim_stats`.
- `wasm/src/terminal_core.rs` — implement the stats accessor (`live_style_entries`, `style_bytes_used`, `live_char_entries`, `char_bytes_used`, `slim_cell_total`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `SnapshotEnvelope.version` | Distinguishes V1 from V2 payloads | Reader checks before deserializing payload | Correct payload type loaded |
| `SnapshotV1` (legacy) | Existing fields; loaded as viewport-only | Envelope version == 1 | Restored TerminalCore has scrollback empty; viewport content preserved |
| `SnapshotV2` | Viewport rows + scrollback rows + serialized tables + existing fields (cursor, modes, etc.) | Envelope version == 2 | Restored TerminalCore has full mixed ring with table refcounts intact |
| `SerializedStyleTable` / `SerializedCharTable` | Stable serde schema mirroring the in-memory layout | Read-side validates lengths and IDs | Round-trip preserves all entries and refcounts |
| `wasm_debug_slim_stats` | Returns `{ slim_cells, style_entries, style_bytes, char_entries, char_bytes }` to JS | TerminalCore exists | Numbers reflect current state for use in benches/tests |

**Processing Flow (snapshot read)**:
1. Read envelope; branch on `version`.
2. V1 path: deserialize the legacy `TerminalSnapshot`; build a fresh `TerminalCore` with all rows in `Viewport` mode (limited to `rows`); discard any flat `ring_cells` beyond the viewport (legacy "scrollback" data dropped).
3. V2 path: deserialize `SnapshotV2`; rebuild `RingRow` vector by mapping each entry; deserialize tables; sanity-check that scrollback-row IDs are within bounds; reject if not.

**Implementation Steps**:
1. **Bump `SNAPSHOT_VERSION`** to 2; keep `SnapshotV1` (renaming the current struct) for read-only legacy use.
2. **Define `SnapshotV2`** with the new fields. Add serde derives where needed on `SlimCell`, `StyleEntry`.
3. **Implement read-side dispatch** in `from_envelope` (or equivalent) selecting V1 vs V2.
4. **Implement table serialization helpers** (`SerializedStyleTable`, `SerializedCharTable`) and their deserialization, validating invariants before installation.
5. **Implement `wasm_debug_slim_stats`** returning a small struct via `serde_wasm_bindgen`.
6. **Add tests**: V1-load → V2-save → reload round-trip; corrupt V2 (invalid style_id) → graceful error; debug stats reflect live state.

**Dependencies**: Requires Phase 2 + Phase 3. **Blocks**: Phase 5 (verification needs stats).

**Testing Approach**:
- Unit: V1 round-trip (legacy snapshot loads with empty scrollback); V2 round-trip with rich scrollback; corrupted V2 rejection; debug stats values match expectations.
- Integration: existing `snapshot.rs` tests continue to pass.
- E2E (Docker): deferred to Phase 5.

**Acceptance Criteria**:
- [ ] `SNAPSHOT_VERSION == 2`.
- [ ] V1 snapshot loads (viewport only), saving produces V2.
- [ ] V2 round-trip preserves every cell + table entry.
- [ ] `wasm_debug_slim_stats()` returns a non-null JS object with all five fields.

**Estimated Effort**: medium

---

### Phase 5: Bench harness + verification + E2E

**Goal**: Quantify memory, render latency, compression / decompression latency, and reflow latency. Confirm all SPEC.md acceptance criteria via tests + measurements + E2E.

**Files to Create**:
- `wasm/benches/slim_cell_bench.rs` (or `cargo test --release`-driven measurement module under `wasm/src/bench.rs`) — micro-benches for compression, decompression, reflow, full scrollback memory accounting.
- (Optional) `e2e-tests/specs/slim_cell.e2e.js` — scroll-up through 5000 lines and verify renderer still draws.

**Files to Modify**:
- `doc/tasks/wasm-slim-cell/VERIFICATION.md` — populated in this phase with run commands + observed results (the artifact is created up-front but filled in during this phase).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `bench_compress_row` | Time `cell_to_slim` over a 200-cell row, repeated N times, report ns/op | Phase 1–4 implementation | Per-row latency reported; pass if ≤ 50 µs |
| `bench_decompress_cell` | Time `slim_to_cell` over a single cell, report ns/op | Phase 1–4 implementation | Per-cell latency reported; pass if ≤ 200 ns |
| `bench_scrollback_memory` | Allocate a 10,000 × 200 ring fully populated with realistic data, sum the heap usage of `RingRow` storage + `StyleTable` + `CharTable`, compare against a baseline that uses only `Cell` rows | Phase 2 implementation | Memory ratio reported; pass if ≤ 50% |
| `bench_reflow` | Time `resize_reflow` from 200 cols → 100 cols on a 10,000 × 200 scrollback | Phase 3 implementation | Latency reported; pass if ≤ 2× baseline |
| `bench_scroll_render` | Time the per-frame work (decompress + pack_row) for typical scroll patterns | Phase 2 implementation | p50/p95/p99 reported; pass if p99 within 5% of baseline |
| E2E spec (`slim_cell.e2e.js`) | Drive `tauri-driver` to scroll back through colored output + emoji and screenshot; manual visual inspection of screenshots | Docker E2E available | Screenshot saved; existing E2E tests still green |

**Implementation Steps**:
1. **Add a small bench harness** under `wasm/src/bench.rs` gated behind `#[cfg(any(test, feature = "bench"))]`, using `std::time::Instant` (works in WASM via `web_time` or via a host-side test). Prefer `cargo test --release` running on the host crate (Rust target, not WASM) to keep the bench reproducible. The same `SlimCell` / `StyleTable` code path runs identically on the host.
2. **Compute the baseline** by also running each bench against a synthetic `Cell`-only ring (use the original `cell.rs` module behind a `#[cfg(test)]` clone, or measure git-pre-change values manually and record them in VERIFICATION.md).
3. **Wire `wasm_debug_slim_stats`** into a tiny TypeScript helper invocation inside an E2E test that scrolls 5,000 lines and asserts `style_entries < some_threshold`.
4. **Run E2E in Docker**: `./scripts/run-e2e-docker.sh test` — confirm no regressions.
5. **Fill VERIFICATION.md** with observed numbers and pass/fail per criterion.

**Dependencies**: Requires Phases 1–4. **Blocks**: nothing (terminal phase).

**Testing Approach**:
- Unit: bench functions also serve as smoke tests.
- Integration: Docker `cargo test` full run.
- E2E (Docker): `./scripts/run-e2e-docker.sh test` (existing specs + new `slim_cell.e2e.js` if added).
- Manual: visual inspection of screenshots; optional 8-hour Claude Code session RSS measurement (NFR / SC).

**Acceptance Criteria**:
- [ ] `bench_scrollback_memory` reports ≥ 50% reduction.
- [ ] `bench_scroll_render` p99 within 5% of baseline.
- [ ] `bench_reflow` within 2× of baseline.
- [ ] `bench_compress_row` ≤ 50 µs.
- [ ] `bench_decompress_cell` ≤ 200 ns.
- [ ] All existing + new unit / integration tests pass.
- [ ] All E2E tests pass via Docker.
- [ ] VERIFICATION.md filled in with observed numbers.

**Estimated Effort**: medium

---

## Complete File Structure

```
wasm/src/
├── cell.rs                # UNCHANGED struct; existing OverflowTable retained for viewport
├── slim_cell.rs           # NEW: SlimCell + flag constants + cell_to_slim / slim_to_cell
├── style_table.rs         # NEW: StyleEntry + StyleTable
├── char_table.rs          # NEW: CharTable
├── ring_buffer.rs         # MODIFIED: RingRow per slot; compression on eviction
├── reflow.rs              # MODIFIED: decompress → reflow → recompress
├── terminal_core.rs       # MODIFIED: owns styles + chars + ring_rows; new stats helpers
├── terminal_cells.rs      # MODIFIED: scrollback read paths via slim_to_cell
├── terminal_rows.rs       # MODIFIED: get_packed_row dispatches on RingRow
├── snapshot.rs            # MODIFIED: V1 read-only + V2 read/write + serialized tables
├── lib.rs                 # MODIFIED: register modules + export wasm_debug_slim_stats
├── bench.rs               # NEW (Phase 5): bench harness behind cfg gate
└── ... (other existing files updated only where they accessed ring_cells directly)

doc/tasks/wasm-slim-cell/
├── SPEC.md                # source of truth (already exists)
├── 要件定義書.md           # Japanese requirements (already exists)
├── sdd.yaml               # workflow tracking (already exists)
├── IMPLEMENTATION.md      # this file
├── VERIFICATION.md        # acceptance test plan
└── tasks.yaml             # phase / task / requirement mapping
```

## Testing Strategy

- **Unit**: per-module tests in `slim_cell.rs`, `style_table.rs`, `char_table.rs`. Coverage target ≥ 90% for these new modules; ≥ 80% project-wide.
- **Integration**: existing `ring_buffer.rs`, `reflow.rs`, `snapshot.rs`, `terminal_*.rs` test suites continue to pass; new tests for eviction lifecycle, rich-content reflow, snapshot V1/V2 round-trip.
- **E2E (Docker)**: `./scripts/run-e2e-docker.sh test`. New `slim_cell.e2e.js` (optional) that scrolls through colored output and ZWJ emoji.
- **Manual**: 8-hour Claude Code session RSS observation (deferred but listed for completeness); visual inspection of saved screenshots.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (no new dependencies) | — | All required functionality already available via existing crates (`std`, `serde`, `bincode`, `wasm-bindgen`, `web-sys`) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| StyleTable u16 saturation in real workloads | Low (theoretical workloads stay well under 65 535 styles; must be measured) | Medium (visible style loss until cleared) | FR9 fallback (id 0 + warn); FR11 stats let us monitor; if measurement shows >50% saturation rate, revert to u32 IDs and grow `SlimCell` to 10 bytes — this is a 1-day rework localized to `slim_cell.rs` and `style_table.rs` |
| Refcount accounting bugs cause memory leak or use-after-free of intern slots | Medium | High (silent memory growth) | `debug_assert` on every dec_ref; post-reflow `rebuild_intern_tables_from_ring` debug-mode assertion; targeted unit tests for every state transition |
| Reflow regression beyond 2× | Low | Medium (UX impact only on resize) | Phase 5 bench gates the milestone; if exceeded, profile and optimize before merge |
| Snapshot V1 → V2 migration drops live scrollback for users on upgrade | Certain (one-time) | Low (snapshots are short-lived, per-session) | Documented in SPEC §Migration; release notes |
| Hidden coupling: other modules read `ring_cells` directly | Medium | Medium (compile / test breakage during refactor) | Phase 2 step 3 sweeps the project for direct accesses; per-row accessors enforce the new boundary |
| Bench bias because the bench runs on host (x86) and not WASM | Low | Low | Document the discrepancy in VERIFICATION.md; cross-check with one in-WASM measurement via `wasm_debug_slim_stats` and a TypeScript timing wrapper |

## Open Questions

- [x] **FR3 saturation threshold (u16 vs u32)** — Resolved by adopting u16 with monitoring (FR11) and graceful fallback (FR9). Re-evaluate if production telemetry shows saturation; rework plan documented in Risk Assessment above.
- [x] **FR10 snapshot strategy (bump vs dual-read)** — Resolved by bumping to V2 with V1 read-only fallback (viewport preserved, scrollback dropped). Acceptable per SPEC §Migration.
- [ ] Whether to ship `bench.rs` results into the repository (e.g. as a check-in artifact) or only as VERIFICATION.md notes — defer to review.

## Success Metrics

- [ ] Functional completeness: every FR1–FR11 implemented.
- [ ] Quality: existing tests green, new tests added per Phase, no new `unsafe`.
- [ ] Performance: NFR1–NFR5 satisfied per Phase 5 measurements.
- [ ] Compatibility: WASM public API and packed binary format unchanged (NFR6); TypeScript callers untouched.
