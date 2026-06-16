# Feature: native-poc CLI Subcommand Port (Phase A + B)

## Overview

Port the `markdown` / `json` / `yaml` / `image` CLI subcommands from
the WebView build (`src-tauri`) into the native-poc binary
(`emterm-native-poc`). After this work, the native-poc binary can emit
OSC 777 emterm sequences (markdown / json / yaml / image) on its own —
no system-installed WebView build needed for OSC verification or for
end-user `emterm markdown foo.md`-style invocations.

This SPEC covers **Phase A** (markdown / json / yaml) and
**Phase B** (image, Kitty + SIXEL encoders). `markdown --interactive`
(stdin loop) and `download` are deferred to separate SDD tasks.

## Objectives

- Provide `markdown`, `json`, `yaml`, and `image` subcommands on
  `emterm-native-poc` with output byte-for-byte compatible with the
  WebView build.
- Add a new `cli` module tree under `native-poc/src/cli/` that mirrors
  the WebView build's `src-tauri/src/{commands,encoding,protocols,validation}/`
  shape, so a future `crates/` extraction is cheap.
- Keep new runtime dependencies to **`clap`** (parser) and **`uuid`**
  (session_id). `rust-i18n` is deliberately not introduced.
- Preserve security guards from the source: file open through TOCTOU-safe
  helpers, image dimension cap (8192×8192), tmux DCS passthrough.

## User Stories

### US1: Run `markdown` standalone
As a native-poc developer, I want to run
`./emterm-native-poc markdown foo.md` and have the viewer pop up,
so that I can verify the OSC pipeline without launching the WebView build.

**Acceptance Criteria:**
- [ ] Stdout receives `\x1b]777;emterm;markdown;begin;...\x1b\\` followed
      by one or more `;chunk;...` frames and an `;end;...` frame.
- [ ] When `$TMUX` is set, each frame is wrapped in
      `\x1bPtmux;...\x1b\\` (every internal `\x1b` doubled per tmux rules).
- [ ] Exit code is `0` on success, non-zero with a localized stderr
      message on failure (matching `CommandError::exit_code()`).

### US2: Run `json` / `yaml` standalone
As a developer, I want `./emterm-native-poc json a.json` to emit
OSC 777 emterm;json sequences, and likewise for `yaml`.

**Acceptance Criteria:**
- [ ] Stdout receives a `begin` → `chunk` × N → `end` frame sequence
      with verb `json` (or `yaml`).
- [ ] No file size limit is enforced (matches `src-tauri` behavior;
      directories and unreadable paths still error out).

### US3: Run `image` standalone with Kitty or SIXEL
As a developer, I want `./emterm-native-poc image foo.png` to emit a
Kitty APC sequence and `--protocol sixel` to emit a SIXEL DCS sequence.

**Acceptance Criteria:**
- [ ] Default protocol is `kitty`.
- [ ] `--protocol sixel` selects the SIXEL encoder.
- [ ] Invalid protocol names fail with `CommandError::InvalidProtocol`
      and exit code 1.
- [ ] Images exceeding 10 MB or 8192×8192 pixels are rejected before
      decode.
- [ ] On Unix, stdin is drained for ~2 s after Kitty output to absorb
      `Gi=…OK` responses (cfg-gated to `unix`).

### US4: i18n without `rust-i18n`
As a maintainer, I do not want `rust-i18n` introduced into the
native-poc binary.

**Acceptance Criteria:**
- [ ] `cargo tree -p emterm-native-poc` does not show `rust-i18n`.
- [ ] All localized strings (clap help text, error messages) live
      either in a new `cli/messages.rs` module or inline at use sites,
      using `match locale` on `crate::i18n::Locale`.

## Technical Requirements

### Functional Requirements

