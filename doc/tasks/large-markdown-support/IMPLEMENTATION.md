# Implementation Plan: Large Markdown Display Support

## Overview

Fix the critical markdown truncation bug caused by the WASM parser's OSC buffer limit (4096 bytes), and remove all artificial size limits across the CLI, WASM parser, and frontend session manager to support arbitrarily large markdown files.

## Objectives

- Fix `MAX_OSC_LEN` bottleneck in WASM parser that truncates markdown chunks
- Remove file size and session size limits from CLI and frontend
- Reset session timeout on each chunk receipt to prevent timeout during large transfers
- Increase chunk size from 64KB to 128KB for better throughput

## Prerequisites

### Development Environment
- Rust toolchain (for WASM and Tauri backend)
- Bun (for TypeScript tests and bundling)
- Docker (for test execution)

### Dependencies
- No new external dependencies required

## Architecture Overview

### Technology Stack
- **WASM Parser**: Rust compiled to WebAssembly
- **CLI Backend**: Rust (Tauri)
- **Frontend Session**: Vanilla TypeScript

### Design Approach

Configuration/limit adjustment only. No architectural changes. Three independent layers are modified in parallel:

1. **WASM Parser** — Increase OSC buffer cap
2. **CLI Command** — Remove file size limit, increase chunk size
3. **Frontend Session** — Remove session size limit, add timeout reset

### Component Interaction

```
CLI (Rust) → stdout (OSC sequences) → WASM Parser → callback → Frontend Session → Render
```

Each layer independently enforces its own limits. The changes remove or raise those limits so the full pipeline can handle large files.

## Implementation Phases

### Phase 1: WASM Parser — Expand OSC Buffer Limit

**Goal**: Allow the WASM parser to accumulate OSC sequences up to 16MB, matching the existing `MAX_DCS_LEN`.

**Files to Modify**:
- `wasm/src/parser.rs` — Change `MAX_OSC_LEN` constant

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MAX_OSC_LEN` constant | Define upper limit for OSC buffer | Currently 4096 bytes | 16 * 1024 * 1024 (16MB) |
| `osc_buffer` initial capacity | Minimize memory for small sequences | `Vec::with_capacity(256)` | Unchanged (256 bytes) — grows dynamically |

**Implementation Steps**:
1. **Update MAX_OSC_LEN** — Change the constant value from 4096 to 16 * 1024 * 1024
2. **Verify initial capacity unchanged** — Confirm `osc_buffer` still starts at 256 bytes
3. **Add unit tests** — Test OSC sequences larger than 4096 bytes parse correctly

**Dependencies**: None (independent layer)

**Testing Approach**:
- Unit: OSC sequence > 4096 bytes produces correct OscDispatch
- Unit: OSC sequence at ~128KB (chunk size + header) produces correct OscDispatch
- Unit: OSC buffer discards bytes beyond 16MB cap

**Acceptance Criteria**:
- [ ] `MAX_OSC_LEN` is 16 * 1024 * 1024
- [ ] Parser correctly handles OSC data larger than 4096 bytes
- [ ] Initial buffer capacity remains small (256 bytes)

**Estimated Effort**: small

---

### Phase 2: CLI — Remove File Size Limit and Increase Chunk Size

**Goal**: Allow `emterm markdown` to process files of any size and output larger chunks for better throughput.

**Files to Modify**:
- `src-tauri/src/commands/markdown.rs` — Remove `MAX_MARKDOWN_SIZE`, change `MARKDOWN_CHUNK_SIZE`, update `open_and_validate_file` call

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MAX_MARKDOWN_SIZE` constant | File size cap | 2MB limit | Removed |
| `MARKDOWN_CHUNK_SIZE` constant | Base64 chunk size | 64KB | 128KB (128 * 1024) |
| `execute_markdown_command` | Read file and output OSC | Calls `open_and_validate_file` with size limit | Reads file without size limit |

**Processing Flow**:
1. Open and validate file (existence, is-a-file) — no size check
2. Read file content
3. Base64 encode, chunk at 128KB boundaries
4. Output OSC sequences to stdout

**Implementation Steps**:
1. **Remove MAX_MARKDOWN_SIZE** — Delete the constant
2. **Increase MARKDOWN_CHUNK_SIZE** — Change from 64KB to 128KB
3. **Replace file open/validate** — Use a file open approach without size validation (open file, check existence and is-file, skip size check)
4. **Update tests** — Remove the oversized-file-rejected test, add test for large file acceptance
5. **Update `CommandError::FileTooLarge` usage** — Verify it's still used by image command; if only markdown used it, assess whether to keep

**Dependencies**: None (independent layer)

