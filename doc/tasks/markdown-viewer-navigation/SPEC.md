# Feature: Markdown Viewer Navigation

## Overview

Extend the `emterm markdown` CLI command and fullscreen Markdown viewer to support navigating relative/absolute `.md` links and displaying inline images referenced by local paths. The CLI enters an interactive mode (when stdin is TTY) to serve on-demand requests for file content and images over PTY, enabling full functionality over SSH.

## Objectives

- Enable clicking `.md` links in the viewer to navigate to other Markdown files
- Display images referenced by relative/absolute paths inline in the viewer
- Maintain SSH compatibility (all communication via PTY)
- Preserve backward compatibility for pipe mode (one-shot exit)

## User Stories

### US1: Navigate to Linked Markdown File
As a developer, I want to click a `.md` link in the Markdown viewer, so that I can browse related documentation without leaving the terminal.

**Acceptance Criteria:**
- [ ] Clicking a relative `.md` link replaces the viewer content with the linked file
- [ ] Clicking an absolute `.md` link replaces the viewer content with the linked file
- [ ] http/https links continue to open in the external browser
- [ ] Non-existent `.md` links show an error in the viewer

### US2: View Inline Images
As a developer, I want to see images referenced in Markdown displayed inline, so that I can read documentation with visual content.

**Acceptance Criteria:**
- [ ] Images with relative paths are displayed inline
- [ ] Images with absolute paths are displayed inline
- [ ] Images are loaded lazily (only when scrolled into viewport)
- [ ] Failed image loads show an error placeholder
- [ ] No size limit on images

### US3: Interactive CLI Session
As a user, I want the CLI to stay alive and serve requests while I browse, so that navigation and images work without re-running the command.

**Acceptance Criteria:**
- [ ] CLI enters interactive mode when stdin is TTY
- [ ] CLI exits immediately when stdin is pipe (backward compatible)
- [ ] CLI exits cleanly on QUIT command or stdin close
- [ ] Works over SSH

## Technical Requirements

### Functional Requirements
- **FR1:** Add `basedir` parameter to OSC 777 markdown begin sequence
- **FR2:** CLI enters interactive request loop when stdin is TTY
- **FR3:** CLI handles `navigate PATH` command: read file, output new OSC session
- **FR4:** CLI handles `image REQ_ID PATH` command: read file, output image-response/image-error OSC
- **FR5:** CLI handles `quit` command: exit cleanly
- **FR6:** CLI exits on stdin close (EOF)
- **FR7:** Frontend resolves relative paths against basedir and writes navigate request to PTY
- **FR8:** Frontend uses IntersectionObserver to detect images entering viewport and writes image request to PTY
- **FR9:** Frontend handles image-response OSC: set img src to data: URI
- **FR10:** Frontend handles image-error OSC: show error on placeholder
- **FR11:** Frontend writes QUIT to PTY on viewer close (Escape/q)
- **FR12:** DOMPurify allows `data:` URI scheme for img src attribute
- **FR13:** CLI detects pipe mode and maintains one-shot backward compatibility

### Non-Functional Requirements
- **NFR1 - Performance:** Lazy image loading via IntersectionObserver to avoid loading all images at once
- **NFR2 - Security:** Path canonicalization on CLI side to prevent path traversal
- **NFR3 - Security:** DOMPurify data: URI allowance restricted to img src only
- **NFR4 - Compatibility:** All communication via PTY (SSH transparent)
- **NFR5 - Compatibility:** Backward compatible with existing pipe usage
- **NFR6 - Reliability:** Graceful shutdown on stdin close / SSH disconnect

## Implementation Approach

### Architecture

**Request-Response Flow over PTY:**
```
┌──────────────────────┐              ┌──────────────────────┐
│  Frontend (WebView)  │              │  CLI Process (Rust)  │
│                      │              │                      │
│  FullscreenView      │  PTY stdin   │  Interactive Loop    │
│  ───────────────►    │─────────────►│  ───────────────►    │
│  navigate/image/quit │              │  Read command        │
│                      │              │                      │
│  ◄───────────────    │◄─────────────│  ◄───────────────    │
│  OSC session/        │  PTY stdout  │  Read file,          │
│  image-response      │   (OSC 777)  │  Generate OSC        │
└──────────────────────┘              └──────────────────────┘
```