- **FR1 — markdown subcommand**: Implement
  `cli::markdown::run(args: &[String]) -> i32` that accepts a single
  positional file path, validates it (≤ 10 MB,
  `MAX_MARKDOWN_SIZE = 10 * 1024 * 1024`), encodes content base64,
  chunks the encoded payload by `MARKDOWN_CHUNK_SIZE = 128 KB`,
  emits OSC 777 emterm;markdown frames (`begin` / `chunk` × N / `end`)
  to stdout, and wraps frames in tmux DCS passthrough when `$TMUX`
  is set.
- **FR2 — json subcommand**: Implement `cli::json::run` with no file
  size limit, 128 KB chunks (`JSON_CHUNK_SIZE`), verb `json`.
- **FR3 — yaml subcommand**: Implement `cli::yaml::run` with the same
  shape as `json`, verb `yaml`, constant `YAML_CHUNK_SIZE = 128 KB`.
- **FR4 — image subcommand**: Implement `cli::image::run` with
  positional file argument and optional `--protocol kitty|sixel`
  (default `kitty`). Caps: 10 MB file
  (`MAX_IMAGE_SIZE = 10 * 1024 * 1024`), 8192×8192 pixels
  (`MAX_IMAGE_DIMENSION`). On Unix, drain stdin for ~2 s after output
  (Kitty only).
- **FR5 — `cli` module dispatcher**: Add `cli::run(args: &[String]) ->
  i32` selecting subcommand by `args[0]`. The dispatcher uses clap
  internally so help / `--help` / unknown-subcommand messages are
  consistent.
- **FR6 — main.rs integration**: In `native-poc/src/main.rs`, before
  the existing `--viewer` / `--settings` / `--image-viewer` /
  `--data-viewer` branch, inspect `args.get(1)`. If it equals
  `markdown` | `json` | `yaml` | `image`, dispatch into `cli::run`
  and `std::process::exit(code)`. Internal child-process flags stay
  on the existing hand-rolled path.
- **FR7 — tmux passthrough**: Port `src-tauri/src/commands/tmux.rs`
  verbatim to `native-poc/src/cli/tmux.rs`. Detection: `std::env::var("TMUX").is_ok()`.
- **FR8 — Localized error messages**: All `CommandError` variants from
  `src-tauri/src/error.rs` are ported. The `Display` impl is rewritten
  to take a `Locale` through a helper function rather than a global
  `t!` macro lookup.
- **FR9 — session_id**: Generated via `uuid::Uuid::new_v4()` for every
  command invocation, matching the WebView build exactly.
- **FR10 — Unit test parity**: Existing unit tests under
  `src-tauri/src/{commands,encoding,protocols,validation,error}.rs`
  are ported to native-poc with `t!`/`rust_i18n::set_locale` calls
  rewritten to call the new locale-aware helper functions.

### Non-Functional Requirements

- **NFR1 — Performance**: For a 100 KB markdown file, the CLI must
  complete (write the full OSC frame stream to a pipe) in < 200 ms on
  the developer's reference machine; for a 1 MB PNG with Kitty
  encoding, < 500 ms. Targets mirror the WebView build's de facto
  behavior.
- **NFR2 — Security**:
  - `image` subcommand: `validation::file::open_and_validate_file`
    returns a size-validated `File` handle, and the entire file content
    is read into an in-memory buffer from that fd before any decoder
    touches the path. Dimension probe (`ImageReader::with_guessed_format`
    + `into_dimensions`) and full decode (`image::load_from_memory`)
    both operate on that same buffer, so there is no TOCTOU window
    between size-check and decode. Dimension check happens *before*
    full decode to defend against decompression bombs.
  - `markdown` subcommand: matches src-tauri verbatim — `canonicalize`
    followed by `File::open(&canonical)` followed by `metadata` on the
    open fd. There is a narrow TOCTOU window between `canonicalize` and
    `File::open` (path swap during the gap), but the byte content
    actually read is the file the fd resolved to, and the size cap is
    enforced on the fd metadata. The canonical path is required for the
    OSC `basedir` parameter, which is why this path differs from
    `image`. This is an accepted, src-tauri-compatible trade-off for a
    local CLI.
  - `json` / `yaml` subcommands: `File::open` + `metadata().is_file()`
    check (no size cap by design, per FR2 / FR3).
  - All user-influenced strings interpolated into error messages
    (`--protocol` value, file paths, format names, etc.) are run
    through `cli::messages::escape_control_chars` before being written
    to stderr, so an attacker-controlled value cannot inject OSC / APC
    / CSI sequences into the user's terminal.
  - Output is never interpolated with raw user content — base64 or DCS
    escaping is mandatory in every frame.
