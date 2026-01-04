# Feature: CLI Display Commands (markdown/image)

## Overview

This feature implements two CLI subcommands (`emterm markdown` and `emterm image`) that generate control sequences for displaying rich content (Markdown and images) in the eMterm terminal emulator. These commands enable users to easily display formatted documents and images inline within the terminal by outputting appropriate OSC (Operating System Command) sequences to stdout.

## Objectives

- Provide user-friendly CLI tools for displaying Markdown and images in eMterm
- Enable programmatic access from shell scripts and automation tools
- Support remote usage via SSH (sequences pass through to local terminal)
- Maintain simplicity with minimal options and sensible defaults
- Ensure robust error handling and validation

## User Stories

### US1: Display Markdown File
As a developer, I want to display a Markdown file in my terminal, so that I can read documentation without leaving the command line.

**Acceptance Criteria:**
- [ ] Command accepts a file path as argument
- [ ] Outputs valid OSC 777 sequences for Markdown rendering
- [ ] Supports GitHub Flavored Markdown (tables, task lists, etc.)
- [ ] Rejects files larger than 2MB with clear error message
- [ ] Returns exit code 0 on success

### US2: Display Image File
As a user, I want to display an image file in my terminal, so that I can view visual content without switching to an image viewer.

**Acceptance Criteria:**
- [ ] Command accepts an image file path as argument
- [ ] Supports PNG, JPEG, GIF, and WebP formats
- [ ] Outputs Kitty Graphics Protocol sequences by default
- [ ] Allows protocol selection via `--protocol` option
- [ ] Rejects files larger than 10MB with clear error message
- [ ] Returns exit code 0 on success

### US3: Script Integration
As a developer, I want to use these commands in shell scripts, so that I can automate rich content display in my workflows.

**Acceptance Criteria:**
- [ ] Commands work reliably in non-interactive mode
- [ ] Error messages go to stderr (stdout remains clean)
- [ ] Exit codes allow proper error handling in scripts
- [ ] No interactive prompts or TTY requirements

### US4: Remote Display via SSH
As a remote user, I want to display content from an SSH session, so that files on remote servers appear in my local terminal.

**Acceptance Criteria:**
- [ ] OSC sequences pass through SSH unchanged
- [ ] Works identically on local and remote systems
- [ ] No server-side rendering required

## Technical Requirements

### Functional Requirements
- **FR1:** Parse command-line arguments using a standard CLI framework (e.g., clap)
- **FR2:** Read file contents from the filesystem with proper error handling
- **FR3:** Validate file size before processing
- **FR4:** Generate base64-encoded OSC sequences for Markdown
- **FR5:** Generate Kitty Graphics Protocol or SIXEL sequences for images
- **FR6:** Output sequences to stdout, errors to stderr
- **FR7:** Return appropriate exit codes (0=success, 1=general error, 2=I/O error)

### Non-Functional Requirements
- **NFR1 - Performance:** Process files < 100KB in under 50ms
- **NFR2 - Performance:** Process files up to 2MB in under 500ms
- **NFR3 - Security:** Normalize file paths to prevent path traversal
- **NFR4 - Security:** Validate all user inputs
- **NFR5 - Usability:** Provide clear, actionable error messages
- **NFR6 - Compatibility:** Work on Linux, macOS, and Windows
- **NFR7 - Maintainability:** Include comprehensive unit and integration tests

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│     CLI Argument Parser (clap)      │
├─────────────────────────────────────┤
│      Command Dispatcher             │
│  ┌────────────┬──────────────┐     │
│  │  markdown  │    image     │     │
│  └────────────┴──────────────┘     │
├─────────────────────────────────────┤
│      File Operations Layer          │
│  - Read file                        │
│  - Validate size                    │
│  - Decode image (image only)        │
├─────────────────────────────────────┤
│     Encoding & Generation Layer     │
│  - Base64 encoding                  │
│  - OSC sequence generation          │
│  - Kitty/SIXEL generation           │
├─────────────────────────────────────┤
│      Output Layer (stdout)          │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
CLI Entry Point (main.rs)
    ├── MarkdownCommand
    │   ├── FileReader
    │   ├── SizeValidator
    │   ├── Base64Encoder
    │   └── OscGenerator
    │
    └── ImageCommand
        ├── FileReader
        ├── SizeValidator
        ├── ImageDecoder
        └── ProtocolGenerator
            ├── KittyEncoder
            └── SixelEncoder