**Component Interactions:**
```
FullscreenMarkdownView
    ├── Link click → resolve path → PtyClient.write("navigate PATH\n")
    ├── IntersectionObserver → PtyClient.write("image REQ_ID PATH\n")
    └── Close (Esc/q) → PtyClient.write("quit\n")

MarkdownSessionManager
    ├── Parse basedir from begin params
    ├── Handle image-response → find placeholder → set data: URI
    └── Handle image-error → find placeholder → show error

CLI Interactive Loop (markdown.rs)
    ├── Read stdin line by line
    ├── "navigate PATH" → read .md file → output new OSC session
    ├── "image REQ_ID PATH" → read file → output image-response/image-error OSC
    ├── "quit" → exit(0)
    └── EOF → exit(0)
```

### Data Flow

**Markdown Navigation:**
```
User clicks .md link
  → FullscreenView resolves path (basedir + href)
  → FullscreenView writes "navigate /absolute/path/to/file.md\n" to PTY
  → CLI reads command, reads file, generates new markdown OSC session
  → Frontend parser receives new begin/chunk/end
  → MarkdownSessionManager replaces viewer content
```

**Image Loading:**
```
Image placeholder enters viewport
  → IntersectionObserver fires
  → FullscreenView writes "image req123 /absolute/path/to/image.png\n" to PTY
  → CLI reads command, reads file, base64 encodes
  → CLI outputs: ESC]777;emterm;markdown;image-response;request_id=req123;mime_type=image/png;data=...ESC\
  → Frontend parser routes to MarkdownSessionManager
  → Session manager finds placeholder by request_id, sets src to data: URI
```

**Viewer Close:**
```
User presses Escape/q
  → FullscreenView writes "quit\n" to PTY
  → CLI reads "quit", exits cleanly
  → FullscreenView closes overlay
```

### OSC Protocol Extensions

#### Modified: markdown begin

Add `basedir` parameter:
```
ESC ] 777 ; emterm ; markdown ; begin ; id={uuid} ; format=gfm ; render=fullscreen ; version=1.0 ; basedir={path} ESC \
```

- `basedir`: Absolute path of the source file's parent directory
- Value is sanitized (semicolons and control characters removed)

#### New: image-response

```
ESC ] 777 ; emterm ; markdown ; image-response ; request_id={id} ; mime_type={type} ; data={base64} ESC \
```

- `request_id`: Matches the ID from the image request
- `mime_type`: MIME type (e.g., `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/svg+xml`)
- `data`: Base64-encoded image data

For large images, chunked transfer:
```
ESC ] 777 ; emterm ; markdown ; image-response ; request_id={id} ; mime_type={type} ; chunk_seq=0 ; chunk_total=N ; data={base64} ESC \
ESC ] 777 ; emterm ; markdown ; image-response ; request_id={id} ; chunk_seq=1 ; chunk_total=N ; data={base64} ESC \
...
```

#### New: image-error

```
ESC ] 777 ; emterm ; markdown ; image-error ; request_id={id} ; error={message} ESC \
```

- `request_id`: Matches the ID from the image request
- `error`: Error description (sanitized for OSC)

### PTY Stdin Command Protocol

Text-based line protocol, one command per line:

| Command | Format | Description |
|---------|--------|-------------|
| navigate | `navigate PATH\n` | Navigate to a .md file (absolute path) |
| image | `image REQ_ID PATH\n` | Request image data (absolute path) |
| quit | `quit\n` | Exit the CLI process |

- PATH is always absolute (Frontend resolves relative paths before sending)
- REQ_ID is a unique identifier generated by Frontend (e.g., `img-{counter}`)

### Changes to Existing Files

#### Rust CLI: `src-tauri/src/commands/markdown.rs`

- Add `basedir` computation (parent directory of canonical file path)
- Pass `basedir` to `generate_markdown_osc`
- After outputting OSC, check if stdin is TTY (`atty::is(atty::Stream::Stdin)` or `std::io::IsTerminal`)
- If TTY: enter interactive loop reading lines from stdin
- Handle `navigate`, `image`, `quit` commands
- On `navigate PATH`: validate path, read file, generate new OSC session with same basedir logic
- On `image REQ_ID PATH`: validate path, read file, detect MIME type, base64 encode, output image-response OSC (or image-error on failure)
- On `quit` or EOF: exit

#### Rust OSC: `src-tauri/src/encoding/osc.rs`

- Modify `generate_markdown_osc` to accept optional `basedir: Option<&str>` parameter
- Add `basedir={path}` to begin sequence when present
- Add `generate_image_response_osc(request_id, mime_type, base64_data)` function
- Add `generate_image_error_osc(request_id, error_message)` function
- Handle chunked image-response for large images