- **NFR3 — Dependency minimalism**: Net new dependencies are exactly:
  - `clap = "4"` (runtime, with `derive` feature)
  - `uuid = { version = "1", features = ["v4"] }` (runtime)
  - `tempfile = "3"` (dev-dependency, for ported tests)
  - No `rust-i18n`, no `term_images` encoder addition, no new crates
    under `crates/`.
- **NFR4 — Cross-platform**: Build and tests must pass on Linux and
  Windows. Unix-only code (`drain_stdin_responses`, libc-based termios
  manipulation) is gated by `#[cfg(unix)]`; Windows uses a no-op stub.
- **NFR5 — Repo policy**: Per `project_native_poc_branch_policy`, no
  files under `src/`, `src-tauri/`, `wasm/`, `crates/` are modified.
  Changes are confined to `native-poc/`.

## Implementation Approach

### Architecture

**Module layout (target):**

```
native-poc/src/
├── main.rs                  # add 1 dispatch arm (Section "main.rs integration")
├── cli/
│   ├── mod.rs               # `pub fn run(args: &[String]) -> i32` + clap App
│   ├── messages.rs          # locale-aware string helpers (cli.* + error.*)
│   ├── error.rs             # ported CommandError (no rust_i18n)
│   ├── tmux.rs              # ported tmux passthrough
│   ├── encoding/
│   │   ├── mod.rs
│   │   ├── base64.rs        # ported encode_base64 + chunk_data
│   │   └── osc.rs           # ported markdown/json/yaml/image OSC builders
│   ├── validation/
│   │   ├── mod.rs
│   │   ├── file.rs          # open_and_validate_file + size check
│   │   └── image.rs         # magic-byte image format validator
│   ├── protocols/
│   │   ├── mod.rs
│   │   ├── kitty.rs         # generate_kitty_sequence (APC builder)
│   │   └── sixel.rs         # generate_sixel_sequence (DCS builder)
│   ├── markdown.rs          # execute_markdown_command (no interactive loop)
│   ├── json.rs              # execute_json_command
│   ├── yaml.rs              # execute_yaml_command
│   └── image.rs             # execute_image_command + ImageProtocol enum
```

`cli/messages.rs` exists because once `t!` is gone the alternative is
either inline `match locale` per error site (verbose, error-prone) or
one centralized table. The latter is cleaner and easier to keep parity
with `src-tauri/locales/{en,ja}.json`.

### Locale routing

```rust
// native-poc/src/cli/messages.rs
use crate::i18n::Locale;

pub fn err_file_not_found(loc: Locale, path: &std::path::Path) -> String {
    match loc {
        Locale::En => format!("File not found: {}", path.display()),
        Locale::Ja => format!("ファイルが見つかりません: {}", path.display()),
    }
}
pub fn err_file_too_large(loc: Locale, size: u64, max_size: u64) -> String {
    match loc {
        Locale::En => format!(
            "File size ({} bytes) exceeds {} bytes limit",
            size, max_size
        ),
        Locale::Ja => format!(
            "ファイルサイズ ({}バイト) が{}バイトの制限を超えています",
            size, max_size
        ),
    }
}
// ... (one fn per error variant + clap about/help strings)
```