```

### Data Flow

```
User Command → Parse Args → Validate File Path → Read File →
Validate Size → Encode Content → Generate Sequence →
Output to stdout → Exit
                      ↓ (on error)
                Error to stderr → Exit with error code
```

### Command-Line Interface Design

#### Command 1: emterm markdown

**Syntax:**
```bash
emterm markdown <FILE>
```

**Arguments:**
- `<FILE>` - Path to Markdown file (required)

**Options:**
- `-h, --help` - Display help message

**Output Format (OSC 777 Sequence):**
```
ESC ] 777 ; emterm ; markdown ; begin ; id={uuid} ; format=gfm ; render=block ; version=1.0 ESC \
ESC ] 777 ; emterm ; markdown ; chunk ; id={uuid} ; seq=0 ; data={base64} ESC \
ESC ] 777 ; emterm ; markdown ; chunk ; id={uuid} ; seq=1 ; data={base64} ESC \
...
ESC ] 777 ; emterm ; markdown ; end ; id={uuid} ESC \
```

**Parameter Details:**
- `id`: UUID v4 (randomly generated session identifier)
- `format`: Always "gfm" (GitHub Flavored Markdown)
- `render`: Always "block" (block-level rendering)
- `version`: Always "1.0" (protocol version)
- `seq`: Zero-based chunk sequence number
- `data`: Base64-encoded Markdown content (max 64KB per chunk)

**Example Usage:**
```bash
# Display README
emterm markdown README.md

# Use in script
emterm markdown /path/to/doc.md

# Remote via SSH
ssh user@remote "emterm markdown ~/notes.md"
```

**Error Cases:**
```bash
# File not found
$ emterm markdown missing.md
Error: File not found: missing.md
(exit code: 2)

# File too large
$ emterm markdown huge.md
Error: File size exceeds 2MB limit
(exit code: 1)

# Permission denied
$ emterm markdown /root/secret.md
Error: Failed to read file: /root/secret.md
(exit code: 2)
```

#### Command 2: emterm image

**Syntax:**
```bash
emterm image [OPTIONS] <FILE>
```

**Arguments:**
- `<FILE>` - Path to image file (required)

**Options:**
- `--protocol <PROTOCOL>` - Image protocol to use [default: kitty] [possible values: kitty, sixel]
- `-h, --help` - Display help message

**Output Format (Kitty Graphics Protocol):**
```
ESC ] _G f=100,a=T,m=1 ; {base64-chunk-1} ESC \
ESC ] _G m=1 ; {base64-chunk-2} ESC \
...
ESC ] _G m=0 ; {base64-chunk-last} ESC \
```

**Output Format (SIXEL):**
```
ESC P q {sixel-data} ESC \
```

**Example Usage:**
```bash
# Display image (default Kitty protocol)
emterm image photo.png

# Use SIXEL protocol
emterm image --protocol=sixel graph.png

# Display JPEG
emterm image screenshot.jpg

# Use in script
for img in *.png; do
  emterm image "$img"
done
```

**Error Cases:**
```bash
# File not found
$ emterm image missing.png
Error: File not found: missing.png
(exit code: 2)

# Unsupported format
$ emterm image document.pdf
Error: Unsupported image format
(exit code: 1)

# File too large
$ emterm image huge.jpg
Error: File size exceeds 10MB limit
(exit code: 1)

