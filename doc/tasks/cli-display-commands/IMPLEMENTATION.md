# Implementation Plan: CLI Display Commands (markdown/image)

## Overview
Implement two CLI subcommands (`emterm markdown` and `emterm image`) that generate control sequences for displaying rich content in the eMterm terminal emulator. These commands output OSC sequences to stdout for inline rendering.

## Objectives
- Provide user-friendly CLI tools for displaying Markdown and images in eMterm
- Enable programmatic access from shell scripts and automation tools
- Support remote usage via SSH (sequences pass through to local terminal)
- Maintain simplicity with minimal options and sensible defaults
- Ensure robust error handling and validation

## Prerequisites

### Development Environment
- Rust 1.70+ (matches Tauri requirements)
- Cargo (latest stable)
- Bun (for project-wide tasks)

### Dependencies
**External Crates (to be added to Cargo.toml):**
- `clap` v4.x - Command-line argument parsing with derive macros
- `uuid` v1.x - UUID v4 generation for session identifiers
  - **Feature flags**: `features = ["v4"]` - Only enable v4 generation
- `base64` v0.21.x - Base64 encoding/decoding
- `image` v0.24.x - Image decoding (PNG, JPEG, GIF, WebP)
  - **Feature flags**: `default-features = false, features = ["png", "jpeg", "gif", "webp"]`
  - **Rationale**: Reduces binary size and compile time by disabling unused codecs
- `anyhow` v1.x - Error handling with context
- `thiserror` v1.x - Custom error type derivation

**SIXEL (Phase 4):**
- `sixel-rs` or custom implementation - SIXEL encoding
- **Investigation needed**: Evaluate `sixel-rs` vs `sixel-sys` vs custom implementation during Phase 4

**Dependency Optimization Notes:**
- The `image` crate is heavy; limiting features significantly reduces binary size
- `uuid` with only v4 feature avoids unnecessary MAC address or timestamp dependencies
- Consider `fast_image_resize` if runtime resizing is added in the future

### Knowledge Requirements
- Rust error handling patterns (Result, anyhow, thiserror)
- Control sequence formats (OSC, Kitty Graphics Protocol, SIXEL)
- CLI design principles (exit codes, stderr vs stdout)
- Image encoding fundamentals (formats, base64)

## Architecture Overview

### Technology Stack
- **Language**: Rust 1.70+
- **Framework**: Tauri (existing application infrastructure)
- **CLI Framework**: clap v4.x with derive macros
- **Key Libraries**:
  - `image` - Image decoding and format detection
  - `base64` - Efficient base64 encoding with SIMD support
  - `uuid` - Session identifier generation
  - `thiserror` - Custom error type derivation

### Design Approach
**Bottom-up layered architecture:**
- **Layer 1 (Foundation)**: Error types, validation utilities, encoding helpers
- **Layer 2 (Protocols)**: OSC, Kitty, SIXEL sequence generators
- **Layer 3 (Commands)**: Markdown and image command logic
- **Layer 4 (CLI)**: Argument parsing and dispatch

**Key Design Decisions:**
- Stateless commands - no caching, each invocation reads fresh from filesystem
- Output to stdout only - errors to stderr (enables piping)
- Early validation - fail fast on invalid inputs
- Protocol abstraction - clean separation between Kitty and SIXEL

**Stateless Design Trade-offs:**

| Aspect | Pros | Cons |
|--------|------|------|
| Simplicity | No state management, predictable behavior | N/A |
| Correctness | Always fresh data, no cache invalidation | N/A |
| Memory | No persistent memory usage | Each invocation loads full file |
| Performance | Single-use: acceptable | Repeated calls: no deduplication |

- **Impact**: Acceptable for single-use CLI; may be suboptimal in tight loops
- **Mitigation**: Document this limitation in user-facing docs
- **Future Optimization**: If needed, consider process-level caching or ID reuse for Kitty protocol

### Component Interaction
```
CLI Parser (clap)
    ↓
Command Dispatcher (main.rs)
    ↓
    ├─→ MarkdownCommand ──→ FileValidator ──→ Base64Encoder ──→ OscGenerator ──→ stdout
    │
    └─→ ImageCommand ──→ FileValidator ──→ ImageDecoder ──→ ProtocolGenerator ──→ stdout
                                                                  ├─→ KittyEncoder
                                                                  └─→ SixelEncoder
                                             ↓ (on error)
                                          ErrorHandler ──→ stderr ──→ exit(code)
```

## Implementation Phases

### Phase 1: Foundation Layer

**Goal**: Establish core infrastructure for error handling, validation, and encoding - all subsequent phases depend on this foundation. Success = all foundation modules compile and pass unit tests.

**Files to Create**:
- `src-tauri/src/error.rs` - Custom error types with exit code mapping
- `src-tauri/src/validation/mod.rs` - Module exports for validation utilities
- `src-tauri/src/validation/file.rs` - File path and size validation logic
- `src-tauri/src/validation/image.rs` - Image format validation and detection
- `src-tauri/src/encoding/mod.rs` - Module exports for encoding utilities
- `src-tauri/src/encoding/base64.rs` - Base64 encoding with chunking support

**Files to Modify**:
- `src-tauri/Cargo.toml`:
  - Add dependencies: `clap`, `uuid`, `base64`, `image`, `anyhow`, `thiserror`
  - Configure feature flags if needed

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| CommandError | Unified error representation with display messages and exit codes | Valid error variant constructed | Error can be displayed and mapped to exit code |
| validate_file_path | Canonicalize and verify file existence | String/Path reference | Valid PathBuf or ValidationError |
| validate_file_size | Check file size against limit | Valid file path, max size threshold | Success or FileTooLarge error |
| validate_image_format | Detect and validate image format | Valid image file path | ImageFormat enum or UnsupportedFormat error |
| encode_base64 | Encode bytes to base64 string | Byte slice | Base64-encoded String |
| chunk_data | Split data into fixed-size chunks | Data string, chunk size | Vector of data chunks |

