# Feature: Markdown Pipeline Optimization

## Overview

Post-review optimizations for the large-markdown-support feature. Addresses memory allocation efficiency, unnecessary string copies, dead code, and minor test issues identified during code review.

## Objectives

- Reduce unnecessary memory allocations in the markdown pipeline (Rust backend)
- Eliminate redundant string copies in the WASM OSC parser
- Remove dead code fields from TypeScript interfaces
- Fix test hygiene issues (unused imports, fixture dependencies)

## Technical Requirements

### Functional Requirements

- **FR1: Pre-allocate `read_to_end` buffer** — In `src-tauri/src/commands/markdown.rs`, use `Vec::with_capacity(metadata.len() as usize)` before `file.read_to_end()` to avoid reallocation for large files.

- **FR2: Early release of intermediate variables** — In `src-tauri/src/commands/markdown.rs`, `drop(content)` after base64 encoding and `drop(encoded)` after chunking to reduce peak memory usage.

- **FR3: Zero-copy `dispatch_osc` for valid UTF-8** — In `wasm/src/parser.rs`, replace `String::from_utf8_lossy(&self.osc_buffer).to_string()` with `String::from_utf8` that takes ownership of the buffer. Fall back to `from_utf8_lossy` only when the buffer contains invalid UTF-8. Re-initialize `osc_buffer` with `Vec::new()` after ownership transfer.

- **FR4: Remove dead code fields from `MarkdownSession`** — Remove `createdAt`, `dataSize`, and `nextSeq` from `MarkdownSession` interface in `src/markdown/types.ts` and corresponding assignments in `src/markdown/session.ts`. Update tests that reference these fields.

- **FR5: Pre-allocate OSC output String** — In `src-tauri/src/encoding/osc.rs`, calculate estimated output size from chunk data length and header overhead before constructing the String. Use `String::with_capacity()`.

- **FR6: Optimize Base64 byte conversion** — In `src/markdown/session.ts` `decodeBase64Utf8()`, replace the byte-by-byte `for` loop with `Uint8Array.from(binary, c => c.charCodeAt(0))`.

- **FR7: Fix test hygiene** — In `src-tauri/tests/integration/markdown_tests.rs`:
  - Remove unused `use predicates::prelude::*;` import.
  - Replace `tests/fixtures/large.md` dependency in `test_markdown_at_size_limit` with a dynamically generated temp file (consistent with other tests).

### Non-Functional Requirements

- **NFR1 - No behavioral regression:** All existing tests must pass. No observable behavior change in markdown display.
- **NFR2 - Memory efficiency:** Peak memory for a 10MB file should decrease (intermediate variables released earlier, pre-allocated buffers avoid geometric growth waste).

## Implementation Approach

### Affected Files

| File | Change | FR |
|------|--------|----|
| `src-tauri/src/commands/markdown.rs` | Pre-allocate Vec, drop intermediates | FR1, FR2 |
| `wasm/src/parser.rs` | Zero-copy `dispatch_osc` | FR3 |
| `src/markdown/types.ts` | Remove `createdAt`, `dataSize`, `nextSeq` | FR4 |
| `src/markdown/session.ts` | Remove dead field assignments, optimize `decodeBase64Utf8` | FR4, FR6 |
| `src/markdown/session.test.ts` | Update tests for removed fields | FR4 |
| `src-tauri/src/encoding/osc.rs` | Pre-allocate output String | FR5 |
| `src-tauri/tests/integration/markdown_tests.rs` | Remove unused import, replace fixture | FR7 |

### FR3 Detail: `dispatch_osc` Change

```rust
// Before
fn dispatch_osc<F>(&mut self, emit: &mut F) where F: FnMut(ParsedAction) {
    let data = String::from_utf8_lossy(&self.osc_buffer).to_string();
    emit(ParsedAction::OscDispatch { param: self.osc_param, data });
    self.osc_buffer.clear();
    // ...
}

// After
fn dispatch_osc<F>(&mut self, emit: &mut F) where F: FnMut(ParsedAction) {
    let buf = std::mem::take(&mut self.osc_buffer);
    let data = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    emit(ParsedAction::OscDispatch { param: self.osc_param, data });
    // osc_buffer is now an empty Vec (from mem::take)
    // ...
}
```

Trade-off: Buffer capacity is lost on each dispatch. For markdown chunks (128KB each), this means re-allocation per chunk. This is acceptable because:
- Markdown chunks are infrequent (one per 128KB of content)
- The previous code also allocated a new String per dispatch
- Small OSC sequences (common case) use minimal memory either way

### FR5 Detail: OSC Output Pre-allocation

```rust
// Estimate: header(~100 chars) * (num_chunks + 2) + total_data_length
let total_data: usize = chunks.iter().map(|c| c.len()).sum();
let estimated = total_data + 100 * (chunks.len() + 2);
let mut output = String::with_capacity(estimated);
```

## Test Scenarios

### Unit Tests
- [ ] WASM parser `dispatch_osc` correctly dispatches valid UTF-8 OSC data
- [ ] WASM parser `dispatch_osc` correctly handles invalid UTF-8 (lossy conversion)
- [ ] WASM parser `osc_buffer` is empty after dispatch
- [ ] `decodeBase64Utf8` produces identical output with new implementation
- [ ] `MarkdownSession` interface no longer contains `createdAt`, `dataSize`, `nextSeq`

### Integration Tests
- [ ] CLI processes markdown files without regression
- [ ] Integration test no longer depends on `tests/fixtures/large.md`

### Existing Tests
- [ ] All Rust tests pass: `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] All TypeScript tests pass: `bun test`
- [ ] TypeScript type check passes: `bun run typecheck`

## Security Considerations

- **UTF-8 handling:** The `from_utf8` fallback to `from_utf8_lossy` preserves the safety guarantee. No `unsafe` code is introduced.
- **No new attack surface:** Changes are internal optimizations only.

## Success Criteria

- [ ] All existing tests pass without modification (except tests directly affected by dead code removal and fixture changes)
- [ ] No behavioral change in markdown display
- [ ] `predicates` unused import warning eliminated
- [ ] Dead code fields removed cleanly

## References

- Review recommendations: `tmp/2026-02-25-large-markdown-review-recommendations.md`
- Parent feature spec: `doc/tasks/large-markdown-support/SPEC.md`