# Invalid protocol
$ emterm image --protocol=ascii art.png
Error: Invalid protocol: ascii
(exit code: 1)
```

### File Structure

```
src-tauri/
├── src/
│   ├── main.rs                    # CLI entry point, argument parsing
│   ├── commands/
│   │   ├── mod.rs                 # Command module exports
│   │   ├── markdown.rs            # Markdown command implementation
│   │   └── image.rs               # Image command implementation
│   ├── encoding/
│   │   ├── mod.rs                 # Encoding module exports
│   │   ├── base64.rs              # Base64 encoding utilities
│   │   └── osc.rs                 # OSC sequence generation
│   ├── protocols/
│   │   ├── mod.rs                 # Protocol module exports
│   │   ├── kitty.rs               # Kitty Graphics Protocol encoder
│   │   └── sixel.rs               # SIXEL encoder
│   ├── validation/
│   │   ├── mod.rs                 # Validation module exports
│   │   ├── file.rs                # File path and size validation
│   │   └── image.rs               # Image format validation
│   └── error.rs                   # Error types and handling
├── tests/
│   ├── integration/
│   │   ├── markdown_tests.rs      # Integration tests for markdown command
│   │   └── image_tests.rs         # Integration tests for image command
│   └── fixtures/
│       ├── sample.md              # Test Markdown file
│       ├── small.png              # Small test image
│       ├── large.md               # Near-limit Markdown file
│       └── various formats...     # PNG, JPEG, GIF, WebP samples
└── Cargo.toml                     # Dependencies
```

### Dependencies

**Internal Dependencies:**
- Shares code with eMterm main application (common utilities)

**External Dependencies (Rust crates):**
- `clap` (v4.x) - Command-line argument parsing with derive macros
- `uuid` (v1.x) - UUID v4 generation
- `base64` (v0.21.x) - Base64 encoding/decoding
- `image` (v0.24.x) - Image decoding (PNG, JPEG, GIF, WebP)
- `anyhow` (v1.x) - Error handling with context
- `thiserror` (v1.x) - Custom error type derivation

**SIXEL Dependencies:**
- `sixel-rs` or custom implementation - SIXEL encoding

### Detailed Implementation Specifications

#### Markdown Command Implementation

**Function Signature:**
```rust
pub fn execute_markdown_command(file_path: &Path) -> Result<(), CommandError>
```

**Processing Steps:**

1. **Validate File Path**
   ```rust
   fn validate_file_path(path: &Path) -> Result<PathBuf, ValidationError> {
       // Canonicalize path (resolve symlinks, relative paths)
       let canonical = path.canonicalize()?;

       // Check file exists
       if !canonical.exists() {
           return Err(ValidationError::FileNotFound(path.to_owned()));
       }

       // Check is file (not directory)
       if !canonical.is_file() {
           return Err(ValidationError::NotAFile(path.to_owned()));
       }

       Ok(canonical)
   }
   ```

2. **Validate File Size**
   ```rust
   fn validate_file_size(path: &Path, max_size: u64) -> Result<(), ValidationError> {
       let metadata = fs::metadata(path)?;
       let size = metadata.len();

       if size > max_size {
           return Err(ValidationError::FileTooLarge {
               size,
               max_size
           });
       }

       Ok(())
   }
   ```

3. **Read File Content**
   ```rust
   fn read_file_content(path: &Path) -> Result<Vec<u8>, std::io::Error> {
       fs::read(path)
   }
   ```

4. **Generate UUID**
   ```rust
   use uuid::Uuid;

   let session_id = Uuid::new_v4();
   ```

5. **Encode to Base64 and Chunk**
   ```rust
   fn encode_and_chunk(content: &[u8], chunk_size: usize) -> Vec<String> {
       use base64::{Engine as _, engine::general_purpose};

       let encoded = general_purpose::STANDARD.encode(content);

       // Split into chunks (default: 64KB)
       encoded
           .as_bytes()
           .chunks(chunk_size)
           .map(|chunk| String::from_utf8_lossy(chunk).to_string())
           .collect()
   }
   ```

6. **Generate OSC Sequences**
   ```rust
   fn generate_markdown_osc(
       session_id: &Uuid,
       chunks: Vec<String>
   ) -> String {
       let mut output = String::new();

       // Begin sequence
       output.push_str(&format!(
           "\x1b]777;emterm;markdown;begin;id={};format=gfm;render=block;version=1.0\x1b\\",
           session_id
       ));

       // Chunk sequences
       for (seq, data) in chunks.iter().enumerate() {
           output.push_str(&format!(
               "\x1b]777;emterm;markdown;chunk;id={};seq={};data={}\x1b\\",
               session_id, seq, data
           ));
       }

       // End sequence
       output.push_str(&format!(
           "\x1b]777;emterm;markdown;end;id={}\x1b\\",
           session_id
       ));

       output
   }
   ```

7. **Output to stdout**
   ```rust
   use std::io::{self, Write};

   fn output_sequence(sequence: &str) -> io::Result<()> {
       let stdout = io::stdout();
       let mut handle = stdout.lock();
       handle.write_all(sequence.as_bytes())?;
       handle.flush()?;
       Ok(())
   }
   ```

#### Image Command Implementation

**Function Signature:**
```rust
pub fn execute_image_command(
    file_path: &Path,
    protocol: ImageProtocol
) -> Result<(), CommandError>
```

**Image Protocol Enum:**
```rust
#[derive(Debug, Clone, Copy)]
pub enum ImageProtocol {
    Kitty,
    Sixel,
}
```

**Processing Steps:**

1. **Validate File** (same as markdown)

2. **Decode Image**
   ```rust
   use image::{DynamicImage, ImageFormat};

   fn decode_image(path: &Path) -> Result<DynamicImage, ImageError> {
       // Detect format
       let format = ImageFormat::from_path(path)?;

       // Validate supported formats
       match format {
           ImageFormat::Png | ImageFormat::Jpeg |
           ImageFormat::Gif | ImageFormat::WebP => {},
           _ => return Err(ImageError::UnsupportedFormat(format)),
       }

       // Load and decode
       let img = image::open(path)?;
       Ok(img)
   }
   ```

3. **Generate Protocol Sequence**
   ```rust
   fn generate_image_sequence(
       img: &DynamicImage,
       protocol: ImageProtocol
   ) -> Result<String, EncodingError> {
       match protocol {
           ImageProtocol::Kitty => generate_kitty_sequence(img),
           ImageProtocol::Sixel => generate_sixel_sequence(img),
       }
   }
   ```

4. **Kitty Protocol Generation**
   ```rust
   fn generate_kitty_sequence(img: &DynamicImage) -> Result<String, EncodingError> {
       use base64::{Engine as _, engine::general_purpose};

       // Convert to PNG bytes
       let mut png_bytes = Vec::new();
       img.write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)?;

       // Base64 encode
       let encoded = general_purpose::STANDARD.encode(&png_bytes);

       // Split into chunks (4096 bytes per chunk for Kitty)
       let chunks: Vec<_> = encoded.as_bytes().chunks(4096).collect();
       let mut output = String::new();

       // First chunk with metadata
       output.push_str(&format!(
           "\x1b_Gf=100,a=T,m=1;{}\x1b\\",
           String::from_utf8_lossy(chunks[0])
       ));

       // Middle chunks
       for chunk in &chunks[1..chunks.len()-1] {
           output.push_str(&format!(
               "\x1b_Gm=1;{}\x1b\\",
               String::from_utf8_lossy(chunk)
           ));
       }

       // Last chunk
       if chunks.len() > 1 {
           output.push_str(&format!(
               "\x1b_Gm=0;{}\x1b\\",
               String::from_utf8_lossy(chunks[chunks.len()-1])
           ));
       }

       Ok(output)
   }
   ```

5. **SIXEL Protocol Generation**
   ```rust
   fn generate_sixel_sequence(img: &DynamicImage) -> Result<String, EncodingError> {
       // Convert to RGB8
       let rgb_img = img.to_rgb8();

       // Encode to SIXEL
       // (Implementation depends on chosen SIXEL library)
       let sixel_data = encode_to_sixel(&rgb_img)?;

       // Wrap in SIXEL sequence
       let output = format!("\x1bPq{}\x1b\\", sixel_data);

       Ok(output)
   }
   ```

### Error Handling

**Error Type Definition:**
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Failed to read file: {0}")]
    FileReadError(#[from] std::io::Error),

    #[error("File size exceeds {max_size} limit")]
    FileTooLarge { size: u64, max_size: u64 },

    #[error("Unsupported image format")]
    UnsupportedImageFormat,

    #[error("Failed to decode image: {0}")]
    ImageDecodeError(#[from] image::ImageError),

    #[error("Invalid protocol: {0}")]
    InvalidProtocol(String),

    #[error("Encoding error: {0}")]
    EncodingError(String),
}
```