**Processing Flow**:
```
1. Error Type Definitions
   - Define CommandError enum with variants
   - Implement Display trait for user-friendly messages
   - Map each variant to exit code (0=success, 1=general, 2=I/O)

2. File Validation Flow
   - Receive file path
   - Canonicalize path (resolve symlinks, relative paths)
   - Check file exists → ValidationError::FileNotFound if not
   - Check is file (not directory) → ValidationError::NotAFile if directory
   - Return validated PathBuf

3. Size Validation Flow
   - Read file metadata
   - Extract file size in bytes
   - Compare against limit → ValidationError::FileTooLarge if exceeded
   - Return success

4. Image Format Validation Flow
   - Detect format from file extension and magic bytes
   - Match against supported formats (PNG, JPEG, GIF, WebP)
   - Return ImageFormat or UnsupportedFormat error

5. Base64 Encoding Flow
   - Encode full content to base64
   - Split encoded string into chunks (default 64KB for markdown, 4096 bytes for Kitty)
   - Return vector of chunk strings

**Chunk Size Rationale:**
| Protocol | Chunk Size | Rationale |
|----------|------------|-----------|
| Markdown (OSC 777) | 64KB | Balance between overhead (fewer sequences) and memory (manageable buffer). Stays under typical terminal line buffer limits. |
| Kitty Graphics | 4096 bytes | Per Kitty Graphics Protocol specification. Matches typical terminal line buffer sizes. Kitty recommends 4KB chunks. |

- **Future Enhancement**: Consider making chunk size configurable via `--chunk-size` CLI flag if edge cases arise
```

**Implementation Steps**:

1. **Define Error Types**
   - Create `CommandError` enum with variants for all error cases
   - Implement `thiserror::Error` derive for automatic Display implementation
   - Add `exit_code()` method to map errors to exit codes
   - Key considerations:
     - FileNotFound, FileReadError → exit code 2 (I/O error)
     - FileTooLarge, UnsupportedFormat, InvalidProtocol → exit code 1 (general error)
   - **Error Handling Pattern (anyhow + thiserror)**:
     - Use `thiserror` for public `CommandError` enum (typed, structured errors)
     - Use `anyhow` internally for error context propagation
     - **Important**: Preserve `CommandError` at module boundaries for exit code mapping
     - Example pattern:
       ```rust
       // Internal function uses anyhow for context
       fn internal_fn() -> anyhow::Result<()> { /* ... */ }

       // Public function converts to CommandError
       pub fn command_fn() -> Result<(), CommandError> {
           internal_fn().map_err(|e| CommandError::SomeVariant(e.to_string()))?;
           Ok(())
       }
       ```
     - **Warning**: Do not wrap `CommandError` with `anyhow::Context` as it loses type information needed for exit codes

2. **Implement File Validation**
   - Create `validate_file_path` function with path validation
   - Create `validate_file_size` function with configurable limits
   - Key considerations:
     - **Path Validation Strategy**: Two approaches are available:
       - **Approach A (Simple)**: Check `path.exists()` and `path.is_file()` without canonicalization.
         - Pros: Works with non-existent paths during error handling, simpler logic
         - Cons: Potential path traversal if path is used unsafely later
       - **Approach B (Secure)**: Use `Path::canonicalize()` to resolve symlinks and relative paths.
         - Pros: Prevents path traversal, resolves symlinks consistently
         - Cons: Fails on non-existent paths, follows symlinks to unexpected locations
       - **Recommendation**: Use Approach A for validation, but ensure file is read immediately after validation (no TOCTOU risk since we read synchronously)
     - Handle symlinks: Follow them (legitimate use case), but document this behavior
     - Provide clear error messages with file path context

3. **Implement Image Format Validation**
   - Create `validate_image_format` using `image::ImageFormat::from_path`
   - Match against allowed formats (PNG, JPEG, GIF, WebP)
   - Key considerations:
     - Format detection should be robust (check magic bytes, not just extension)
     - Return specific error for unsupported formats

4. **Implement Base64 Encoding**
   - Create `encode_base64` function using `base64` crate
   - Create `chunk_data` function for splitting encoded strings
   - Key considerations:
     - Use `base64::engine::general_purpose::STANDARD` for standard encoding
     - Chunk size should be configurable (different for markdown vs image)
     - Use iterator-based chunking to minimize allocations

**Dependencies**:
- Requires: None (foundation layer)
- Blocks: Phase 2 (Markdown), Phase 3 (Image)

**Testing Approach**:

*Unit Tests*:
- `error.rs`:
  - Test exit_code() returns correct codes for each error variant
  - Test error Display messages are clear and actionable
- `validation/file.rs`:
  - Test validate_file_path with valid file → returns canonicalized path
  - Test validate_file_path with non-existent file → FileNotFound error
  - Test validate_file_path with directory → NotAFile error
  - Test validate_file_path with symlink → follows link and validates target
  - Test validate_file_size with file under limit → success
  - Test validate_file_size with file over limit → FileTooLarge error
  - Test validate_file_size with exactly at limit → success
- `validation/image.rs`:
  - Test validate_image_format with PNG → returns ImageFormat::Png
  - Test validate_image_format with JPEG → returns ImageFormat::Jpeg
  - Test validate_image_format with unsupported format → UnsupportedFormat error
- `encoding/base64.rs`:
  - Test encode_base64 with sample data → correct base64 output
  - Test chunk_data with data smaller than chunk size → single chunk
  - Test chunk_data with data larger than chunk size → multiple chunks with correct boundaries

*Integration Tests*:
- Create test fixtures directory with sample files
- Test validation pipeline: path → size → format
- Test encoding pipeline: read → encode → chunk

**Acceptance Criteria**:
- [ ] All error types compile with thiserror derive
- [ ] validate_file_path correctly canonicalizes paths and detects invalid files
- [ ] validate_file_size enforces limits (2MB for markdown, 10MB for images)
- [ ] validate_image_format correctly detects PNG, JPEG, GIF, WebP
- [ ] encode_base64 produces standard-compliant base64
- [ ] chunk_data splits data at exact boundaries
- [ ] Unit test coverage ≥ 90% for foundation layer
- [ ] All tests pass with `cargo test --lib`

**Estimated Effort**: 中 (3-4 days)

**Risks and Mitigation**:
- **Risk**: Path canonicalization behaves differently on Windows
  - **Mitigation**: Test on Windows early, use platform-agnostic path handling
- **Risk**: Image format detection library may have edge cases
  - **Mitigation**: Add comprehensive test fixtures covering common and edge cases

---

### Phase 2: Markdown Command Implementation

**Goal**: Implement complete markdown command with OSC 777 sequence generation. Success = `emterm markdown file.md` outputs valid sequences that render in eMterm terminal.

**Files to Create**:
- `src-tauri/src/commands/mod.rs` - Module exports for command implementations
- `src-tauri/src/commands/markdown.rs` - Markdown command logic and execution
- `src-tauri/src/encoding/osc.rs` - OSC 777 sequence generation utilities
- `src-tauri/tests/integration/markdown_tests.rs` - Integration tests for markdown command
- `src-tauri/tests/fixtures/sample.md` - Small test markdown file
- `src-tauri/tests/fixtures/large.md` - Near-limit (2MB) test markdown file