#### Frontend: `src/markdown/types.ts`

- Add `basedir` to `MarkdownSession` interface
- Add `basedir` to `BeginParams` interface
- Extend `MarkdownVerb` type: `"begin" | "chunk" | "end" | "image-response" | "image-error"`

#### Frontend: `src/markdown/session.ts`

- Parse `basedir` from begin params, store in session
- Add handler for `image-response` verb: find img placeholder by `request_id`, assemble chunks if needed, set `src` to `data:{mime_type};base64,{data}`
- Add handler for `image-error` verb: find img placeholder by `request_id`, display error

#### Frontend: `src/markdown/fullscreen.ts`

- Modify `handleLinkClick`: if `href` ends with `.md`, resolve against `basedir`, write `navigate` to PTY
- Add `IntersectionObserver` setup for img elements with local path `src` attributes
- On intersection: generate request_id, write `image REQ_ID PATH` to PTY, mark as loading
- On close: write `quit\n` to PTY before closing overlay
- Accept a PTY write callback (or `PtyClient` reference) for writing to PTY stdin

#### Frontend: `src/markdown/renderer.ts`

- Update `ALLOWED_URI_REGEXP` in DOMPurify config to allow `data:` URI scheme
- Mark local-path images with a `data-local-src` attribute and set `src` to a placeholder during rendering

#### WASM Parser: `wasm/src/osc_handler.rs`

- Route `image-response` and `image-error` verbs through the existing EmtermExtension callback mechanism (same as `markdown` verb routing)

### File Structure (Changes)

```
src-tauri/src/
├── commands/
│   └── markdown.rs          # + Interactive loop, navigate/image/quit handlers
├── encoding/
│   └── osc.rs               # + basedir param, image-response/image-error generators

src/markdown/
├── types.ts                 # + basedir field, new verb types
├── session.ts               # + basedir parsing, image-response/error handling
├── fullscreen.ts            # + .md link navigation, IntersectionObserver, quit command
├── renderer.ts              # + data: URI in DOMPurify, local-path image marking

wasm/src/
└── osc_handler.rs           # + route new verbs (if needed)
```

### Dependencies

**Internal Dependencies:**
- `src/markdown/session.ts`: Needs access to PtyClient write function
- `src/markdown/fullscreen.ts`: Needs access to PtyClient write function and basedir from session
- `src-tauri/src/commands/markdown.rs`: Uses existing `encoding/osc.rs` and `encoding/base64.rs`

**External Dependencies:**
- `atty` crate or `std::io::IsTerminal` (Rust 1.70+): TTY detection for stdin
- `mime_guess` crate (optional): MIME type detection from file extension

## Test Scenarios

### Unit Tests
- [ ] `generate_markdown_osc` with basedir parameter includes basedir in begin sequence
- [ ] `generate_markdown_osc` without basedir omits basedir from begin sequence
- [ ] `generate_image_response_osc` produces correct OSC format
- [ ] `generate_image_error_osc` produces correct OSC format
- [ ] Path resolution: relative path + basedir = correct absolute path
- [ ] Command parsing: "navigate /path/to/file.md\n" parsed correctly
- [ ] Command parsing: "image req1 /path/to/img.png\n" parsed correctly
- [ ] Command parsing: "quit\n" parsed correctly
- [ ] MIME type detection from file extension (png, jpg, gif, webp, svg)
- [ ] DOMPurify config allows data: URI in img src
- [ ] DOMPurify config blocks data: URI in other contexts (a href, etc.)

### Integration Tests
- [ ] Full navigate flow: write navigate command → receive new markdown OSC session
- [ ] Full image flow: write image command → receive image-response OSC
- [ ] Image error flow: write image command for non-existent file → receive image-error OSC
- [ ] Quit flow: write quit command → process exits with code 0
- [ ] Pipe mode: stdin is pipe → process exits after initial output
- [ ] Multiple navigations in sequence
- [ ] Multiple concurrent image requests

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with WebdriverIO + tauri-driver
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] Scenario 1: Open markdown with .md link, click link, verify content changes
- [ ] Scenario 2: Open markdown with local image, scroll to image, verify image loads
- [ ] Scenario 3: Close viewer with Escape, verify CLI process exits

### Edge Cases
- [ ] Circular links: A.md links to B.md links to A.md (no history stack, just replaces)
- [ ] Image with spaces in path
- [ ] Deeply nested relative paths (../../images/foo.png)
- [ ] Symlinked files
- [ ] Very large image file (>10MB)
- [ ] Image with unknown extension
- [ ] Multiple images entering viewport simultaneously
- [ ] Navigate while image requests are pending
- [ ] CLI process killed externally while viewer is open

