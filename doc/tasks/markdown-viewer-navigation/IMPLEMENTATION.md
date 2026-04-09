# Implementation Plan: Markdown Viewer Navigation

## Overview

Extend the `emterm markdown` CLI and fullscreen viewer to support navigating `.md` links and displaying inline images referenced by local paths, using an interactive request-response protocol over PTY.

## Objectives

- Enable clicking `.md` links to navigate between Markdown files in the fullscreen viewer
- Display local-path images inline via lazy loading (IntersectionObserver)
- Maintain SSH compatibility (all data flows through PTY)
- Preserve backward compatibility for pipe mode (one-shot exit)

## Prerequisites

### Development Environment
- Rust 1.85+ (edition 2024)
- Bun (package manager / bundler)
- Docker (for test execution)

### Dependencies
- Existing `base64` crate (already in Cargo.toml)
- `std::io::IsTerminal` trait (stable since Rust 1.70, no new crate needed)
- No new external dependencies required

## Architecture Overview

### Technology Stack
- **Backend**: Rust (Tauri CLI commands, OSC encoding)
- **Frontend**: TypeScript (fullscreen viewer, session manager, renderer)
- **WASM**: Rust (OSC parser routing)

### Design Approach

On-demand request-response protocol over PTY stdin/stdout. The CLI process stays alive (interactive mode) when stdin is TTY, serving navigation and image requests. Frontend writes text commands to PTY stdin; CLI responds with OSC sequences on stdout.

### Component Interaction

```
FullscreenMarkdownView
    |-- Link click --> resolve path --> write "navigate PATH\n" to PTY
    |-- IntersectionObserver --> write "image REQ_ID PATH\n" to PTY
    |-- Close (Esc/q) --> write "quit\n" to PTY

MarkdownSessionManager
    |-- Parse basedir from begin params
    |-- Handle image-response verb --> find placeholder --> set data: URI
    |-- Handle image-error verb --> find placeholder --> show error

CLI Interactive Loop (markdown.rs)
    |-- Read stdin line by line
    |-- "navigate PATH" --> read .md file --> output new OSC session
    |-- "image REQ_ID PATH" --> read file --> output image-response/error OSC
    |-- "quit" / EOF --> exit
```

## Implementation Phases

### Phase 1: OSC Protocol Extensions (Backend)

**Goal**: Extend OSC encoding to support basedir parameter, image-response, and image-error verbs.

**Files to Modify**:
- `src-tauri/src/encoding/osc.rs` - Add basedir to markdown begin, add image-response/error generators

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| generate_markdown_osc | Generate markdown OSC with optional basedir | Valid session_id, chunks, optional basedir path | Begin sequence includes `basedir={path}` when provided |
| generate_image_response_osc | Generate image-response OSC sequence | Valid request_id, mime_type, base64 data | Well-formed OSC with all fields, chunked if data exceeds threshold |
| generate_image_error_osc | Generate image-error OSC sequence | Valid request_id, error message | Well-formed OSC with sanitized error message |

**Processing Flow**:
1. Modify `generate_markdown_osc` signature to accept optional basedir
   - When basedir is present -> append `basedir={sanitized_path}` to begin sequence
   - When absent -> no change to existing output
2. Add image-response generator
   - Accept request_id, mime_type, base64 data
   - For large data -> split into chunks with chunk_seq/chunk_total parameters
   - Each chunk -> separate OSC sequence with shared request_id
3. Add image-error generator
   - Accept request_id, error message
   - Sanitize error message using existing `sanitize_osc_value`

**Implementation Steps**:
1. **Extend generate_markdown_osc** - Add optional basedir parameter, append to begin sequence when present
2. **Add image-response OSC generator** - Single-shot and chunked variants, with MIME type and base64 data
3. **Add image-error OSC generator** - Error message in sanitized OSC format
4. **Add unit tests** - Verify basedir inclusion/omission, image-response format, chunked transfer, image-error format

**Dependencies**: None (standalone encoding module)

**Testing Approach**:
- Unit: Verify OSC output format with/without basedir, image-response single/chunked, image-error format

**Acceptance Criteria**:
- [ ] `generate_markdown_osc` with basedir produces correct begin sequence
- [ ] `generate_markdown_osc` without basedir is backward compatible
- [ ] `generate_image_response_osc` produces valid OSC for small and large images
- [ ] `generate_image_error_osc` produces valid OSC with sanitized error

**Estimated Effort**: small

---

### Phase 2: Interactive CLI Loop (Backend)

**Goal**: Extend `execute_markdown_command` to enter an interactive stdin loop when stdin is TTY, handling navigate/image/quit commands.