**Error Code Mapping:**
```rust
impl CommandError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CommandError::FileNotFound(_) => 2,
            CommandError::FileReadError(_) => 2,
            CommandError::FileTooLarge { .. } => 1,
            CommandError::UnsupportedImageFormat => 1,
            CommandError::ImageDecodeError(_) => 1,
            CommandError::InvalidProtocol(_) => 1,
            CommandError::EncodingError(_) => 1,
        }
    }
}
```

**Error Output:**
```rust
fn handle_error(err: CommandError) -> ! {
    eprintln!("Error: {}", err);
    std::process::exit(err.exit_code());
}
```

### CLI Argument Parsing

**Using clap with derive macros:**
```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "emterm")]
#[command(about = "eMterm - Modern terminal emulator with rich rendering")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Display Markdown file in eMterm
    Markdown {
        /// Path to Markdown file
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },

    /// Display image file in eMterm
    Image {
        /// Path to image file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Image protocol to use
        #[arg(long, default_value = "kitty", value_parser = ["kitty", "sixel"])]
        protocol: String,
    },
}
```

**Main Function:**
```rust
fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Markdown { file } => {
            execute_markdown_command(&file)
        },
        Commands::Image { file, protocol } => {
            let proto = match protocol.as_str() {
                "kitty" => ImageProtocol::Kitty,
                "sixel" => ImageProtocol::Sixel,
                _ => {
                    handle_error(CommandError::InvalidProtocol(protocol));
                }
            };
            execute_image_command(&file, proto)
        },
    };

    if let Err(err) = result {
        handle_error(err);
    }
}
```