`cli::error::CommandError`'s `Display` impl resolves the locale via
`crate::i18n::resolve(load_language())` (where `load_language` either
reads settings.json or falls back to `Language::Auto`) and dispatches:

```rust
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = crate::cli::active_locale(); // pub fn in cli::mod
        let s = match self {
            CommandError::FileNotFound(p) => messages::err_file_not_found(loc, p),
            CommandError::FileTooLarge { size, max_size } =>
                messages::err_file_too_large(loc, *size, *max_size),
            // ... 10 variants total
        };
        f.write_str(&s)
    }
}
```

`active_locale()` reads settings once per CLI invocation and caches in
a `OnceLock<Locale>` so each error format does not re-read the file.

### main.rs integration

Insert *before* the existing flag-style branches:

```rust
fn main() {
    init_logging();

    let args: Vec<String> = std::env::args().collect();

    // CLI subcommand dispatch (Phase A + B)
    if let Some(sub) = args.get(1).map(|s| s.as_str()) {
        if matches!(sub, "markdown" | "json" | "yaml" | "image") {
            let code = emterm_native_poc::cli::run(&args[1..]);
            std::process::exit(code);
        }
    }

    // Existing branches (unchanged):
    // --viewer / --settings / --image-viewer / --data-viewer / mux / ...
    // ... terminal startup ...
}
```

The CLI subcommands and child-process flags never collide because the
former are bare words (`markdown`) and the latter are `--`-prefixed.

### Data Flow

```
user shell
   │ exec ./emterm-native-poc markdown foo.md
   ▼
main.rs (args[1] == "markdown")
   │
   ▼
cli::run(&args[1..])
   │ clap parse, dispatch by subcommand
   ▼
cli::markdown::execute_markdown_command(path)
   │ canonicalize(path)               (basedir source; src-tauri-equivalent)
   │ File::open(&canonical) + metadata.len() ≤ MAX_MARKDOWN_SIZE
   │ read content
   │ (NFR2: TOCTOU-safe for image, src-tauri-equivalent for markdown — see NFR2)
   │ uuid v4 session_id
   │ encoding::base64::encode + chunk_data
   │ encoding::osc::build_markdown_frames
   ▼
cli::tmux::passthrough_if_needed (wraps if $TMUX set)
   │
   ▼
stdout (line-buffered, flushed)
   │
   ▼
parent PTY = native-poc ANSI parser
   │ recognize OSC 777 emterm;markdown
   ▼
viewer/markdown.rs (already implemented)
   │
   ▼
viewer child process renders Markdown
```

### CLI Subcommand Grammar

```
emterm-native-poc markdown <FILE>
emterm-native-poc json <FILE>
emterm-native-poc yaml <FILE>
emterm-native-poc image <FILE> [--protocol kitty|sixel]
```

clap definition (using `clap = { version = "4", features = ["derive"] }`):

```rust
#[derive(clap::Parser)]
#[command(name = "emterm-native-poc", about_l10n = ...)]
struct Cli {
    #[command(subcommand)]
    sub: SubCmd,
}

#[derive(clap::Subcommand)]
enum SubCmd {
    Markdown { file: PathBuf },
    Json { file: PathBuf },
    Yaml { file: PathBuf },
    Image {
        file: PathBuf,
        #[arg(long, default_value = "kitty")]
        protocol: String,
    },
}
```

Clap's static help text strings come from `cli::messages` — the `about`
fields are filled in at runtime using `Command::about(...)` rather than
the macro-time string literal, because the message depends on locale.

### OSC Frame Format

The exact byte format is inherited verbatim from
`src-tauri/src/encoding/osc.rs` and `src-tauri/src/protocols/{kitty,sixel}.rs`
and MUST stay byte-identical. The format strings below are the
authoritative shape — the SPEC follows the implementation, not the
other way around.

For `markdown`:

```
\x1b]777;emterm;markdown;begin;id={uuid};format=gfm;render=fullscreen;version=1.0[;basedir={sanitized_dir}]\x1b\\
\x1b]777;emterm;markdown;chunk;id={uuid};seq=0;data={base64}\x1b\\
\x1b]777;emterm;markdown;chunk;id={uuid};seq=1;data={base64}\x1b\\
...
\x1b]777;emterm;markdown;end;id={uuid}[;interactive=1]\x1b\\
```

Notes:
- `;basedir=...` is appended to the `begin` frame only when the caller
  resolved a parent directory (markdown subcommand emits it; other paths
  may omit). The value is run through `sanitize_osc_value` to strip
  semicolons and control characters.
- The `end` frame appends `;interactive=1` **only** when the
  interactive loop is engaged. Non-interactive emissions carry no
  `interactive` parameter at all — receivers MUST treat absence as
  `false`.
- There is intentionally NO `total={N}` parameter on the end frame.
  Frame-count validation is the receiver's job via the `seq=…` indices.

For `json` / `yaml`, the verb segment is `emterm;json;…` /
`emterm;yaml;…`. The begin frame is `;version=1.0` with no
`format=`/`render=`/`basedir=`:

```
\x1b]777;emterm;json;begin;id={uuid};version=1.0\x1b\\
\x1b]777;emterm;yaml;begin;id={uuid};version=1.0\x1b\\
```

For `image` (Kitty APC, PNG streaming with `q=2` quiet mode):

```
\x1b_Gi={id},f=100,q=2,a=T,m=1;{base64 chunk 1}\x1b\\
\x1b_Gi={id},q=2,m=1;{base64 chunk 2}\x1b\\
...
\x1b_Gi={id},q=2,m=0;{base64 final}\x1b\\
```

- `f=100` = PNG (dimensions are embedded in the PNG header, so the
  legacy `s=`/`v=` raw-RGBA params are deliberately absent).
- `q=2` suppresses the terminal's `OK` ACKs (errors still come back).
- `a=T` = transmit + display.
- `m=1` on every non-final chunk, `m=0` on the last.

For `image` (SIXEL DCS):

```
\x1bPq{sixel data}\x1b\\
```

When `$TMUX` is set, every `\x1b` inside the frame is doubled and the
outer wrapper `\x1bPtmux;<inner>\x1b\\` is added, matching the WebView
build's `commands/tmux.rs` exactly.

### Dependencies

**Internal Dependencies:**
- `native-poc/src/i18n.rs` (`Locale` enum, `resolve` function) — used by
  `cli::active_locale`.
- `native-poc/src/settings.rs` (`Language` enum, `load_language`) — used
  to pick the locale at CLI dispatch time.
- `native-poc/src/viewer/markdown.rs` / `viewer/data.rs` — receiver
  side, already implemented. No changes.

**External Dependencies (added):**
- `clap = { version = "4", features = ["derive"] }` (runtime)
- `uuid = { version = "1", features = ["v4"] }` (runtime)
- `tempfile = "3"` (dev-dependency, ported tests)

**External Dependencies (existing, reused):**
- `base64 = "0.22"` (encoding)
- `image = "0.25"` (image decode)
- `log = "0.4"` (diagnostics)
- `libc` (Unix termios for stdin drain)

### File Structure

See *Module layout* above. New / modified files:

```
native-poc/
├── Cargo.toml                       # MODIFIED: add clap, uuid, tempfile(dev), [lib] target
├── src/
│   ├── main.rs                      # MODIFIED: add CLI dispatch arm
│   ├── lib.rs                       # NEW (Phase 4): library target hosting `pub mod cli;`
│   └── cli/                         # NEW: entire subtree
│       ├── mod.rs
│       ├── messages.rs
│       ├── error.rs
│       ├── tmux.rs
│       ├── encoding/
│       │   ├── mod.rs
│       │   ├── base64.rs
│       │   └── osc.rs
│       ├── validation/
│       │   ├── mod.rs
│       │   ├── file.rs
│       │   └── image.rs
│       ├── protocols/
│       │   ├── mod.rs
│       │   ├── kitty.rs
│       │   └── sixel.rs
│       ├── markdown.rs
│       ├── json.rs
│       ├── yaml.rs
│       └── image.rs
```