**Files to Modify**:
- `src-tauri/src/commands/markdown.rs` - Add TTY detection, interactive loop, command parsing, navigate/image/quit handlers

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| execute_markdown_command | Entry point with TTY detection | Valid file path | Outputs initial OSC, enters interactive loop if TTY |
| Interactive loop | Read stdin lines, dispatch commands | stdin is TTY, initial OSC sent | Exits on "quit", EOF, or error |
| navigate handler | Read .md file and output new OSC session | Absolute path to .md file | New markdown OSC session output, or error content |
| image handler | Read image, base64 encode, output response | Request ID, absolute path | image-response or image-error OSC output |
| Command parser | Parse "navigate PATH", "image REQ_ID PATH", "quit" | Raw stdin line | Structured command or ignore |

**Processing Flow**:
1. Execute existing markdown command logic (read file, generate OSC, output)
2. Compute basedir from canonical path of source file's parent directory
3. Pass basedir to generate_markdown_osc
4. Check if stdin is TTY
   - Not TTY -> exit immediately (backward compatible)
   - Is TTY -> enter interactive loop
5. Interactive loop reads stdin line by line
   - "navigate PATH" -> canonicalize path, validate .md extension, read file, compute new basedir, output new OSC session
   - "image REQ_ID PATH" -> canonicalize path, validate image extension, read file, detect MIME type from extension, base64 encode, output image-response OSC (or image-error on failure)
   - "quit" -> exit cleanly
   - EOF -> exit cleanly
   - Unknown command -> ignore (log to stderr)

**Implementation Steps**:
1. **Add basedir computation** - Derive parent directory from canonical file path
2. **Add TTY detection** - Use IsTerminal trait on stdin to decide interactive vs pipe mode
3. **Add command parser** - Parse line-based protocol (navigate, image, quit)
4. **Add navigate handler** - Read markdown file, generate new OSC session with updated basedir
5. **Add image handler** - Read image file, validate extension, detect MIME type, base64 encode, generate response OSC
6. **Add interactive loop** - Tie together stdin reading, command dispatch, and output
7. **Add unit tests** - Command parsing, path validation, MIME detection, error cases

**Dependencies**: Phase 1 (OSC generators)

**Testing Approach**:
- Unit: Command parsing, MIME type detection, path validation (extension checks)
- Integration: Full navigate flow, image flow, quit flow, pipe mode compatibility

**Acceptance Criteria**:
- [ ] Pipe mode exits immediately after initial output (backward compatible)
- [ ] Interactive mode stays alive reading stdin
- [ ] "navigate" reads file and outputs new markdown OSC session
- [ ] "image" reads file and outputs image-response OSC
- [ ] "image" for non-existent file outputs image-error OSC
- [ ] "quit" and EOF cause clean exit
- [ ] Path canonicalization prevents traversal attacks

**Estimated Effort**: medium

---

### Phase 3: Type Definitions and Session Manager (Frontend)

**Goal**: Extend TypeScript types and MarkdownSessionManager to handle basedir, image-response, and image-error verbs.

**Files to Modify**:
- `src/markdown/types.ts` - Add basedir to session/begin types, extend MarkdownVerb
- `src/markdown/session.ts` - Parse basedir, handle image-response/error verbs, accept PTY write callback
- `src/markdown/renderer.ts` - Update DOMPurify config for data: URI on img, mark local-path images with data attribute

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| MarkdownSession (type) | Hold basedir alongside existing session data | basedir in begin params | Accessible during session lifecycle |
| MarkdownVerb (type) | Include new verbs | N/A | Includes "image-response" and "image-error" |
| handleCommand (session.ts) | Route new verbs to handlers | Valid verb and params | image-response updates img src, image-error shows error |
| image-response handler | Assemble chunks if needed, set img src to data: URI | Valid request_id, matching img placeholder | img element shows loaded image |
| image-error handler | Show error on placeholder | Valid request_id, matching img placeholder | Placeholder shows error message |
| DOMPurify config (renderer.ts) | Allow data: URI for img src only | N/A | data: scheme passes for img, blocked elsewhere |
| Image markup (renderer.ts) | Mark local-path images with data-local-src | Markdown contains image with local path | img elements have data-local-src attribute, src set to placeholder |

**Processing Flow**:
1. Update types to include basedir in session and begin params, extend verb type
2. In handleBegin: extract basedir from parsed params, store in session
3. In handleCommand: route "image-response" and "image-error" to new handlers
4. image-response handler:
   - Look up pending request by request_id
   - If chunked -> accumulate chunks, assemble on final chunk
   - Find img element by data attribute matching request_id
   - Set src to "data:{mime_type};base64,{data}"
5. image-error handler:
   - Find img placeholder by request_id
   - Replace with error indicator