## Test Scenarios

### Unit Tests

#### Markdown Tests
- [ ] `validate_file_path` - Valid file path returns canonicalized path
- [ ] `validate_file_path` - Non-existent file returns error
- [ ] `validate_file_path` - Directory path returns error
- [ ] `validate_file_size` - File within limit passes validation
- [ ] `validate_file_size` - File exceeding limit returns error
- [ ] `encode_and_chunk` - Content is correctly base64 encoded
- [ ] `encode_and_chunk` - Large content is split into chunks
- [ ] `generate_markdown_osc` - Correct OSC sequence format
- [ ] `generate_markdown_osc` - UUID is included in all sequences
- [ ] `generate_markdown_osc` - Sequence numbers are consecutive

#### Image Tests
- [ ] `decode_image` - PNG image decodes successfully
- [ ] `decode_image` - JPEG image decodes successfully
- [ ] `decode_image` - GIF image decodes successfully
- [ ] `decode_image` - WebP image decodes successfully
- [ ] `decode_image` - Unsupported format returns error
- [ ] `decode_image` - Corrupted image returns error
- [ ] `generate_kitty_sequence` - Correct Kitty protocol format
- [ ] `generate_kitty_sequence` - Base64 encoded PNG data
- [ ] `generate_sixel_sequence` - Valid SIXEL format

### Integration Tests

#### Markdown Command Integration
- [ ] Test 1: Small Markdown file (< 1KB) - Outputs valid OSC sequences, exit code 0
- [ ] Test 2: Medium Markdown file (100KB) - Outputs chunked sequences, exit code 0
- [ ] Test 3: GFM features (tables, task lists) - Correct encoding, exit code 0
- [ ] Test 4: File at size limit (exactly 2MB) - Processes successfully, exit code 0
- [ ] Test 5: File over size limit (2MB + 1 byte) - Error message, exit code 1
- [ ] Test 6: Non-existent file - Error message, exit code 2
- [ ] Test 7: Empty file (0 bytes) - Outputs minimal valid sequence, exit code 0
- [ ] Test 8: File without read permission - Error message, exit code 2