`src-tauri/`, `src/`, `wasm/`, `crates/` are **untouched**.

## Test Scenarios

### Unit Tests

Ported verbatim from `src-tauri` (with `rust_i18n::set_locale` replaced
by setting the cached `active_locale` via a test-only helper):

- [ ] `cli::error` — 10+ tests for `CommandError::exit_code()` and
      `Display` in both locales (`test_error_display_messages_localized`).
- [ ] `cli::encoding::base64` — `test_encode_base64`, `test_chunk_data`,
      empty input, off-by-one boundaries.
- [ ] `cli::encoding::osc` — frame format tests (begin / chunk / end
      verb correctness, parameter ordering, ESC sequences present).
- [ ] `cli::markdown` — ~50 tests including: tiny file, empty file, near
      `MAX_MARKDOWN_SIZE`, just over the limit, UTF-8 / non-UTF-8 bytes,
      embedded image references.
- [ ] `cli::json` — 3 tests (small, large, malformed-but-readable).
- [ ] `cli::yaml` — 3 tests (same shape as json).
- [ ] `cli::image` — 8 tests including: protocol parsing, valid PNG with
      Kitty, valid PNG with SIXEL, missing file, oversize file, dimension
      check, max-dimension constant.
- [ ] `cli::tmux` — 9 tests covering ESC doubling and wrapper format.
- [ ] `cli::validation::file` — `open_and_validate_file` happy path,
      not-a-file (directory), TOCTOU.
- [ ] `cli::validation::image` — magic byte recognition for PNG/JPEG/
      GIF/WebP, unsupported format rejection.

### Integration Tests

- [ ] `native-poc/tests/cli_subcommands.rs` (new): spawn
      `emterm-native-poc markdown <fixture>.md`, capture stdout,
      assert OSC framing and that `begin`/`end` IDs match.
- [ ] Similar tests for `json`, `yaml`, `image --protocol kitty`,
      `image --protocol sixel`.
- [ ] `TMUX=1` env var path: assert outer DCS wrapper present.

### E2E Tests

**Existing E2E tests**: `e2e-tests/specs/*.e2e.js` (Tauri / WebView build
only). The Phase A+B native-poc CLI is not currently exercised by the
project's WebDriver E2E suite — that suite targets the WebView app,
not native-poc.

- [ ] Existing E2E tests pass without regression (this work touches
      only `native-poc/`, so no impact expected, but verify).
- [ ] **New manual verification**: launch `target-host/release/emterm-native-poc`,
      then from a shell inside that terminal run
      `./target-host/release/emterm-native-poc markdown README.md` — viewer
      window should pop up rendering the README. Repeat for json / yaml
      / image with Kitty and SIXEL.

### Edge Cases

- [ ] Empty file → produces a single `begin` + `end` pair, no `chunk`
      frames.
- [ ] File at exactly `MAX_MARKDOWN_SIZE` (10 MB) → succeeds.
- [ ] File at `MAX_MARKDOWN_SIZE + 1` → `FileTooLarge`.
- [ ] Image with dimensions `8192 × 8192` → succeeds.
- [ ] Image with dimensions `8193 × 8192` → `EncodingError("…exceeds maximum…")`.
- [ ] `--protocol ascii` → `InvalidProtocol`, exit code 1.
- [ ] Missing file path → `FileNotFound`, exit code 2.
- [ ] Path is a directory → `NotAFile`, exit code 2.
- [ ] `$TMUX` is set → every frame is wrapped, internal ESC doubled.
- [ ] `$TMUX` is unset → frames emitted as-is.
- [ ] System locale is `ja_JP.UTF-8` → error messages are in Japanese.
- [ ] System locale is `en_US.UTF-8` → error messages are in English.