6. In renderer: update DOMPurify ALLOWED_URI_REGEXP to also allow `data:` scheme
7. In renderer: post-process rendered HTML to identify local-path img elements, add `data-local-src` attribute with resolved path, set `src` to transparent placeholder

**Implementation Steps**:
1. **Extend type definitions** - Add basedir to MarkdownSession and BeginParams, extend MarkdownVerb
2. **Update DOMPurify config** - Allow data: URI scheme for img src while maintaining other restrictions
3. **Add local image marking in renderer** - Post-process to identify local-path images and add data attributes
4. **Update session manager begin handler** - Parse and store basedir
5. **Add image-response/error handlers in session manager** - With chunk assembly support and DOM updates
6. **Add PTY write callback to session manager** - For sending requests to CLI

**Dependencies**: Phase 2 (CLI must be ready to respond, but frontend can be developed in parallel)

**Testing Approach**:
- Unit: basedir parsing, image-response assembly (single/chunked), image-error handling, DOMPurify data: URI allowance, local image marking

**Acceptance Criteria**:
- [ ] basedir stored in session from begin params
- [ ] image-response updates img element src to data: URI
- [ ] Chunked image-response assembles correctly
- [ ] image-error shows error on placeholder
- [ ] DOMPurify allows data: URI for img src
- [ ] DOMPurify blocks data: URI in non-img contexts (a href, etc.)
- [ ] Local-path images marked with data-local-src attribute

**Estimated Effort**: medium

---

### Phase 4: Fullscreen Viewer Navigation and Image Loading (Frontend)

**Goal**: Add .md link navigation, IntersectionObserver-based image loading, and quit command to the fullscreen viewer.

**Files to Modify**:
- `src/markdown/fullscreen.ts` - Link click handler for .md, IntersectionObserver setup, quit on close, PTY write integration

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleLinkClick (modified) | Route .md links to navigate, keep http to browser | Active fullscreen with basedir | .md links trigger navigate, http links open browser |
| IntersectionObserver setup | Observe img[data-local-src] for viewport entry | Content rendered with marked images | Visible images trigger PTY image requests |
| close (modified) | Write quit command before closing overlay | Active fullscreen with PTY write access | "quit\n" sent to PTY, then overlay removed |
| PTY write integration | Accept write callback for sending commands | PTY client available | Commands written to correct PTY session |

**Processing Flow**:
1. Modify constructor/show to accept PTY write callback and basedir
2. Modify handleLinkClick:
   - If href ends with `.md` -> resolve against basedir -> write "navigate {absolute_path}\n" to PTY
   - If href is http/https -> existing behavior (confirm dialog / external browser)
   - Otherwise -> ignore
3. After content is rendered, set up IntersectionObserver:
   - Target: all img elements with `data-local-src` attribute
   - On intersection -> generate request_id (counter-based) -> write "image {req_id} {path}\n" to PTY -> mark as pending
   - Unobserve after request sent
   - Track pending requests to avoid duplicates
4. Modify close:
   - Write "quit\n" to PTY before removing overlay
   - Only send quit if PTY write callback is available (not in non-interactive mode)
5. On navigate response (new OSC session received):
   - Session manager handles new begin/chunk/end -> calls show again with new content
   - Previous overlay is replaced

**Implementation Steps**:
1. **Add PTY write callback and basedir to fullscreen view** - Accept via show() parameters or setter
2. **Modify link click handler** - Route .md links to navigate via PTY
3. **Add IntersectionObserver for local images** - Lazy loading with request deduplication
4. **Modify close to send quit** - Write quit command to PTY on viewer close
5. **Handle navigation lifecycle** - Ensure smooth content replacement on new session

**Dependencies**: Phase 3 (types and session manager must handle new verbs)

**Testing Approach**:
- Unit: Path resolution logic (relative + basedir = absolute), link routing (.md vs http vs other)
- E2E (Docker): Open markdown with .md link, click, verify content change; scroll to image, verify load; close viewer

**Acceptance Criteria**:
- [ ] Clicking .md link writes navigate command to PTY
- [ ] Clicking http link opens external browser (unchanged)
- [ ] Images load lazily when scrolling into viewport
- [ ] Duplicate image requests are prevented
- [ ] Escape/q writes quit to PTY and closes viewer
- [ ] Navigation replaces content smoothly

**Estimated Effort**: medium

---

### Phase 5: OSC Router and WASM Updates

**Goal**: Ensure new markdown verbs (image-response, image-error) are routed correctly through the OSC handler chain.

**Files to Modify**:
- `src/terminal-app/osc-handler.ts` - Verify markdown routing handles new verbs (likely no change needed, since all markdown verbs already route to markdown manager)
- `wasm/src/osc_handler.rs` - No change expected (OSC 777 already mapped to EmtermExtension action type 100)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OSC handler routing | Route image-response/error to markdown manager | New verbs in OSC data | Session manager receives and processes new verbs |