**Files to Modify**:
- `src-tauri/src/main.rs`:
  - Add clap CLI structure with Markdown subcommand
  - Implement command dispatcher to route to markdown execution
  - Add error handling and exit code logic

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| execute_markdown_command | Orchestrate markdown processing pipeline | Valid file path | OSC sequences written to stdout or error |
| generate_markdown_osc | Create OSC 777 sequence with UUID and chunks | Base64 chunks, UUID | Complete OSC sequence string |
| output_to_stdout | Write sequence to stdout with proper flushing | Valid sequence string | Bytes written to stdout |

**Processing Flow**:
```
1. Command Entry
   - Receive file path from CLI arguments
   - Invoke execute_markdown_command

2. Validation Phase
   - Validate file path exists and is readable
   - Validate file size ≤ 2MB
   → If validation fails, return error

3. File Reading
   - Read entire file into byte buffer
   - Handle I/O errors

4. UUID Generation
   - Generate UUID v4 for session identifier
   - Convert to string format

5. Encoding Phase
   - Encode file bytes to base64
   - Split into chunks (64KB each)

6. OSC Generation
   - Emit begin sequence with UUID, format=gfm, render=block, version=1.0
   - Emit chunk sequences with UUID, seq number, base64 data
   - Emit end sequence with UUID

7. Output
   - Write complete sequence to stdout
   - Flush stdout buffer
   - Return success
```

**Implementation Steps**:

1. **Create OSC Generator**
   - Implement `generate_markdown_osc` function in `encoding/osc.rs`
   - Generate begin, chunk, and end sequences per spec format
   - Key considerations:
     - Use exact format from spec: `\x1b]777;emterm;markdown;...`
     - Ensure proper escaping and termination (`\x1b\`)
     - Include all required parameters (id, format, render, version, seq, data)

2. **Implement Markdown Command**
   - Create `execute_markdown_command` in `commands/markdown.rs`
   - Orchestrate: validate → read → encode → generate → output
   - Key considerations:
     - Use foundation layer validators (Phase 1)
     - Propagate errors with context using `anyhow`
     - Generate fresh UUID for each invocation

3. **Implement Output Handler**
   - Create `output_to_stdout` helper
   - Use `std::io::stdout()` with lock and flush
   - Key considerations:
     - Lock stdout for atomic write
     - Flush buffer after write to ensure immediate output
     - Handle write errors gracefully

4. **Integrate CLI Parsing**
   - Add clap Parser and Subcommand definitions in `main.rs`
   - Add Markdown subcommand with file argument
   - Route parsed command to `execute_markdown_command`
   - Key considerations:
     - Use derive macros for clean CLI definition
     - Provide clear help messages
     - Map Result to exit codes

**Dependencies**:
- Requires: Phase 1 (foundation layer must be complete)
- Blocks: None (independent from image command)

**Testing Approach**:

*Unit Tests*:
- `encoding/osc.rs`:
  - Test generate_markdown_osc with single chunk → correct begin/chunk/end format
  - Test generate_markdown_osc with multiple chunks → sequential seq numbers
  - Test UUID appears in all sequences
  - Test format, render, version parameters are correct
- `commands/markdown.rs`:
  - Test execute_markdown_command with valid small file → success
  - Test execute_markdown_command with oversized file → FileTooLarge error
  - Test execute_markdown_command with non-existent file → FileNotFound error

*Integration Tests*:
- Test Case 1: Small markdown file (1KB)
  - Verify OSC sequence structure
  - Verify base64 decodes back to original content
  - Verify exit code 0
- Test Case 2: Medium markdown file (100KB)
  - Verify chunking occurs correctly
  - Verify sequential chunk numbers
  - Verify exit code 0
- Test Case 3: File at limit (exactly 2MB)
  - Verify processing succeeds
  - Verify exit code 0
- Test Case 4: File over limit (2MB + 1 byte)
  - Verify error message mentions size limit
  - Verify exit code 1
- Test Case 5: Non-existent file
  - Verify error message includes file path
  - Verify exit code 2
- Test Case 6: Empty file (0 bytes)
  - Verify minimal valid sequence emitted
  - Verify exit code 0

**Acceptance Criteria**:
- [ ] `emterm markdown sample.md` outputs valid OSC 777 sequences
- [ ] All sequences include consistent UUID
- [ ] Large files are chunked at 64KB boundaries
- [ ] File size validation enforces 2MB limit
- [ ] Error messages go to stderr, sequences to stdout
- [ ] Exit code 0 on success, 1 for general errors, 2 for I/O errors
- [ ] Integration tests pass for all test cases
- [ ] Manual test: `emterm markdown README.md` renders in eMterm terminal

**Estimated Effort**: 中 (3-4 days)

**Risks and Mitigation**:
- **Risk**: OSC sequence format may have subtle formatting errors
  - **Mitigation**: Test with actual eMterm terminal early, validate against spec
- **Risk**: Large file chunking may have off-by-one errors
  - **Mitigation**: Comprehensive unit tests with boundary cases

---

### Phase 3: Image Command Implementation (Kitty Protocol)

**Goal**: Implement image command with Kitty Graphics Protocol support for PNG, JPEG, GIF, and WebP. Success = `emterm image photo.png` displays inline in eMterm terminal.

**Files to Create**:
- `src-tauri/src/commands/image.rs` - Image command logic and execution
- `src-tauri/src/protocols/mod.rs` - Module exports for protocol encoders
- `src-tauri/src/protocols/kitty.rs` - Kitty Graphics Protocol encoder
- `src-tauri/tests/integration/image_tests.rs` - Integration tests for image command
- `src-tauri/tests/fixtures/small.png` - Small PNG test image
- `src-tauri/tests/fixtures/photo.jpg` - JPEG test image
- `src-tauri/tests/fixtures/animation.gif` - GIF test image
- `src-tauri/tests/fixtures/graphic.webp` - WebP test image

**Files to Modify**:
- `src-tauri/src/main.rs`:
  - Add Image subcommand to CLI with file and protocol arguments
  - Add protocol argument parsing (kitty/sixel)
  - Route to image command execution

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| execute_image_command | Orchestrate image processing pipeline | Valid file path, protocol selection | Kitty/SIXEL sequences to stdout or error |
| decode_image | Load and decode image from file | Valid image file path | DynamicImage or decoding error |
| generate_kitty_sequence | Encode image to Kitty Graphics Protocol | Decoded image | Kitty sequence string with chunked base64 PNG |
| ImageProtocol enum | Represent protocol selection | - | Type-safe protocol selection |

**Processing Flow**:
```
1. Command Entry
   - Receive file path and protocol from CLI arguments
   - Invoke execute_image_command

2. Validation Phase
   - Validate file path exists and is readable
   - Validate file size ≤ 10MB
   → If validation fails, return error

3. Image Decoding
   - Detect image format from file
   - Validate format is supported (PNG, JPEG, GIF, WebP)
   - Decode image to in-memory representation
   → If decode fails, return error

4. Protocol Selection
   - Branch on protocol parameter:
     - Protocol::Kitty → generate_kitty_sequence
     - Protocol::Sixel → generate_sixel_sequence (Phase 4)

5. Kitty Encoding (Phase 3 scope)
   - Convert image to PNG format in memory
   - Base64 encode PNG bytes
   - Split into 4096-byte chunks
   - Generate Kitty sequence:
     - First chunk: metadata (f=100, a=T, m=1) + data
     - Middle chunks: m=1 + data
     - Last chunk: m=0 + data

6. Output
   - Write sequence to stdout
   - Flush buffer
   - Return success
```

**Implementation Steps**:

1. **Define Image Protocol Enum**
   - Create `ImageProtocol` enum in `commands/image.rs`
   - Variants: Kitty, Sixel
   - Derive Debug, Clone, Copy
   - Key considerations:
     - Simple enum for type-safe protocol selection
     - Will be extended in Phase 4 with Sixel

2. **Implement Image Decoder**
   - Create `decode_image` function using `image` crate
   - Detect format, validate support, load image
   - Key considerations:
     - Use `image::ImageFormat::from_path` for format detection
     - Match against allowed formats explicitly
     - Return `DynamicImage` for protocol flexibility

3. **Implement Kitty Protocol Generator**
   - Create `generate_kitty_sequence` in `protocols/kitty.rs`
   - Convert image to PNG, base64 encode, chunk, format sequences
   - Key considerations:
     - Use `image::DynamicImage::write_to` with PNG format
     - Chunk at exactly 4096 bytes (Kitty spec)
     - First chunk has `f=100,a=T,m=1` parameters
     - Middle chunks have `m=1` (more data)
     - Last chunk has `m=0` (end of transmission)
     - Use `\x1b_G` prefix and `\x1b\` suffix

4. **Implement Image Command**
   - Create `execute_image_command` in `commands/image.rs`
   - Orchestrate: validate → decode → generate protocol → output
   - Key considerations:
     - Use Phase 1 validators for file validation
     - Dispatch to correct protocol generator
     - Provide context in errors (file path, format)

5. **Integrate CLI for Image Command**
   - Add Image subcommand to main.rs
   - Add `--protocol` option with default "kitty"
   - Parse protocol string to ImageProtocol enum
   - Key considerations:
     - Default to Kitty protocol
     - Validate protocol string (only "kitty" in Phase 3, add "sixel" in Phase 4)
     - Clear help text for options

**Dependencies**:
- Requires: Phase 1 (foundation layer must be complete)
- Blocks: Phase 4 (SIXEL support depends on this structure)

**Testing Approach**:

*Unit Tests*:
- `protocols/kitty.rs`:
  - Test generate_kitty_sequence with small image (< 4096 bytes) → single chunk with m=0
  - Test generate_kitty_sequence with large image → multiple chunks with correct m flags
  - Test first chunk contains f=100, a=T, m=1
  - Test last chunk has m=0
  - Test base64 encoding is valid
- `commands/image.rs`:
  - Test decode_image with PNG → successful decode
  - Test decode_image with JPEG → successful decode
  - Test decode_image with unsupported format → error
  - Test execute_image_command with valid image → success
  - Test execute_image_command with oversized image → FileTooLarge error

*Integration Tests*:
- Test Case 1: PNG image (Kitty protocol)
  - Verify Kitty sequence format
  - Verify base64 data is valid
  - Verify exit code 0
- Test Case 2: JPEG image (Kitty protocol)
  - Verify conversion to PNG occurs
  - Verify sequence is valid
  - Verify exit code 0
- Test Case 3: GIF image (Kitty protocol)
  - Verify first frame is extracted and encoded
  - Verify exit code 0
- Test Case 4: WebP image (Kitty protocol)
  - Verify decoding and encoding
  - Verify exit code 0
- Test Case 5: File at limit (exactly 10MB)
  - Verify processing succeeds
  - Verify exit code 0
- Test Case 6: File over limit (10MB + 1 byte)
  - Verify error message
  - Verify exit code 1
- Test Case 7: Unsupported format (e.g., BMP or PDF)
  - Verify UnsupportedFormat error
  - Verify exit code 1
- Test Case 8: Tiny image (1x1 pixel)
  - Verify valid sequence
  - Verify exit code 0

**Acceptance Criteria**:
- [ ] `emterm image photo.png` outputs valid Kitty Graphics Protocol sequences
- [ ] Supports PNG, JPEG, GIF, WebP formats
- [ ] File size validation enforces 10MB limit
- [ ] Kitty sequences have correct chunking (4096 bytes)
- [ ] First chunk has metadata (f=100, a=T, m=1)
- [ ] Last chunk has m=0 flag
- [ ] Integration tests pass for all image formats
- [ ] Manual test: `emterm image test.png` displays in eMterm terminal

**Estimated Effort**: 中 (4-5 days)

**Risks and Mitigation**:
- **Risk**: Image format conversion may lose quality or fail for edge cases
  - **Mitigation**: Test with diverse images (grayscale, transparency, EXIF data)
- **Risk**: Kitty protocol chunking may have boundary errors
  - **Mitigation**: Unit tests with images of various sizes (just under, at, just over chunk boundary)
- **Risk**: Large image memory usage may be excessive
  - **Mitigation**: Monitor memory usage in tests, ensure ≤ 3x file size

---

### Phase 4: SIXEL Protocol Support

**Goal**: Add SIXEL protocol support as alternative to Kitty for image display. Success = `emterm image --protocol=sixel photo.png` outputs valid SIXEL sequences.

**Files to Create**:
- `src-tauri/src/protocols/sixel.rs` - SIXEL encoder implementation
- `src-tauri/tests/integration/sixel_tests.rs` - SIXEL-specific integration tests

**Files to Modify**:
- `src-tauri/Cargo.toml`:
  - Add SIXEL dependency (`sixel-rs` or chosen library)
- `src-tauri/src/commands/image.rs`:
  - Add Sixel branch in protocol dispatch
- `src-tauri/src/main.rs`:
  - Update protocol argument parser to accept "sixel"

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| generate_sixel_sequence | Encode image to SIXEL format | Decoded image | SIXEL sequence string wrapped in DCS |

**Processing Flow**:
```
1. Image Command Entry (same as Phase 3)
   - Receive file path and protocol="sixel"

2. Validation and Decoding (same as Phase 3)
   - Validate file, decode image

3. SIXEL Encoding
   - Convert image to RGB8 format
   - Encode to SIXEL data using library or custom encoder
   - Wrap in SIXEL DCS: \x1bPq{data}\x1b\

4. Output (same as Phase 3)
   - Write to stdout, flush, return success
```

**Implementation Steps**:

1. **Evaluate SIXEL Libraries**
   - Research available Rust SIXEL encoders (`sixel-rs`, `libsixel` bindings)
   - Select based on: ease of use, maintenance, dependencies
   - Key considerations:
     - Prefer pure Rust if available
     - Check for WebP/modern format support
     - Evaluate encoding performance

2. **Implement SIXEL Generator**
   - Create `generate_sixel_sequence` in `protocols/sixel.rs`
   - Convert image to RGB8, encode to SIXEL, wrap in DCS
   - Key considerations:
     - SIXEL uses DCS (Device Control String): `\x1bPq` ... `\x1b\`
     - May need color palette quantization for optimal output
     - Handle transparency (if library supports it)

3. **Integrate SIXEL into Image Command**
   - Add `ImageProtocol::Sixel` dispatch in `execute_image_command`
   - Update protocol argument parser in main.rs to accept "sixel"
   - Key considerations:
     - Use same validation and decoding pipeline
     - Only protocol generation differs

**Dependencies**:
- Requires: Phase 3 (image command structure must exist)
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- `protocols/sixel.rs`:
  - Test generate_sixel_sequence with small RGB image → valid SIXEL output
  - Test SIXEL sequence starts with `\x1bPq` and ends with `\x1b\`
  - Test grayscale image → correct SIXEL encoding

*Integration Tests*:
- Test Case 1: PNG with `--protocol=sixel`
  - Verify SIXEL sequence format
  - Verify exit code 0
- Test Case 2: JPEG with `--protocol=sixel`
  - Verify conversion and SIXEL encoding
  - Verify exit code 0
- Test Case 3: GIF with `--protocol=sixel`
  - Verify first frame encoded to SIXEL
  - Verify exit code 0
- Test Case 4: Invalid protocol option
  - `--protocol=ascii` → error message, exit code 1

**Acceptance Criteria**:
- [ ] `emterm image --protocol=sixel photo.png` outputs valid SIXEL sequences
- [ ] SIXEL sequences are properly wrapped in DCS (`\x1bPq` ... `\x1b\`)
- [ ] All image formats work with SIXEL (PNG, JPEG, GIF, WebP)
- [ ] Protocol selection via `--protocol` option works correctly
- [ ] Integration tests pass for SIXEL output
- [ ] Manual test: SIXEL sequences display in SIXEL-capable terminal

**Estimated Effort**: 小 (2-3 days)

**Risks and Mitigation**:
- **Risk**: SIXEL library may be unmaintained or have bugs
  - **Mitigation**: Evaluate multiple options, be prepared for custom implementation if needed
- **Risk**: SIXEL color quantization may produce poor quality
  - **Mitigation**: Test with diverse images, consider making palette size configurable

---

### Phase 5: Integration, Testing & Polish

**Goal**: Achieve production-ready quality through comprehensive testing, performance benchmarking, documentation, and cross-platform validation. Success = all tests pass, benchmarks meet goals, works on Linux/macOS/Windows.

**Files to Create**:
- `src-tauri/tests/fixtures/` - Additional test fixtures (edge cases)
- `src-tauri/benches/command_benchmarks.rs` - Performance benchmarks
- Documentation in rustdoc comments for all public APIs

**Files to Modify**:
- All source files:
  - Add rustdoc comments
  - Refine error messages
  - Add inline documentation
- `README.md` or dedicated CLI docs:
  - Add usage examples
  - Document all commands and options

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| E2E test suite | Validate end-to-end workflows in actual eMterm | Compiled binaries | Full user scenarios verified |
| Performance benchmarks | Measure and validate performance goals | All commands implemented | Performance meets NFR goals |

**Processing Flow**:
```
1. Test Completeness Review
   - Verify unit test coverage ≥ 80% for all modules
   - Verify integration tests cover all test scenarios from spec
   - Add missing tests for edge cases

2. Performance Benchmarking
   - Benchmark small files (< 100KB) → < 50ms
   - Benchmark medium files (100KB - 1MB) → < 200ms
   - Benchmark large files (up to limits) → < 500ms/1s
   - Measure memory usage ≤ 3x file size

3. E2E Testing with eMterm
   - Test markdown rendering in actual terminal
   - Test image display in actual terminal
   - Test script integration and piping
   - Test remote usage via SSH

4. Cross-Platform Testing
   - Test on Linux (primary development platform)
   - Test on macOS (path handling, terminal compatibility)
   - Test on Windows (path handling, line endings)

5. Documentation Review
   - Add rustdoc to all public functions
   - Document error cases
   - Create usage examples
   - Update CLI help text

6. Error Message Refinement
   - Review all error messages for clarity
   - Ensure actionable guidance (e.g., "File too large (5MB). Maximum size is 2MB.")
   - Consistent formatting and tone
```

**Implementation Steps**:

1. **Complete Test Coverage**
   - Review coverage reports (`cargo tarpaulin` or similar)
   - Add missing unit tests for edge cases
   - Add integration tests for error paths
   - Key considerations:
     - Target ≥ 80% coverage overall, ≥ 90% for critical paths
     - Focus on error handling paths (often under-tested)
     - Test boundary conditions (size limits, chunk boundaries)

2. **Implement Performance Benchmarks**
   - Create benchmark suite using `cargo bench` (criterion)
   - Benchmark markdown and image commands with various file sizes
   - Measure memory usage during processing
   - Key considerations:
     - Use consistent test fixtures
     - Run multiple iterations for statistical significance
     - Document baseline performance on reference hardware

3. **Conduct E2E Testing**
   - Test markdown rendering in eMterm with various documents
   - Test image display with various formats and sizes
   - Test error scenarios in actual terminal
   - Test script usage (loops, piping, exit code handling)
   - Key considerations:
     - Verify rendering quality (no corruption)
     - Verify error messages appear correctly in terminal
     - Test both local and remote (SSH) usage

4. **Cross-Platform Validation**
   - Run full test suite on Linux, macOS, Windows
   - Validate path handling (Windows backslashes, long paths)
   - Validate terminal compatibility
   - Key considerations:
     - Use GitHub Actions or similar CI for automated multi-platform testing
     - Test with different shells (bash, zsh, PowerShell)
     - Verify control sequences work in different terminals

5. **Documentation and Polish**
   - Add rustdoc comments to all public APIs
   - Create usage examples in comments and docs
   - Refine error messages for clarity and helpfulness
   - Update help text and CLI descriptions
   - Key considerations:
     - Examples should be copy-paste ready
     - Error messages should include what went wrong and how to fix it
     - Document security considerations (path handling, size limits)

**Dependencies**:
- Requires: Phases 1-4 (all features implemented)
- Blocks: None (final phase)

**Testing Approach**:

*E2E Test Scenarios*:
- Scenario 1: Display README.md in eMterm
  - Verify Markdown renders with correct formatting (headings, lists, code blocks)
- Scenario 2: Display PNG screenshot in eMterm
  - Verify image appears inline at correct size
- Scenario 3: Use in shell script to display multiple files
  - Verify all files display in sequence without errors
- Scenario 4: Pipe output to non-eMterm terminal
  - Verify sequences are ignored gracefully (no errors, no display)
- Scenario 5: Run on remote server via SSH
  - Verify content displays in local eMterm

*Performance Tests*:
- Load test: Process 100 small files (< 10KB each) in loop → < 5s total
- Stress test: Process maximum size markdown (2MB) → < 500ms
- Stress test: Process maximum size image (10MB) → < 1s
- Memory test: Ensure memory ≤ 3x file size during processing

*Edge Case Tests*:
- Symbolic link to markdown/image file → follows link and processes
- Relative path with `..` components → resolves correctly
- Markdown with Unicode (emoji, CJK) → encodes correctly
- Grayscale image → converts and displays correctly
- Animated GIF → displays (animation behavior TBD)
- Image with EXIF orientation → respects orientation
- Very long file path (> 260 chars on Windows) → handles correctly

**Acceptance Criteria**:
- [ ] Unit test coverage ≥ 80% overall, ≥ 90% for validation and encoding
- [ ] All integration tests pass on Linux, macOS, Windows
- [ ] E2E tests verify actual rendering in eMterm
- [ ] Performance benchmarks meet NFR goals (< 50ms for small, < 500ms/1s for large)
- [ ] Memory usage ≤ 3x file size verified in tests
- [ ] All error messages reviewed and refined
- [ ] Rustdoc comments complete for all public APIs
- [ ] CLI help text is clear and complete
- [ ] No compiler warnings
- [ ] `cargo clippy` passes with no warnings

**Estimated Effort**: 中 (4-5 days)

**Risks and Mitigation**:
- **Risk**: Performance goals may not be met on slower hardware
  - **Mitigation**: Profile and optimize hot paths, consider async I/O if needed
- **Risk**: Cross-platform issues may be discovered late
  - **Mitigation**: Test on all platforms continuously via CI
- **Risk**: E2E testing may reveal rendering issues in eMterm frontend
  - **Mitigation**: Coordinate with frontend team, adjust sequence format if needed

---

## Complete File Structure

```
src-tauri/
├── src/
│   ├── main.rs                          # CLI entry point, clap parsing, command dispatch
│   ├── error.rs                         # CommandError type with exit code mapping
│   ├── commands/
│   │   ├── mod.rs                       # Command module exports
│   │   ├── markdown.rs                  # Markdown command execution logic
│   │   └── image.rs                     # Image command execution logic, ImageProtocol enum
│   ├── encoding/
│   │   ├── mod.rs                       # Encoding module exports
│   │   ├── base64.rs                    # Base64 encoding and chunking utilities
│   │   └── osc.rs                       # OSC 777 sequence generation for markdown
│   ├── protocols/
│   │   ├── mod.rs                       # Protocol module exports
│   │   ├── kitty.rs                     # Kitty Graphics Protocol encoder
│   │   └── sixel.rs                     # SIXEL protocol encoder
│   └── validation/
│       ├── mod.rs                       # Validation module exports
│       ├── file.rs                      # File path canonicalization, size validation
│       └── image.rs                     # Image format detection and validation
├── tests/
│   ├── integration/
│   │   ├── markdown_tests.rs            # Integration tests for markdown command
│   │   ├── image_tests.rs               # Integration tests for image command (Kitty)
│   │   └── sixel_tests.rs               # Integration tests for SIXEL protocol
│   └── fixtures/
│       ├── sample.md                    # Small markdown test file (< 1KB)
│       ├── large.md                     # Near-limit markdown file (~2MB)
│       ├── small.png                    # Small PNG test image
│       ├── photo.jpg                    # JPEG test image
│       ├── animation.gif                # GIF test image
│       ├── graphic.webp                 # WebP test image
│       └── (additional edge case files)
├── benches/
│   └── command_benchmarks.rs            # Performance benchmarks (cargo bench)
├── Cargo.toml                           # Dependencies and project metadata
└── README.md                            # (or CLI documentation)
```

**File Relationships:**
- `main.rs` imports and dispatches to `commands/markdown.rs` and `commands/image.rs`
- Command modules depend on `validation/` for input validation
- Command modules depend on `encoding/` and `protocols/` for output generation
- All modules use `error.rs` for error handling
- Test files depend on `fixtures/` for test data

## Testing Strategy

### Unit Testing

**Approach**:
- Use Rust's built-in `testing` framework with `#[cfg(test)]` modules
- Table-driven tests for multiple scenarios (test data in arrays)
- Mock filesystem operations where needed (use `tempfile` crate for temp files)

**Test Coverage Goals**:
- Overall: ≥ 80% line coverage
- Validation modules: ≥ 90% (critical for security)
- Encoding modules: ≥ 90% (critical for correctness)
- Command modules: ≥ 80% (orchestration logic)
- Protocol modules: ≥ 85% (format correctness)

**Key Test Areas**:

1. **Error Handling** (`error.rs`)
   - Each error variant displays correctly
   - Exit codes map correctly to error types

2. **File Validation** (`validation/file.rs`)
   - Path canonicalization (symlinks, relative paths)
   - File existence checks
   - Directory vs file detection
   - Size validation at boundaries (under, at, over limit)

3. **Image Validation** (`validation/image.rs`)
   - Format detection (PNG, JPEG, GIF, WebP)
   - Unsupported format rejection
   - Corrupted file handling

4. **Base64 Encoding** (`encoding/base64.rs`)
   - Correct encoding (compare against reference)
   - Chunking at boundaries (single chunk, multiple chunks, exact boundary)

5. **OSC Generation** (`encoding/osc.rs`)
   - Sequence format correctness
   - UUID inclusion in all sequences
   - Parameter correctness (format, render, version)
   - Sequential chunk numbering

6. **Kitty Protocol** (`protocols/kitty.rs`)
   - Metadata in first chunk (f=100, a=T, m=1)
   - Continuation flag in middle chunks (m=1)
   - Termination flag in last chunk (m=0)
   - Base64 encoding validity

7. **SIXEL Protocol** (`protocols/sixel.rs`)
   - DCS wrapping (\x1bPq ... \x1b\)
   - Valid SIXEL data format

### Integration Testing

**Scenarios**:

**Markdown Command:**
1. Small file (< 1KB) → valid output, exit 0
2. Medium file (100KB) → chunked output, exit 0
3. File at limit (2MB) → success, exit 0
4. File over limit (2MB+1) → error, exit 1
5. Non-existent file → error, exit 2
6. Empty file (0 bytes) → minimal output, exit 0
7. File without read permission → error, exit 2

**Image Command:**
1. PNG with Kitty → valid output, exit 0
2. JPEG with Kitty → valid output, exit 0
3. GIF with Kitty → valid output, exit 0
4. WebP with Kitty → valid output, exit 0
5. PNG with SIXEL → valid output, exit 0
6. Protocol option validation → error on invalid
7. File at limit (10MB) → success, exit 0
8. File over limit (10MB+1) → error, exit 1
9. Unsupported format → error, exit 1
10. Corrupted image → error, exit 1

**Approach**:
- Use `assert_cmd` crate for command-line testing
- Create test fixtures in `tests/fixtures/`
- Verify stdout contains expected sequences (pattern matching)
- Verify stderr contains expected errors
- Verify exit codes

### Manual Testing Checklist

**Based on spec test scenarios:**
- [ ] Display README.md in eMterm - Markdown renders correctly with formatting
- [ ] Display PNG screenshot in eMterm - Image appears inline at correct size
- [ ] Use in shell script to display multiple files - All files display in sequence
- [ ] Pipe output to non-eMterm terminal - Sequences ignored gracefully
- [ ] Run on remote server via SSH - Content displays in local eMterm
- [ ] Symbolic link to file - Follows link and processes target
- [ ] Relative path with `..` - Resolves correctly
- [ ] Markdown with Unicode (emoji, CJK) - Encodes correctly
- [ ] Grayscale image - Displays correctly
- [ ] Animated GIF - Displays (first frame or animation)
- [ ] Image with EXIF orientation - Respects orientation
- [ ] Very long file path (> 260 chars) - Handles correctly (Windows)

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| clap | v4.x | Command-line argument parsing with derive macros | `cargo add clap --features derive` |
| uuid | v1.x | UUID v4 generation for session identifiers | `cargo add uuid --features v4` |
| base64 | v0.21.x | Base64 encoding/decoding with SIMD support | `cargo add base64` |
| image | v0.24.x | Image decoding (PNG, JPEG, GIF, WebP) | `cargo add image` |
| anyhow | v1.x | Error handling with context | `cargo add anyhow` |
| thiserror | v1.x | Custom error type derivation | `cargo add thiserror` |
| sixel-rs | (TBD) | SIXEL encoding (Phase 4) | (Research in Phase 4) |

**Development Dependencies:**
| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| assert_cmd | latest | Command-line testing | `cargo add --dev assert_cmd` |
| predicates | latest | Assertion helpers for tests | `cargo add --dev predicates` |
| tempfile | latest | Temporary file creation for tests | `cargo add --dev tempfile` |
| criterion | latest | Performance benchmarking | `cargo add --dev criterion` |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Foundation (no dependencies)
2. Phase 2: Markdown (depends on Phase 1)
3. Phase 3: Image/Kitty (depends on Phase 1)
4. Phase 4: SIXEL (depends on Phase 3)
5. Phase 5: Integration (depends on Phases 1-4)

**Module Dependencies:**
- `commands/markdown.rs` depends on: `validation/file.rs`, `encoding/base64.rs`, `encoding/osc.rs`, `error.rs`
- `commands/image.rs` depends on: `validation/file.rs`, `validation/image.rs`, `protocols/kitty.rs`, `protocols/sixel.rs`, `error.rs`
- `encoding/osc.rs` depends on: `encoding/base64.rs`
- `protocols/kitty.rs` depends on: `encoding/base64.rs`
- All modules depend on: `error.rs`

## Risk Assessment

### Technical Risks

1. **Image Decoding Performance**
   - **Risk**: Large image decoding may exceed time goals
   - **Likelihood**: Medium
   - **Impact**: Medium (degrades UX, violates NFR)
   - **Mitigation**:
     - Profile image decoding performance early
     - Consider lazy decoding or streaming for very large images
     - Optimize PNG encoding (use fast compression level)
     - Document performance characteristics

2. **SIXEL Library Availability**
   - **Risk**: No suitable Rust SIXEL library available
   - **Likelihood**: Medium
   - **Impact**: High (Phase 4 blocked)
   - **Mitigation**:
     - Research SIXEL libraries in Phase 1
     - Prepare fallback: custom SIXEL encoder (simpler than full spec)
     - Consider FFI bindings to libsixel if necessary

3. **Cross-Platform Path Handling**
   - **Risk**: Path canonicalization behaves differently on Windows
   - **Likelihood**: Low (Rust stdlib handles this)
   - **Impact**: High (security vulnerability)
   - **Mitigation**:
     - Test on Windows early (Phase 1)
     - Use platform-agnostic path handling (`std::path::Path`)
     - Add comprehensive path tests for Windows edge cases

4. **Memory Usage with Large Files**
   - **Risk**: Processing 10MB images may use excessive memory
   - **Likelihood**: Low
   - **Impact**: Medium (violates NFR)
   - **Mitigation**:
     - Monitor memory usage in benchmarks
     - Use streaming or chunked processing if needed
     - Enforce strict size limits (already in spec)

### Implementation Risks

1. **Scope Creep**
   - **Risk**: Adding features beyond spec (e.g., image resizing, caching)
   - **Mitigation**: Strictly adhere to spec, document future enhancements separately

2. **Test Coverage Gaps**
   - **Risk**: Edge cases not covered by tests leading to bugs
   - **Mitigation**: Systematic test planning, coverage measurement, peer review

3. **Integration Issues with eMterm Frontend**
   - **Risk**: Sequence format incompatibility with terminal renderer
   - **Mitigation**: Coordinate with frontend team early, test E2E in Phase 1-2

## Performance Considerations

### Performance Goals (from NFR)
- **NFR1**: Files < 100KB → < 50ms end-to-end
- **NFR2**: Files up to 2MB → < 500ms end-to-end
- **Memory**: ≤ 3x file size during processing

### Optimization Strategies

1. **Efficient Encoding**
   - Use `base64` crate with SIMD support for fast encoding
   - Minimize memory allocations (use iterators for chunking)
   - Avoid intermediate copies (use references and slices)

2. **Image Processing**
   - Use `image` crate's efficient decoding
   - Use fast PNG compression level for Kitty encoding
   - Consider palette reduction for SIXEL (balances quality and size)

3. **I/O Optimization**
   - Read file in one operation (already small enough to fit in memory)
   - Use buffered stdout writing with single flush
   - Avoid unnecessary filesystem metadata queries

4. **Memory Management**
   - Reuse buffers where possible
   - Limit allocations in hot paths
   - Profile with `valgrind` or `heaptrack` to identify leaks or excessive allocations

### Benchmarking Plan
- Benchmark file reading with various sizes
- Benchmark base64 encoding with various data sizes
- Benchmark image decoding and re-encoding (PNG)
- Benchmark full end-to-end command execution
- Compare against baseline (simple `cat` command for reference)

## Security Considerations

### Path Traversal Prevention
- **Strategy**: Use `Path::canonicalize()` to resolve all paths before accessing
- **Validation**: Ensure file exists and is a file (not directory or special device)
- **Symlinks**: Follow symlinks (users may legitimately use them), but canonicalization prevents escaping accessible paths

### Input Validation
- **File Paths**: Validate and canonicalize before use
- **Protocol Options**: Whitelist-based validation (only "kitty" or "sixel")
- **Size Limits**: Enforce strict limits to prevent resource exhaustion (2MB markdown, 10MB images)

### Memory Safety
- **Rust Guarantees**: Ownership system prevents buffer overflows, use-after-free
- **Bounds Checking**: All array/vector accesses are bounds-checked by Rust

### Memory Optimization Strategy

**Current Approach (Phase 1-4)**:
- Read entire file into memory
- Encode to base64 (increases size ~1.33x)
- Chunk and output

**Memory Profile**:
```
File (N bytes) + Decoded Image (M bytes) + Base64 (N*1.33 bytes) ≈ 3x file size peak
```

**Streaming Optimization (Future Enhancement)**:
If NFR memory tests fail, consider streaming approach:
- Use `BufReader` for file reading
- Streaming base64 encoder (encode chunks directly)
- Output chunks immediately to stdout

**Benefits of Streaming**:
- Memory bounded by chunk size, not file size
- Reduces peak from 3x to ~1.1x file size
- Better for very large files

**Decision**: Start with simple approach; optimize only if NFR tests fail.

### Output Safety
- **No Code Execution**: Output is pure data (base64 encoded), no executable code in sequences
- **Terminal Injection**: Control sequences are well-formed and predictable (no user-controlled data in sequence structure, only in base64 payload)

### Threat Model
| Threat | Mitigation |
|--------|------------|
| Malicious file paths (path traversal) | Path canonicalization, existence checks |
| Oversized files (DoS) | Size validation before processing |
| Malformed images (crashes) | Robust image library with error handling |
| Resource exhaustion (memory) | Size limits, bounded memory usage |
| Symlink attacks | Canonicalization resolves symlinks safely |

## Open Questions

### From Specification:
- [ ] Should we add a `--verbose` flag for debugging output?
- [ ] Do we need progress indication for large files?
- [ ] Should image command support automatic resizing to terminal width?
- [ ] Should we add a `--dry-run` option to validate without outputting?
- [ ] Do we need configuration file support for defaults?

### Implementation-Specific:
- [ ] Which SIXEL library should we use? (Research in Phase 4)
- [ ] Should we optimize PNG encoding level for Kitty protocol (speed vs size)?
- [ ] How to handle animated GIFs? (First frame only, or full animation data?)
- [ ] Should we respect EXIF orientation in images? (Requires additional processing)

### To Clarify with User:
- [ ] Are there any additional image formats to support beyond PNG, JPEG, GIF, WebP?
- [ ] Should we support stdin input (piped data) in addition to files?
- [ ] Do we need colorized error messages for better UX?

## Future Enhancements

Items deferred to later phases or releases:

### Not in Current Spec:
- Configuration file support for default protocol, size limits
- Verbose/debug mode for troubleshooting
- Progress indicators for large file processing
- Automatic image resizing to fit terminal width
- Support for additional image formats (BMP, TIFF, etc.)
- Stdin input support (pipe data directly to command)
- Dry-run mode to validate files without outputting sequences
- Batch processing mode (process multiple files in one invocation)
- Caching of processed sequences for repeated use

## Success Metrics

### Functional Completeness
- [ ] All functional requirements (FR1-FR7) implemented
- [ ] All user stories have acceptance criteria met
- [ ] All test scenarios pass

### Quality Metrics
- [ ] Unit test coverage ≥ 80% (≥ 90% for validation/encoding)
- [ ] All integration tests pass on Linux, macOS, Windows
- [ ] No critical or high-severity bugs in manual testing
- [ ] Code follows Rust best practices (clippy, rustfmt)

### Performance Metrics
- [ ] Small files (< 100KB) process in < 50ms
- [ ] Large files (up to limits) process in < 500ms (markdown) / 1s (images)
- [ ] Memory usage ≤ 3x file size during processing

### User Experience
- [ ] Clear, actionable error messages
- [ ] Help text is comprehensive and accurate
- [ ] Commands work intuitively without reading docs
- [ ] Works seamlessly in scripts and automation

## References

- **Specification**: `/home/sakura/cache/worktrees/emterm/feature-cli-display-commands/doc/tasks/cli-display-commands/SPEC.md`
- **Kitty Graphics Protocol**: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- **SIXEL Specification**: https://vt100.net/docs/vt3xx-gp/chapter14.html
- **GitHub Flavored Markdown**: https://github.github.com/gfm/
- **Rust Documentation**:
  - clap: https://docs.rs/clap/
  - image: https://docs.rs/image/
  - base64: https://docs.rs/base64/
  - uuid: https://docs.rs/uuid/
  - thiserror: https://docs.rs/thiserror/
  - anyhow: https://docs.rs/anyhow/

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Review plan with team/stakeholders
   - Address open questions and ambiguities
   - Confirm approach and timeline estimates

2. **Environment Setup**
   - Ensure Rust toolchain is up to date
   - Install any platform-specific dependencies
   - Setup CI for cross-platform testing

3. **Begin Implementation**
   - Start with Phase 1 (foundation layer)
   - Follow test-driven approach (write tests first)
   - Commit incrementally with clear messages

4. **Continuous Integration**
   - Setup CI pipeline (GitHub Actions or similar)
   - Run tests automatically on each commit
   - Enforce code quality checks (clippy, rustfmt)