### Performance Tests

- [ ] 100 KB markdown file → CLI exit within 200 ms (wall clock).
- [ ] 1 MB PNG → Kitty encoding completes within 500 ms.
- [ ] 5 MB PNG → SIXEL encoding completes within reasonable time
      (no hard target; just no regressions vs WebView build).

## Security Considerations

- **Authentication**: N/A (CLI invoked by the local user).
- **Authorization**: Inherits OS file permissions; `PermissionDenied`
  is surfaced via `CommandError`.
- **Input Validation**:
  - File path validation via `validation::file::open_and_validate_file`
    (TOCTOU-safe).
  - File size limits enforced before reading content.
  - Image magic bytes validated before invoking `image::open`.
  - Image dimensions validated before full decode (decompression bomb
    defense).
- **Data Protection**: All file content goes out via stdout — same
  trust boundary as the user's existing shell session.
- **XSS / Injection**: N/A for CLI surface. OSC payloads are base64
  encoded so embedded `\x1b` cannot break out of the frame.
- **SQL Injection**: N/A.
- **CSRF**: N/A.

## Error Handling

### Error Variants (ported verbatim from `src-tauri/src/error.rs`)

| Variant | Exit Code | When |
|---------|-----------|------|
| `FileNotFound(PathBuf)` | 2 | Path does not exist |
| `NotAFile(PathBuf)` | 2 | Path is a directory or special file |
| `FileReadError(io::Error)` | 2 | OS-level read failure |
| `FileTooLarge { size, max_size }` | 1 | Above `MAX_MARKDOWN_SIZE` / `MAX_IMAGE_SIZE` |
| `UnsupportedImageFormat(image::ImageFormat)` | 1 | Magic bytes match a format `image` cannot decode |
| `ImageDecodeError(image::ImageError)` | 1 | `image::open` failure |
| `InvalidProtocol(String)` | 1 | `--protocol` value not in `{kitty, sixel}` |
| `EncodingError(String)` | 1 | Dimension cap exceeded, encoder internal error |
| `NameRequired` | 2 | Reserved for Phase D (download); not used in Phase A/B |
| `PermissionDenied(PathBuf)` | 2 | Path readable bit not set for the user |

### Error Flow

```
Error Occurs (in cli/{markdown,json,yaml,image}.rs)
   │
   ▼
Result<(), CommandError> bubbles up to cli::run
   │
   ▼
cli::run formats with localized Display impl
   │
   ▼
eprintln!("Error: {}", err)
   │
   ▼
std::process::exit(err.exit_code())
```

`stderr` formatting matches the WebView build: `Error: <localized message>`.

## Performance Optimization

### Performance Goals

- markdown 100 KB → < 200 ms exit
- json/yaml 100 KB → < 200 ms exit
- image 1 MB PNG (Kitty) → < 500 ms exit
- No new regressions vs WebView build for any sub-command.

### Optimization Strategies

- **Pre-allocated Vec buffers**: `Vec::with_capacity(metadata.len())`
  before `read_to_end` (pattern from `commands/json.rs:29`).
- **Early drop**: drop `content: Vec<u8>` immediately after base64
  encoding to free memory before chunk emission.
- **Locked stdout**: `io::stdout().lock()` once per invocation, single
  `write_all` per frame, single `flush` at the end (already in
  `commands/image.rs`).
- **`OnceLock<Locale>`**: read settings.json once per process invocation
  to resolve the active locale; no repeated I/O.

### Caching Strategy

- Active locale cached in `cli::active_locale()` via `OnceLock<Locale>`
  for the lifetime of one CLI invocation.

## Success Criteria

