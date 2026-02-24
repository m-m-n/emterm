# Feature: Large Markdown Display Support

## Overview

Fix a critical bug where markdown content is silently truncated due to the WASM parser's OSC buffer limit (4096 bytes) being incompatible with the markdown chunk size (64KB). Remove all artificial size limits to allow displaying arbitrarily large markdown files.

## Objectives

- Fix the `MAX_OSC_LEN` bottleneck in the WASM parser that causes markdown truncation
- Remove file size and session size limits across the pipeline
- Improve session timeout handling for large file transfers
- Optimize chunk size for better throughput

## User Stories

### US1: Display Standard Documentation
As a developer, I want to display markdown documentation of any size via `emterm markdown`, so that I can read documents without truncation.

**Acceptance Criteria:**
- [ ] A 200-line, 12KB markdown file displays completely without truncation
- [ ] Documentation files of tens of KiB display correctly
- [ ] Existing small markdown files continue to work as before

### US2: Display Large Data Markdown
As a data analyst, I want to display large markdown files containing data tables, so that I can review data summaries in the terminal.

**Acceptance Criteria:**
- [ ] Multi-MiB markdown files display correctly
- [ ] No artificial file size limit prevents display
- [ ] Session does not time out during large file transfer

## Technical Requirements

### Functional Requirements

- **FR1: Expand WASM OSC buffer** — Increase `MAX_OSC_LEN` in `wasm/src/parser.rs` from 4096 to 16MB (16 * 1024 * 1024), matching `MAX_DCS_LEN`.
- **FR2: Remove CLI file size limit** — Remove the `MAX_MARKDOWN_SIZE` constant (2MB) and its file size check in `src-tauri/src/commands/markdown.rs`.
- **FR3: Remove frontend session size limit** — Remove the `MAX_SESSION_SIZE` constant (2MB) and its size check in `src/markdown/session.ts`.
- **FR4: Reset session timeout on chunk receipt** — In `handleChunk()`, reset the session timeout timer each time a chunk is received, preventing timeout during slow transfers.
- **FR5: Increase chunk size** — Change `MARKDOWN_CHUNK_SIZE` in `src-tauri/src/commands/markdown.rs` from 64KB to 128KB (128 * 1024).

### Non-Functional Requirements

- **NFR1 - Performance:** Small markdown files (< 1KB) must display with no perceptible latency regression. Multi-MiB files should display within a few seconds.
- **NFR2 - Compatibility:** tmux passthrough must continue to work. Individual OSC sequences must stay within the tmux passthrough buffer limit (~256KB).
- **NFR3 - Memory:** The OSC buffer initial capacity should remain small (e.g., 256 or 1024 bytes) to avoid wasting memory for small OSC sequences. The buffer grows dynamically up to the 16MB cap.

## Implementation Approach

### Architecture

No architectural changes. This is a configuration/limit adjustment across three existing layers:

```
CLI (Rust)              → Remove MAX_MARKDOWN_SIZE, increase MARKDOWN_CHUNK_SIZE
  ↓ stdout (OSC sequences)
WASM Parser             → Increase MAX_OSC_LEN to 16MB
  ↓ callback
Frontend (TypeScript)   → Remove MAX_SESSION_SIZE, reset timeout on chunk
```

### Data Flow

```
emterm markdown file.md
  → Read file (no size limit)
  → Base64 encode
  → Split into 128KB chunks
  → Output OSC sequences:
      ESC ] 777;emterm;markdown;begin;id=UUID;... ST
      ESC ] 777;emterm;markdown;chunk;id=UUID;seq=0;data=BASE64_128KB ST
      ESC ] 777;emterm;markdown;chunk;id=UUID;seq=1;data=BASE64_128KB ST
      ...
      ESC ] 777;emterm;markdown;end;id=UUID ST
  → PTY delivers to WASM parser (4096-byte reads, parser accumulates)
  → WASM OSC buffer holds full chunk sequence (up to 16MB)
  → dispatch_osc fires callback to TypeScript
  → MarkdownSessionManager accumulates decoded chunks
  → On "end": render full markdown in overlay
```

### Affected Files

| File | Change |
|------|--------|
| `wasm/src/parser.rs` | `MAX_OSC_LEN`: 4096 → 16MB |
| `src-tauri/src/commands/markdown.rs` | Remove `MAX_MARKDOWN_SIZE`, change `MARKDOWN_CHUNK_SIZE`: 64KB → 128KB |
| `src/markdown/session.ts` | Remove `MAX_SESSION_SIZE`, add timeout reset in `handleChunk()` |

### Dependencies

**Internal Dependencies:**
- WASM parser: Core change that enables the fix
- CLI markdown command: Size limit and chunk size changes
- Frontend session manager: Size limit and timeout changes

**External Dependencies:**
- None (no new dependencies)

## Test Scenarios

### Unit Tests
- [ ] WASM parser correctly handles OSC sequences > 4096 bytes
- [ ] WASM parser correctly handles OSC sequences up to 128KB + header overhead
- [ ] CLI reads and chunks files larger than 2MB without error
- [ ] Frontend session accumulates chunks without size limit
- [ ] Frontend session timeout resets on each chunk receipt

### Integration Tests
- [ ] End-to-end: 12KB markdown file displays completely
- [ ] End-to-end: 1MB markdown file displays completely
- [ ] Session timeout does not fire during multi-chunk transfer

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Empty markdown file: Renders empty overlay
- [ ] Single-byte markdown file: Renders correctly
- [ ] Markdown exactly at chunk boundary (128KB of base64): No off-by-one
- [ ] Very large file (10MB+): Completes without timeout or crash

## Security Considerations

- **Input Validation:** Base64 validation in `decodeBase64Utf8()` is preserved
- **Memory:** No arbitrary memory allocation from untrusted input; OSC buffer has a 16MB cap. The parser discards bytes beyond the cap rather than allocating unbounded memory.
- **XSS Prevention:** Markdown rendering sanitization is unchanged (not in scope)

## Error Handling

No new error conditions are introduced. Existing error handling is simplified by removing the file-too-large and session-too-large checks.

| Removed Error | Previous Condition |
|---------------|--------------------|
| `FileTooLarge` | File > 2MB (CLI) |
| Session size exceeded warning | Session data > 2MB (Frontend) |

## Performance Optimization

### Chunk Size Increase
- Previous: 64KB chunks → ~2000 OSC sequences for a 100MB file
- New: 128KB chunks → ~1000 OSC sequences for a 100MB file
- Fewer OSC sequences means less parsing overhead and fewer JS callbacks

### OSC Buffer Initial Capacity
- Keep initial capacity small (256 or 1024 bytes) for common small OSC sequences
- Vec grows geometrically, so large sequences cause a few reallocations but no performance issue

## Success Criteria

- [ ] A 200-line, 12KB markdown file displays completely (original bug is fixed)
- [ ] Multi-MiB markdown files display without truncation
- [ ] No artificial size limits remain in the pipeline
- [ ] tmux passthrough compatibility is maintained
- [ ] All existing tests pass
- [ ] Session timeout resets correctly during chunk transfers

## Open Questions

None. All requirements have been clarified.

## References

- Bug report: 200-line 11699-byte markdown truncated during `emterm markdown` display
- WASM parser: `wasm/src/parser.rs` (MAX_OSC_LEN)
- CLI markdown: `src-tauri/src/commands/markdown.rs` (MAX_MARKDOWN_SIZE, MARKDOWN_CHUNK_SIZE)
- Frontend session: `src/markdown/session.ts` (MAX_SESSION_SIZE, SESSION_TIMEOUT)