**Testing Approach**:
- Unit: File > 2MB is accepted (previously rejected)
- Unit: File not found still returns appropriate error
- Unit: Chunk size is 128KB

**Acceptance Criteria**:
- [ ] No `MAX_MARKDOWN_SIZE` constant exists
- [ ] `MARKDOWN_CHUNK_SIZE` is 128 * 1024
- [ ] Files larger than 2MB are processed without error
- [ ] Non-existent file still returns `FileNotFound`

**Estimated Effort**: small

---

### Phase 3: Frontend Session — Remove Size Limit and Fix Timeout

**Goal**: Allow the session manager to accumulate unlimited data and prevent timeout during large file transfers.

**Files to Modify**:
- `src/markdown/session.ts` — Remove `MAX_SESSION_SIZE`, add timeout reset in `handleChunk()`
- `src/markdown/types.ts` — Add `lastChunkAt` field to `MarkdownSession`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MAX_SESSION_SIZE` constant | Session data cap | 2MB limit | Removed |
| `handleChunk()` method | Accumulate chunk data | Checks size limit, no timeout reset | No size limit, resets timeout on each chunk |
| `MarkdownSession.lastChunkAt` | Track last chunk timestamp | Does not exist | New field, updated on each chunk |
| `cleanupExpiredSessions()` | Remove stale sessions | Checks against `createdAt` | Checks against `lastChunkAt` (most recent activity) |

**Processing Flow**:
1. Receive chunk → decode Base64
2. ~~Check size limit~~ (removed)
3. Store chunk in session
4. Update `lastChunkAt` timestamp
5. Timeout cleanup compares `lastChunkAt` against threshold

**Implementation Steps**:
1. **Remove MAX_SESSION_SIZE** — Delete the constant and the size check in `handleChunk()`
2. **Add `lastChunkAt` to MarkdownSession** — New timestamp field in types, set on begin and updated on each chunk
3. **Reset timeout on chunk** — Update `lastChunkAt` in `handleChunk()`
4. **Update cleanup logic** — Change `cleanupExpiredSessions()` to compare `lastChunkAt` instead of `createdAt`
5. **Update tests** — Remove size limit test, add timeout reset test

**Dependencies**: None (independent layer)

**Testing Approach**:
- Unit: Chunk accumulation works without size limit
- Unit: `lastChunkAt` updates on each chunk
- Unit: Session with recent chunk does not time out
- Unit: Session with old `lastChunkAt` does time out

**Acceptance Criteria**:
- [ ] No `MAX_SESSION_SIZE` constant exists
- [ ] `lastChunkAt` field exists on `MarkdownSession`
- [ ] `handleChunk()` updates `lastChunkAt`
- [ ] `cleanupExpiredSessions()` uses `lastChunkAt`
- [ ] Large data accumulation does not cause session deletion

**Estimated Effort**: small

---

## Complete File Structure

```
wasm/src/parser.rs           — MAX_OSC_LEN: 4096 → 16MB
src-tauri/src/commands/markdown.rs — Remove MAX_MARKDOWN_SIZE, MARKDOWN_CHUNK_SIZE: 64KB → 128KB
src/markdown/session.ts      — Remove MAX_SESSION_SIZE, add timeout reset
src/markdown/types.ts        — Add lastChunkAt to MarkdownSession
```

No new files created.

## Testing Strategy

- **Unit (WASM)**: Large OSC buffer handling, boundary conditions
- **Unit (Rust)**: File size acceptance, chunking at new size
- **Unit (TypeScript)**: Session accumulation, timeout reset behavior
- **Integration**: Existing E2E tests pass without regression
- **E2E (Docker)**: `./scripts/run-e2e-docker.sh test` — existing tests
- **Manual**: Display a 200-line, 12KB markdown file via `emterm markdown` to confirm the original bug is fixed

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | —       | No new dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Memory pressure from 16MB OSC buffer | Low | Medium | Initial capacity stays at 256 bytes; buffer grows geometrically only when needed |
| tmux passthrough buffer overflow | Low | Low | Individual OSC chunks at 128KB are well within tmux's ~256KB limit |
| Existing test breakage from removing limits | Medium | Low | Tests that assert on removed limits must be updated simultaneously |

## Open Questions

None. All requirements have been clarified in the specification.

## Success Metrics

- [ ] 200-line, 12KB markdown file displays completely (original bug fixed)
- [ ] Multi-MiB markdown files display without truncation
- [ ] No artificial size limits remain in the pipeline
- [ ] tmux passthrough compatibility maintained
- [ ] All existing tests pass
- [ ] Session timeout resets correctly during chunk transfers