**Processing Flow**:
1. Verify that OSC 777 with `emterm;markdown;image-response;...` flows through existing routing
   - WASM maps OSC 777 -> action type 100 (EmtermExtension) -> already done
   - osc-handler.ts case 100: routes `emterm` + `markdown` prefix to markdown manager -> already done
   - Session manager handleCommand dispatches based on verb -> Phase 3 adds image-response/image-error cases
2. If no routing changes needed, this phase is verification-only

**Implementation Steps**:
1. **Verify existing routing** - Confirm OSC 777 emterm;markdown;* reaches session manager
2. **Add integration test** - End-to-end verification of new verb routing if needed

**Dependencies**: Phases 1-4

**Testing Approach**:
- Integration: Verify image-response and image-error OSC sequences reach session manager correctly

**Acceptance Criteria**:
- [ ] image-response OSC reaches MarkdownSessionManager.handleCommand
- [ ] image-error OSC reaches MarkdownSessionManager.handleCommand
- [ ] Existing markdown begin/chunk/end routing is unaffected

**Estimated Effort**: small

---

### Phase 6: Integration Testing and Edge Cases

**Goal**: Verify end-to-end functionality, handle edge cases, and ensure backward compatibility.

**Files to Create**:
- `src-tauri/tests/integration/markdown_navigation_tests.rs` - Integration tests for interactive CLI

**Files to Modify**:
- Existing test files as needed for new test scenarios

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Integration tests | Verify CLI interactive protocol | CLI binary built | All flows work end-to-end |
| Edge case handling | Spaces in paths, large images, concurrent requests | All phases complete | Graceful handling of all edge cases |

**Implementation Steps**:
1. **Add CLI integration tests** - Test interactive loop with navigate, image, quit, EOF scenarios
2. **Add backward compatibility test** - Verify pipe mode still works
3. **Test edge cases** - Paths with spaces, deeply nested relative paths, large images, concurrent requests
4. **Run full E2E suite** - Ensure no regressions

**Dependencies**: Phases 1-5

**Testing Approach**:
- Integration: Full protocol scenarios in Rust integration tests
- E2E (Docker): Complete user workflows
- Manual: SSH testing, visual verification of image rendering

**Acceptance Criteria**:
- [ ] All existing tests pass (no regression)
- [ ] Interactive CLI protocol works end-to-end
- [ ] Pipe mode backward compatibility verified
- [ ] Edge cases handled gracefully

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
  encoding/
    osc.rs                  # + basedir in begin, image-response/error generators
  commands/
    markdown.rs             # + interactive loop, navigate/image/quit handlers

src-tauri/tests/integration/
  markdown_navigation_tests.rs  # NEW: integration tests for interactive CLI

src/markdown/
  types.ts                  # + basedir field, new verb types
  session.ts                # + basedir parsing, image-response/error handling, PTY write callback
  renderer.ts               # + data: URI in DOMPurify, local-path image marking
  fullscreen.ts             # + .md link navigation, IntersectionObserver, quit command

src/terminal-app/
  osc-handler.ts            # Verify routing (likely no changes)

wasm/src/
  osc_handler.rs            # Verify routing (likely no changes)
```

## Testing Strategy

- **Unit tests**: Core logic in Rust (OSC generation, command parsing, path validation, MIME detection) and TypeScript (basedir parsing, image response handling, DOMPurify config)
- **Integration tests**: CLI interactive protocol (navigate, image, quit, EOF, pipe mode)
- **E2E (Docker)**: Full user workflows via `./scripts/run-e2e-docker.sh`
- **Manual**: SSH testing, visual verification of image rendering quality

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| base64 | 0.22 (existing) | Base64 encoding for image data |
| uuid | 1 (existing) | Session ID generation |

No new external dependencies required.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Large image OSC exceeds parser buffer | Medium | High | Chunked transfer with configurable chunk size |
| stdin blocking on interactive loop | Low | Medium | Line-buffered reading, EOF detection |
| DOMPurify data: URI allows XSS | Low | High | Restrict to img src only via ALLOWED_URI_REGEXP |
| Race between navigate and pending image requests | Medium | Low | Discard pending requests on navigation |

## Open Questions

No unresolved questions. All requirements have status "ok" in sdd.yaml.

## Success Metrics

- [ ] All FR1-FR13 functional requirements implemented and tested
- [ ] All NFR1-NFR6 non-functional requirements met
- [ ] .md link navigation works for relative and absolute paths
- [ ] Inline images display correctly via lazy loading
- [ ] Full functionality works over SSH
- [ ] Pipe mode backward compatibility preserved
- [ ] Existing E2E tests pass without regression