- [ ] FR1–FR10 fully implemented in `native-poc/src/cli/` with no
      modifications to `src-tauri/`, `src/`, `wasm/`, or `crates/`.
- [ ] All ported unit tests pass under
      `CARGO_TARGET_DIR=native-poc/target cargo test
      --manifest-path native-poc/Cargo.toml`.
- [ ] Integration tests covering happy paths and tmux passthrough pass.
- [ ] Release binary at `native-poc/target-host/release/emterm-native-poc`
      built via the path documented in `.claude/rules/native-poc-build-location.md`.
- [ ] Manual verification: launch native-poc, run each subcommand,
      confirm viewer / data window / inline image rendering.
- [ ] `cargo tree -p emterm-native-poc` shows `clap` + `uuid` added
      and **no `rust-i18n`**.
- [ ] No regressions in existing native-poc behavior (terminal startup,
      `--viewer` child, mux, settings, image viewer, data viewer).

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

(None — all judgment-call questions in the source spec memo were
resolved in Phase 2 of `sdd.1-create-spec`. The deferred Phase C / D
items are tracked as separate SDD tasks, not as `tbd` on this one.)

## Implementation Phases

### Phase A: markdown / json / yaml

**Goals:** Get the three OSC-only subcommands to byte-parity with the
WebView build.

**Deliverables:**
- `native-poc/src/cli/mod.rs` + clap dispatcher.
- `cli::error`, `cli::messages`, `cli::tmux`, `cli::encoding/{base64,osc}`,
  `cli::validation/file`.
- `cli::markdown` (without interactive loop), `cli::json`, `cli::yaml`.
- Ported unit tests for the above.
- `Cargo.toml` updated with `clap`, `uuid`, `tempfile (dev)`.
- main.rs dispatch arm.

### Phase 4 add-ons (covered in the implementation plan)

- `native-poc/src/lib.rs` is created to host the `cli` module so the
  integration tests can call `cli::run` in-process (native-poc is
  currently a bin-only crate). A `[lib]` target is added to
  `Cargo.toml` alongside the existing `[[bin]]`.

### Phase B: image (Kitty + SIXEL)

**Goals:** Bring the image subcommand to full parity with the WebView
build, including both encoders.

**Deliverables:**
- `cli::validation::image` (magic byte validator).
- `cli::protocols/{kitty,sixel}` (encoder modules ported verbatim from
  `src-tauri/src/protocols/`).
- `cli::image` (`ImageProtocol` enum, `execute_image_command`).
- Unix-only `drain_stdin_responses` (cfg-gated).
- Ported image unit tests.

## References

- `tmp/native-poc-cli-subcommand-port-2026-06-16.md` — initial scoping memo.
- `src-tauri/src/main.rs:31-220` — original CLI dispatch.
- `src-tauri/src/error.rs` — `CommandError` definition (188 lines).
- `src-tauri/src/encoding/base64.rs` — `encode_base64`, `chunk_data`.
- `src-tauri/src/encoding/osc.rs` — OSC frame builders.
- `src-tauri/src/commands/markdown.rs` — markdown command (622 lines,
  interactive loop excluded from Phase A).
- `src-tauri/src/commands/json.rs` / `yaml.rs` — data viewer CLI (84 lines each).
- `src-tauri/src/commands/image.rs` — image command (229 lines).
- `src-tauri/src/commands/tmux.rs` — DCS passthrough (232 lines).
- `src-tauri/src/protocols/{kitty,sixel}.rs` — image encoders (173 + 378 lines).
- `src-tauri/src/validation/{file,image}.rs` — input validators.
- `src-tauri/locales/{en,ja}.json` — original `cli.*` / `error.*` strings.
- `.claude/rules/native-poc-build-location.md` — build paths
  (`native-poc/target` vs `native-poc/target-host`).
- Memory: `project_native_poc_branch_policy`,
  `project_native_poc_markdown_viewer_port`,
  `project_native_webview_host_shared`.