## Security Considerations

### Communication Channel Safety

The interactive mode uses **stdin occupation** (like `less`/`vim`): the `emterm markdown` process runs in the foreground and owns stdin. Frontend writes requests to PTY, which are delivered to the process's stdin — not to the shell. This avoids shell injection risks entirely because:

- The shell is suspended while a foreground process is reading stdin
- The CLI process parses commands with its own protocol, not via shell evaluation
- No shell metacharacters (`;`, `$()`, `` ` ``, `|`, `&&`, `>`) are interpreted
- On process exit (quit/EOF/SIGINT), stdin returns to the shell normally

### Lifecycle Safety

| Scenario | Behavior |
|----------|----------|
| Esc/q in viewer | Frontend writes `quit\n` → process exits → shell prompt returns |
| Process hangs | User presses Ctrl+C → SIGINT terminates process → shell prompt returns |
| SSH disconnect | stdin EOF → process exits cleanly |
| Ctrl+Z (suspend) | Process is suspended, viewer remains visible. `fg` resumes. No data loss |

### Input Validation (CLI side)

- **Command parsing:** Only `navigate`, `image`, `quit` are recognized. Unknown commands are silently ignored (logged to stderr)
- **Path canonicalization:** All paths are resolved via `std::fs::canonicalize()` before file access. Symlinks are resolved. Non-existent paths return an error response
- **Path traversal:** No directory restriction enforced (user already has shell access to the same filesystem). Canonicalization prevents `..` tricks but does not sandbox
- **File type validation (navigate):** Only files with `.md` extension are accepted
- **File type validation (image):** MIME type is derived from file extension. Only known image types (`png`, `jpg`, `jpeg`, `gif`, `webp`, `svg`, `bmp`, `ico`) are accepted

### Output Safety (Frontend side)

- **DOMPurify:** `data:` URI allowed only for `img[src]` via `ALLOWED_URI_REGEXP` update. All other DOMPurify restrictions remain unchanged
- **XSS Prevention:** Image data is set via `img.src = "data:..."` attribute assignment (not innerHTML). Markdown content continues through DOMPurify sanitization
- **Request ID validation:** Frontend-generated request IDs are alphanumeric with prefix (`img-{counter}`). Responses with unrecognized request IDs are discarded
- **Resource Protection:** Frontend tracks pending requests to avoid duplicate requests for the same image

## Error Handling

### Error Scenarios

| Scenario | Handler | Response |
|----------|---------|----------|
| Navigate: file not found | CLI | Output markdown OSC with error content |
| Navigate: read permission denied | CLI | Output markdown OSC with error content |
| Image: file not found | CLI | Output image-error OSC |
| Image: read permission denied | CLI | Output image-error OSC |
| Image: invalid/corrupt file | CLI | Output image-error OSC |
| Invalid command format | CLI | Ignore (log to stderr) |
| stdin close / EOF | CLI | Exit cleanly |
| PTY write failure | Frontend | Log error, no retry |

## Performance Optimization

### Strategies
- **Lazy loading:** IntersectionObserver ensures only visible images are requested
- **Request deduplication:** Frontend tracks in-flight requests to avoid duplicate image loads
- **Chunked transfer:** Large images split into chunks to avoid single oversized OSC sequences
- **No caching:** Each navigation loads fresh content (stateless design)

## Success Criteria

- [ ] All functional requirements (FR1-FR13) are implemented and tested
- [ ] All non-functional requirements (NFR1-NFR6) are met
- [ ] .md link navigation works for relative and absolute paths
- [ ] Inline images display correctly for relative and absolute paths
- [ ] Full functionality works over SSH
- [ ] Pipe mode backward compatibility preserved
- [ ] Existing E2E tests pass without regression
- [ ] Security: path traversal attacks are prevented
- [ ] Security: DOMPurify restrictions maintained except for img data: URI

## Open Questions

> **Note**: No unresolved requirements at this time.

## References

- CLI Display Commands spec: `doc/tasks/cli-display-commands/SPEC.md`
- Existing OSC implementation: `src-tauri/src/encoding/osc.rs`
- Existing markdown CLI: `src-tauri/src/commands/markdown.rs`
- Existing fullscreen viewer: `src/markdown/fullscreen.ts`
- Existing session manager: `src/markdown/session.ts`
- DOMPurify config: `src/markdown/renderer.ts`