#### Image Command Integration
- [ ] Test 1: PNG image (Kitty protocol) - Outputs valid Kitty sequence, exit code 0
- [ ] Test 2: JPEG image (Kitty protocol) - Outputs valid Kitty sequence, exit code 0
- [ ] Test 3: GIF image (SIXEL protocol) - Outputs valid SIXEL sequence, exit code 0
- [ ] Test 4: WebP image (Kitty protocol) - Outputs valid Kitty sequence, exit code 0
- [ ] Test 5: Protocol option `--protocol=kitty` - Uses Kitty encoding
- [ ] Test 6: Protocol option `--protocol=sixel` - Uses SIXEL encoding
- [ ] Test 7: Invalid protocol option - Error message, exit code 1
- [ ] Test 8: File at size limit (exactly 10MB) - Processes successfully, exit code 0
- [ ] Test 9: File over size limit (10MB + 1 byte) - Error message, exit code 1
- [ ] Test 10: Non-image file (text file) - Error message, exit code 1
- [ ] Test 11: Corrupted image file - Error message, exit code 1
- [ ] Test 12: Tiny image (1x1 pixel) - Outputs valid sequence, exit code 0

### E2E Tests

- [ ] Scenario 1: Display README.md in eMterm - Markdown renders correctly with formatting
- [ ] Scenario 2: Display PNG screenshot in eMterm - Image appears inline at correct size
- [ ] Scenario 3: Use in shell script to display multiple files - All files display in sequence
- [ ] Scenario 4: Pipe output to another terminal emulator - Sequences are ignored (no errors)
- [ ] Scenario 5: Run on remote server via SSH - Content displays in local eMterm

### Edge Cases

- [ ] Edge case 1: Symbolic link to Markdown file - Follows link and processes target
- [ ] Edge case 2: Relative path with `..` components - Resolves correctly
- [ ] Edge case 3: Markdown with Unicode characters - Encodes correctly
- [ ] Edge case 4: Grayscale image - Converts and displays correctly
- [ ] Edge case 5: Animated GIF - Displays (animation behavior TBD by frontend)
- [ ] Edge case 6: Image with EXIF orientation - Respects orientation
- [ ] Edge case 7: Very long file path (> 260 chars on Windows) - Handles correctly

### Performance Tests

- [ ] Load test: Process 100 small files (< 10KB each) in sequence - Completes in < 5 seconds
- [ ] Stress test: Process maximum size Markdown file (2MB) - Completes in < 500ms
- [ ] Stress test: Process maximum size image (10MB) - Completes in < 1 second
- [ ] Memory test: Ensure memory usage ≤ 3x file size during processing

## Security Considerations

- **Path Traversal Prevention:** Use `Path::canonicalize()` to resolve all paths before accessing
- **Input Validation:** Validate all command-line arguments and file paths before processing
- **Size Limits:** Enforce strict size limits to prevent resource exhaustion (2MB for Markdown, 10MB for images)
- **Memory Safety:** Rust's ownership system prevents buffer overflows and use-after-free
- **No Code Execution:** Output is pure data (base64), no executable code in sequences
- **Symlink Handling:** Follow symlinks (users may legitimately use them), but canonicalization prevents escaping accessible paths

**Threat Model:**
- Malicious file paths: Mitigated by path canonicalization
- Oversized files: Mitigated by size validation
- Malformed images: Mitigated by image library's robust decoding with error handling
- Resource exhaustion: Mitigated by size limits and bounded memory usage

## Performance Optimization

### Performance Goals
- Small files (< 100KB): < 50ms end-to-end
- Medium files (100KB - 1MB): < 200ms end-to-end
- Large files (1MB - 2MB/10MB): < 500ms/1s end-to-end
- Memory overhead: ≤ 3x file size (file + decoded + encoded)

### Optimization Strategies
- **Lazy Loading:** Don't decode entire image until needed
- **Streaming Base64:** Consider streaming encoding for very large files (future optimization)
- **Efficient Chunking:** Use iterators to avoid intermediate allocations
- **Minimal Copies:** Use references and slices where possible
- **Fast Base64:** Use optimized base64 crate with SIMD support

### Caching Strategy
No caching - commands are stateless and single-use. Each invocation reads fresh from filesystem.

## Success Criteria

- [ ] All functional requirements (FR1-FR7) are implemented
- [ ] All non-functional requirements (NFR1-NFR7) are met
- [ ] Unit test coverage ≥ 80%
- [ ] All integration tests pass
- [ ] E2E tests verify actual rendering in eMterm
- [ ] Performance benchmarks meet specified goals
- [ ] Documentation (rustdoc) is complete for all public APIs
- [ ] Code review completed and approved
- [ ] Works on Linux, macOS, and Windows

## Open Questions

- [ ] Should we add a `--verbose` flag for debugging output?
- [ ] Do we need progress indication for large files?
- [ ] Should image command support automatic resizing to terminal width?
- [ ] Should we add a `--dry-run` option to validate without outputting?
- [ ] Do we need configuration file support for defaults?

## Terminal Compatibility

### OSC 777 Protocol (eMterm Extension)

The Markdown display feature uses **OSC 777**, which is an **eMterm-specific extension**. This is not a standardized protocol.

**Behavior in Non-eMterm Terminals:**

| Terminal | Behavior | User Experience |
|----------|----------|-----------------|
| eMterm | Renders Markdown as HTML | ✅ Full support |
| Other terminals | OSC 777 sequences are ignored | ⚠️ Raw base64 may appear briefly |
| Terminal multiplexers (tmux, screen) | Passes through to attached terminal | Depends on attached terminal |

**Important Notes:**
- OSC 777 with `emterm` namespace is designed to be safely ignored by non-supporting terminals
- Non-eMterm terminals will typically discard unrecognized OSC sequences
- In worst case, brief garbage characters may appear but no harm is done
- The `ST` (String Terminator) ensures clean sequence boundaries

### Kitty Graphics Protocol

The image display feature uses **Kitty Graphics Protocol** by default, which is supported by:
- Kitty terminal (native)
- eMterm (implemented)
- Some other modern terminals (varies)

**Behavior in Non-Kitty Terminals:**
- Sequences are ignored or display as text
- Using `--protocol=sixel` may provide wider compatibility

### SIXEL Protocol

SIXEL is an older protocol with broader support:
- Native DEC VT terminals
- xterm (with `-ti 340` option)
- mlterm, mintty, and others
- eMterm (implemented)

**Recommendation:** Use SIXEL for maximum compatibility across terminals.

### Design Rationale

This CLI tool intentionally outputs control sequences to stdout without checking the terminal type because:
1. **Stateless Design:** Works over SSH and through pipes
2. **User Control:** Users know their terminal environment
3. **No Side Effects:** Non-supporting terminals safely ignore sequences
4. **Composability:** Can be combined with other tools (e.g., `tmux`, terminal recorders)

## Implementation Phases

### Phase 1: Core Markdown Command
**Goals:** Implement basic Markdown command with file reading and OSC generation
**Deliverables:**
- Markdown command CLI parsing
- File validation and reading
- Base64 encoding and chunking
- OSC sequence generation
- Unit tests for core functionality

### Phase 2: Core Image Command (Kitty)
**Goals:** Implement image command with Kitty Graphics Protocol support
**Deliverables:**
- Image command CLI parsing
- Image decoding (PNG, JPEG, GIF, WebP)
- Kitty protocol sequence generation
- Unit tests for image processing

### Phase 3: SIXEL Support
**Goals:** Add SIXEL protocol support for image command
**Deliverables:**
- SIXEL encoding implementation
- Protocol selection via `--protocol` option
- Tests for SIXEL output

### Phase 4: Integration & Testing
**Goals:** Comprehensive testing and integration with eMterm
**Deliverables:**
- Integration tests for both commands
- E2E tests with eMterm terminal
- Performance benchmarks
- Documentation

### Phase 5: Polish & Release
**Goals:** Final refinements and release preparation
**Deliverables:**
- Error message improvements
- Help text refinement
- Cross-platform testing
- Release documentation

## References

- eMterm CLAUDE.md: `/home/sakura/cache/worktrees/emterm/feature-cli-display-commands/CLAUDE.md`
- Kitty Graphics Protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- SIXEL Specification: https://vt100.net/docs/vt3xx-gp/chapter14.html
- GitHub Flavored Markdown: https://github.github.com/gfm/
- Rust clap documentation: https://docs.rs/clap/
- Rust image crate: https://docs.rs/image/
