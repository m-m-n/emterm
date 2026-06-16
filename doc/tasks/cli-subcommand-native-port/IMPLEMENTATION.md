# Implementation Plan: native-poc CLI Subcommand Port (Phase A + B)

## Overview

Port the `markdown` / `json` / `yaml` / `image` CLI subcommands from the
WebView build (`src-tauri`) into the native-poc binary, organized as a
new `cli` module tree under `native-poc/src/cli/`. Two implementation
phases — text subcommands (markdown/json/yaml) and image subcommand
(Kitty + SIXEL).

## Objectives

- Provide working `markdown` / `json` / `yaml` / `image` subcommands on
  `emterm-native-poc` with byte-for-byte OSC compatibility with
  `src-tauri`.
- Keep new runtime dependencies to `clap` and `uuid` only (`tempfile`
  for tests); no `rust-i18n`.
- Preserve every security guard, validation step, and tmux passthrough
  behavior of the source.
- Land in a structure that mirrors `src-tauri/src/{commands,encoding,protocols,validation}/`
  to keep a future `crates/` extraction cheap.

## Prerequisites

### Development Environment

- Rust toolchain matching `native-poc/Cargo.toml` edition 2021
- `cargo` ≥ 1.75 (already required by the workspace)
- Standard build deps for native-poc on Linux + Windows
  (GTK+WebKitGTK on Linux, WebView2 on Windows — already in place for
  the viewer subsystem; no extension for this work)

### Dependencies

- Crate additions (will be applied in Phase 1):
  - `clap` 4.x with `derive` (runtime)
  - `uuid` 1.x with `v4` (runtime)
  - `tempfile` 3.x (dev-dependencies)
- Internal modules already present and reused:
  - `crate::i18n` (`Locale`, `resolve`)
  - `crate::settings` (`Language`, settings loader)
  - `crate::viewer::markdown` / `crate::viewer::data` (receivers,
    untouched)

## Architecture Overview

### Technology Stack

- **Language**: Rust 2021 (native-poc)
- **Framework**: standalone binary; clap-based subcommand dispatch
- **Key Libraries**:
  - `clap` — argument parsing for `markdown` / `json` / `yaml` / `image`
  - `uuid` — `Uuid` v4 for OSC session_id
  - `base64` (existing) — payload encoding
  - `image` (existing) — raster decoding
  - `libc` (existing, Unix only) — termios for stdin drain
  - `log` (existing) — diagnostics

### Design Approach

1. **Mirror src-tauri module shape**. A new `native-poc/src/cli/`
   subtree mirrors `src-tauri/src/{commands,encoding,protocols,validation}/`
   one-for-one. Future `crates/` extraction can merge both sides without
   significant divergence.
2. **Locale-aware messages via a small message helper module**, not
   `rust-i18n`. The active locale is resolved once per CLI invocation
   and cached. The legacy `CommandError::Display` impl is rewritten to
   look up locale + format via the helper module.
3. **CLI dispatch precedes flag dispatch in main.rs**. Bare-word
   subcommands (`markdown` / `json` / `yaml` / `image`) are recognized
   first; `--`-prefixed child-process flags retain the existing
   hand-rolled path.
4. **Encoder-by-copy**, not by `term_images` crate extension. The
   Kitty / SIXEL encoder modules under `src-tauri/src/protocols/` are
   copied to `native-poc/src/cli/protocols/` verbatim. The decoder
   path in `term_images` is untouched.
5. **Branch policy compliance** (memory: `project_native_poc_branch_policy`).
   No files outside `native-poc/` are modified.

### Component Interaction

Single binary, single process per CLI invocation:

1. User executes `emterm-native-poc <subcommand> ...` in a terminal.
2. `main.rs` inspects `args[1]`; if it matches a known subcommand, it
   delegates to `cli::run` and exits with the returned code.
3. `cli::run` parses arguments via clap, picks the locale, and
   dispatches to the per-subcommand handler.
4. The handler validates the input, builds OSC/APC/DCS frames, applies
   tmux passthrough when `$TMUX` is set, and writes to stdout.
5. The receiver (the parent PTY = the user's terminal, which may itself
   be native-poc) consumes the frames through its existing OSC dispatch.
   No new receiver-side code is needed (already in place from prior
   work; see memory `project_native_poc_markdown_viewer_port`).

## Implementation Phases

### Phase 1: CLI foundation (deps + shared modules)

**Goal**: Establish the `cli` module tree with shared infrastructure —
dependencies, locale helper, error type, encoding utilities, tmux
passthrough — so subsequent phases only need to add handler logic.

**Files to Create**:
- `native-poc/src/cli/mod.rs` — `cli::run` entry point, `active_locale`
  helper (caches resolved `Locale`), clap-based dispatcher dispatching
  to subcommand modules
- `native-poc/src/cli/messages.rs` — locale-aware string helpers for
  every `cli.*` and `error.*` key currently present in
  `src-tauri/locales/{en,ja}.json`
- `native-poc/src/cli/error.rs` — `CommandError` enum mirroring the
  src-tauri variants (`FileNotFound`, `NotAFile`, `FileReadError`,
  `FileTooLarge`, `UnsupportedImageFormat`, `ImageDecodeError`,
  `InvalidProtocol`, `EncodingError`, `NameRequired`, `PermissionDenied`),
  plus a locale-aware `Display` impl and `exit_code()`
- `native-poc/src/cli/tmux.rs` — `passthrough_if_needed` and
  `wrap_for_tmux` helpers; detection of `$TMUX`; ESC doubling
- `native-poc/src/cli/encoding/mod.rs` — module declaration
- `native-poc/src/cli/encoding/base64.rs` — `encode_base64`,
  `chunk_data` (constants for chunk sizing live alongside the
  per-subcommand handler in Phase 2)
- `native-poc/src/cli/encoding/osc.rs` — OSC frame builders for
  markdown / json / yaml (frames for image are not OSC; see Phase 3)
- `native-poc/src/cli/validation/mod.rs` — module declaration
- `native-poc/src/cli/validation/file.rs` — `open_and_validate_file`
  (TOCTOU-safe metadata + read in one fd; size cap parameter)
- `native-poc/src/lib.rs` (if not present) or equivalent re-export
  to expose `cli` as `emterm_native_poc::cli` — adjust per current
  crate layout

**Files to Modify**:
- `native-poc/Cargo.toml` — add `clap 4` (derive), `uuid 1` (v4),
  `tempfile 3` (dev-dependencies)
- `native-poc/src/main.rs` — `mod cli;` declaration and the actual
  dispatch arm are added in Phase 4; this phase only ensures `cli`
  compiles standalone

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `cli::run` | Parse args, dispatch by subcommand, format / report errors, return exit code | `args[0]` is the subcommand name | Exit code matches `CommandError::exit_code` semantics on failure, 0 on success |
| `cli::active_locale` | Resolve the active `Locale` once per process | Settings file is readable or `Language::Auto` is allowed | Returns `Locale::En` or `Locale::Ja`; result is cached in a swap-friendly cell (e.g. `Mutex<Option<Locale>>`, **not** `OnceLock`) so test-only helpers can override |
| `cli::messages::*` | Per-message localized string formatters | `Locale` argument valid | Returns owned `String` with the formatted message |
| `cli::error::CommandError` | Typed error surface for all subcommands | None | `Display` produces locale-correct text; `exit_code` matches src-tauri semantics |
| `cli::tmux::passthrough_if_needed` | Wrap inner sequence in DCS passthrough iff `$TMUX` set | Inner sequence is well-formed | When `$TMUX` set, returns wrapped string with internal ESCs doubled; else returns input unchanged |
| `cli::encoding::base64::chunk_data` | Split base64-encoded payload into fixed-size chunks | Chunk size > 0 | Returns ordered vector of chunk strings whose concatenation equals input |
| `cli::encoding::osc::build_frames` (markdown / json / yaml) | Produce begin / chunk* / end frames | `session_id` is a UUID; chunks are pre-built | Returns vector of OSC frame strings ready for tmux wrapping |
| `cli::validation::file::open_and_validate_file` | Open + size-check + not-a-directory check in one fd | Path is non-empty | Returns `(File, canonical_path)` on success; `CommandError` on failure |

**Processing Flow** (diagram-convertible):
1. `cli::run` receives args.
   - If subcommand unknown → clap auto-help / unknown-subcommand error.
   - If known → resolve locale (cached), call the subcommand handler.
2. Subcommand handler returns `Result<(), CommandError>`.
   - Ok → exit code 0.
   - Err → format via locale-aware `Display`, write to stderr, exit
     with `CommandError::exit_code`.

**Implementation Steps** (5-7 max):
1. **Add dependencies** to `native-poc/Cargo.toml` (clap, uuid, tempfile).
2. **Define `CommandError` and the locale-aware message helper** so all
   later modules can return typed errors.
3. **Port encoding utilities** (`encoding::base64` + `encoding::osc`)
   from src-tauri, preserving constants and frame format byte-for-byte.
4. **Port tmux passthrough** (`cli::tmux`) verbatim, including ESC
   doubling rules.
5. **Port the file validator** (`validation::file::open_and_validate_file`).
6. **Wire `cli::mod` skeleton** with `run`, `active_locale`, and clap
   subcommand definitions (handlers can be stub functions returning
   `Ok(())` for now; they get implemented in Phases 2 and 3).
7. **Verify the module tree compiles in isolation** (no usage from
   main.rs yet) by running `cargo check` with the native-poc target.

**Dependencies**: Requires nothing. Blocks Phase 2, 3, 4.

**Testing Approach**:
- Unit: `error` exit-code tests; locale message helper covers both
  locales for every key; `tmux` ESC-doubling and wrapping tests (≥ 9
  cases, ported from src-tauri); `encoding::base64` round-trip and
  chunk-boundary tests; `encoding::osc` frame-format tests.
- Integration: deferred to Phase 4 (no end-to-end path exists yet in
  this phase).
- E2E: none in this phase.
- Manual: none in this phase.

**Acceptance Criteria**:
- [ ] `cargo check` for native-poc passes with the new `cli` module.
- [ ] All ported unit tests pass under `cargo test` (native-poc target).
- [ ] `cargo tree -p emterm-native-poc` includes `clap` and `uuid`,
      excludes `rust-i18n`.
- [ ] `cli` module does not yet alter native-poc's runtime behavior
      (main.rs dispatch arm not yet added).

**Estimated Effort**: medium

---

### Phase 2: Text subcommands (markdown / json / yaml)

**Goal**: Deliver working `markdown`, `json`, `yaml` subcommands that
emit OSC 777 emterm frames byte-compatible with the src-tauri build.

**Files to Create**:
- `native-poc/src/cli/markdown.rs` — `execute_markdown_command`
  (without the interactive loop, which is Phase C / separate SDD)
- `native-poc/src/cli/json.rs` — `execute_json_command`
- `native-poc/src/cli/yaml.rs` — `execute_yaml_command`

**Files to Modify**:
- `native-poc/src/cli/mod.rs` — wire the three handler stubs to the
  ported implementations
- `native-poc/src/cli/encoding/osc.rs` — if any frame-builder helpers
  were left as stubs in Phase 1, finalize them here

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `cli::markdown::execute_markdown_command` | Validate input → read bytes → encode → frame → tmux wrap → stdout | File ≤ 10 MB, readable, is a regular file | Stdout contains a `begin` + `chunk*` + `end` OSC sequence with matching `session_id`; returns `Ok` |
| `cli::json::execute_json_command` | Same as markdown but verb is `json` and no file size limit | File readable, is a regular file | Same as above with verb `json` |
| `cli::yaml::execute_yaml_command` | Same shape as json with verb `yaml` | Same as json | Same as json with verb `yaml` |
| File size + chunk constants | Define `MAX_MARKDOWN_SIZE = 10 MB`, `MARKDOWN_CHUNK_SIZE = 128 KB`, `JSON_CHUNK_SIZE = 128 KB`, `YAML_CHUNK_SIZE = 128 KB` | None | Identical to src-tauri constants |

**Processing Flow** (markdown; json / yaml differ only in verb and
optional size cap):
1. Resolve the canonical path for the OSC `basedir` parameter, then
   open the file and check `metadata.len() ≤ MAX_MARKDOWN_SIZE` on the
   fd (src-tauri-equivalent flow — `image` is fully TOCTOU-safe via
   `open_and_validate_file`, see SPEC NFR2).
2. Read content into a pre-allocated buffer.
3. Generate a fresh UUID v4 as `session_id`.
4. Encode content to base64; chunk by `MARKDOWN_CHUNK_SIZE`.
5. Build `begin`, `chunk × N`, `end` OSC frames per the SPEC's "OSC
   Frame Format" — `begin` carries `;format=gfm;render=fullscreen;version=1.0[;basedir=…]`,
   `end` carries the bare `id={uuid}` plus `;interactive=1` only when
   interactive (no `total=`).
6. Apply tmux passthrough if `$TMUX` is set.
7. Write to stdout, flush, return `Ok`.
   - Any error path returns `Err(CommandError::*)` bubbled to `cli::run`.

**Implementation Steps** (5-7 max):
1. **Port markdown handler** (without interactive loop) preserving
   size cap, chunk size, frame format, session_id flow.
2. **Port json handler** (no size cap, 128 KB chunks).
3. **Port yaml handler** (same as json, verb-only difference).
4. **Wire the three handlers** into `cli::mod`'s dispatch.
5. **Port the unit tests** from src-tauri (markdown ~50 / json 3 /
   yaml 3), adapting `t!`/`rust_i18n::set_locale` calls to use the
   Phase 1 locale helper.

**Dependencies**: Requires Phase 1. Blocks Phase 4 (integration into
main.rs is bundled with Phase 4 to keep behavior reversible until
both Phase 2 and Phase 3 are complete).

**Testing Approach**:
- Unit: markdown happy path (small / boundary / oversized), empty
  file, embedded image references, UTF-8 / non-UTF-8 bytes; json &
  yaml happy paths; verb correctness on frame builders.
- Integration: skipped here — exercised in Phase 4 via integration
  tests.
- E2E: skipped here.
- Manual: skipped here.

**Acceptance Criteria**:
- [ ] All ported markdown / json / yaml unit tests pass.
- [ ] Frame format strings are byte-identical to src-tauri's output
      (verified by comparing test expectations).
- [ ] No `rust-i18n` usage anywhere in handlers.

**Estimated Effort**: medium

---

### Phase 3: Image subcommand (Kitty + SIXEL)

**Goal**: Deliver the `image` subcommand with both encoders, matching
src-tauri capabilities (file ≤ 10 MB, dimensions ≤ 8192×8192, Unix
stdin drain after Kitty output).

**Files to Create**:
- `native-poc/src/cli/validation/image.rs` — magic-byte image format
  validator (PNG / JPEG / GIF / WebP recognition; unsupported format
  rejection)
- `native-poc/src/cli/protocols/mod.rs` — module declaration
- `native-poc/src/cli/protocols/kitty.rs` — APC sequence builder
  (`generate_kitty_sequence`), output id allocation
- `native-poc/src/cli/protocols/sixel.rs` — DCS SIXEL builder
  (`generate_sixel_sequence`)
- `native-poc/src/cli/image.rs` — `ImageProtocol` enum,
  `execute_image_command`, and the Unix-only `drain_stdin_responses`
  helper (cfg-gated)

**Files to Modify**:
- `native-poc/src/cli/mod.rs` — wire the `image` subcommand handler
  and `--protocol` argument

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `ImageProtocol` | Typed enum `{Kitty, Sixel}` with string parser | None | `parse("kitty")` / `parse("sixel")` succeed; others return `CommandError::InvalidProtocol` |
| `validation::image::validate_image_format` | Inspect magic bytes; accept PNG/JPEG/GIF/WebP | File is open for reading | Returns `Ok` or `CommandError::UnsupportedImageFormat` |
| `protocols::kitty::generate_kitty_sequence` | Build APC `_Gi=…` frames over base64-encoded RGBA chunks | Image decoded, no dim cap violation | Returns `(sequence_string, image_id)` |
| `protocols::sixel::generate_sixel_sequence` | Build DCS `q…` SIXEL data | Image decoded, no dim cap violation | Returns sequence string |
| `cli::image::execute_image_command` | Orchestrate: validate → dim check → decode → encode → tmux wrap → stdout → drain (Unix) | File ≤ 10 MB, dims ≤ 8192×8192 | Sequence written; on Unix, stdin drained for ~2 s |
| `drain_stdin_responses` (Unix only) | Absorb Kitty `Gi=…OK` response bytes from stdin | Stdin is a TTY | Up to ~2 s of stdin consumed via raw termios; original termios restored |

**Processing Flow**:
1. Parse `--protocol` (default `kitty`); on error → `InvalidProtocol`.
2. Open + validate file with `MAX_IMAGE_SIZE` cap via
   `open_and_validate_file` (returns the validated `File` fd).
3. Validate magic bytes against the fd; reject unsupported formats.
4. Read the entire fd into an in-memory `Vec<u8>` (size already capped
   in step 2), then drop the fd. All subsequent operations use this
   buffer to close the TOCTOU window that the legacy
   `image_dimensions(path)` + `image::open(path)` pair had.
5. Probe dimensions from the buffer (`ImageReader::with_guessed_format`
   + `into_dimensions`); reject if either dimension > 8192.
6. Decode the image from the same buffer (`image::load_from_memory`).
7. Switch on protocol:
   - Kitty → call APC builder.
   - SIXEL → call DCS builder.
8. Apply tmux passthrough if `$TMUX` set.
9. Write to stdout and flush.
10. On Unix only, drain stdin briefly.

**Implementation Steps** (5-7 max):
1. **Port `validation::image`** (magic-byte recognition).
2. **Port `protocols::kitty`** verbatim.
3. **Port `protocols::sixel`** verbatim.
4. **Port `cli::image`** including `ImageProtocol`, the orchestrator,
   and the cfg-gated stdin drain.
5. **Wire `--protocol` argument** into the clap subcommand definition;
   default `kitty`.
6. **Port the image unit tests** (~8 cases), adapting locale helper
   usage.

**Dependencies**: Requires Phase 1. Blocks Phase 4.

**Testing Approach**:
- Unit: protocol parsing (kitty / sixel / invalid); valid PNG with
  Kitty; valid PNG with SIXEL; missing file; oversize file; dimension
  cap; max-dimension constant value sanity.
- Integration: skipped here — exercised in Phase 4.
- E2E: skipped here.
- Manual: post-Phase-4 manual screenshot verification.

**Acceptance Criteria**:
- [ ] All ported image unit tests pass.
- [ ] Kitty output bytes match src-tauri's output for an identical
      input image.
- [ ] SIXEL output bytes match src-tauri's output for an identical
      input image.
- [ ] `drain_stdin_responses` is `#[cfg(unix)]`-gated; Windows builds
      compile without it.

**Estimated Effort**: medium

---

### Phase 4: main.rs integration + integration tests

**Goal**: Hook `cli::run` into `main.rs`, add integration tests
covering happy paths and tmux passthrough, run release build to
confirm artifact location and binary works end-to-end.

**Files to Create**:
- `native-poc/tests/cli_subcommands.rs` — integration tests that
  spawn `emterm-native-poc <subcommand> ...` against fixtures and
  assert stdout shape / tmux wrapping / exit codes
- `native-poc/src/lib.rs` — **new library target** that hosts
  `pub mod cli;` plus any modules `cli` depends on (`i18n`, `settings`,
  …). Created to enable in-process integration tests (currently
  native-poc has no library target; tests cannot reach `cli` symbols
  otherwise). `main.rs` then imports from this library crate via
  `use emterm_native_poc::cli;`.

**Files to Modify**:
- `native-poc/src/main.rs` — switch from inline module declarations
  to `use emterm_native_poc::*;`-style imports for the modules now
  hosted in `lib.rs`, then add the CLI dispatch arm
  placed *before* existing `--viewer` / `--settings` / `--image-viewer`
  / `--data-viewer` / mux branches
- `native-poc/Cargo.toml` — add a `[lib]` target alongside the
  existing `[[bin]]` (path `src/lib.rs`, name `emterm_native_poc`)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `main.rs` CLI arm | If `args[1] ∈ {markdown, json, yaml, image}` → call `cli::run` and `exit` | Args parsed | Process exits without touching the existing child-process flag handling |
| Integration test fixtures | Sample .md / .json / .yaml / .png files | None | Available under `native-poc/tests/fixtures/` (mirror src-tauri's layout where reasonable) |
| Integration tests | Spawn the built binary or invoke `cli::run` in-process; assert OSC framing, exit codes, tmux wrapping | Fixtures exist | All tests pass |

**Processing Flow**:
1. `main.rs` reads `std::env::args` (existing behavior).
2. **New**: inspect `args[1]`; if it is a known subcommand, delegate
   to `cli::run` and exit.
3. Else: fall through to existing `--viewer` / `--settings` / `--image-viewer`
   / `--data-viewer` / mux / terminal-startup handling unchanged.

**Implementation Steps** (5-7 max):
1. **Add the dispatch arm** to `main.rs` (single early-return block).
2. **Verify isolation**: existing internal-flag startup paths
   unaffected (`--viewer foo` still routes to the viewer child as
   before).
3. **Add integration tests** under `native-poc/tests/cli_subcommands.rs`.
4. **Add fixtures** under `native-poc/tests/fixtures/` for markdown /
   json / yaml / image.
5. **Run** `cargo test` (native-poc target) and resolve any
   integration failures.
6. **Build the release binary** to `native-poc/target-host/release/`
   per `.claude/rules/native-poc-build-location.md` and confirm the
   path.

**Dependencies**: Requires Phases 1, 2, 3. Blocks none (end of plan).

**Testing Approach**:
- Unit: covered in prior phases.
- Integration: per-subcommand happy path; per-subcommand failure
  paths (missing file, oversize, invalid protocol); tmux wrapping
  (with `TMUX` env var set).
- E2E: the project's WebDriver E2E suite does not currently target
  native-poc, so no automated E2E additions here.
- Manual: launch the freshly built `native-poc/target-host/release/emterm-native-poc`,
  open a shell inside it, run each subcommand against fixtures, verify
  the viewer / data window / inline image appears.

**Acceptance Criteria**:
- [ ] `main.rs` dispatch arm precedes the `--viewer` family of branches.
- [ ] All integration tests pass.
- [ ] `cargo build --release --manifest-path native-poc/Cargo.toml`
      (with `CARGO_TARGET_DIR=native-poc/target-host`) produces the
      expected binary.
- [ ] Manual verification (developer-driven) of all four subcommands
      against real fixtures succeeds.
- [ ] No regression in existing native-poc behavior (`--viewer`,
      `--settings`, `--image-viewer`, `--data-viewer`, mux, terminal
      startup).

**Estimated Effort**: small

---

## Complete File Structure

```
emterm/
└── native-poc/
    ├── Cargo.toml                          # MODIFIED (Phase 1)
    ├── src/
    │   ├── main.rs                         # MODIFIED (Phase 4)
    │   ├── lib.rs (or cli re-export)       # MODIFIED (Phase 1 or 4)
    │   └── cli/                            # NEW (Phase 1-3)
    │       ├── mod.rs                      # NEW (Phase 1, refined in 2/3)
    │       ├── messages.rs                 # NEW (Phase 1)
    │       ├── error.rs                    # NEW (Phase 1)
    │       ├── tmux.rs                     # NEW (Phase 1)
    │       ├── encoding/
    │       │   ├── mod.rs                  # NEW (Phase 1)
    │       │   ├── base64.rs               # NEW (Phase 1)
    │       │   └── osc.rs                  # NEW (Phase 1)
    │       ├── validation/
    │       │   ├── mod.rs                  # NEW (Phase 1)
    │       │   ├── file.rs                 # NEW (Phase 1)
    │       │   └── image.rs                # NEW (Phase 3)
    │       ├── protocols/
    │       │   ├── mod.rs                  # NEW (Phase 3)
    │       │   ├── kitty.rs                # NEW (Phase 3)
    │       │   └── sixel.rs                # NEW (Phase 3)
    │       ├── markdown.rs                 # NEW (Phase 2)
    │       ├── json.rs                     # NEW (Phase 2)
    │       ├── yaml.rs                     # NEW (Phase 2)
    │       └── image.rs                    # NEW (Phase 3)
    └── tests/
        ├── cli_subcommands.rs              # NEW (Phase 4)
        └── fixtures/                       # NEW (Phase 4)
            ├── markdown/
            ├── data/
            └── images/
```

No file outside `native-poc/` is created or modified.

## Testing Strategy

- **Unit**: core logic of every ported module covered. Target ≥ 80 %
  for the new `cli` subtree; ≥ 90 % for error / validation paths
  (matching src-tauri's coverage policy in `test/README.md`).
- **Integration**: per-subcommand spawn tests with stdout / exit-code
  assertions; tmux wrapping toggled via env var.
- **E2E**: the project's WebDriver E2E does not target native-poc.
  No additions in this SDD.
- **Manual**: post-Phase-4 launch of the release binary and one-shot
  invocation of each subcommand against fixtures.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `clap` | 4 (derive) | Subcommand parsing |
| `uuid` | 1 (v4) | OSC session_id |
| `tempfile` | 3 | dev-dependency for tests |

No other crates are added. `rust-i18n` is **not** introduced.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Frame format drift from src-tauri | Medium | High (receiver-side incompatibility) | Port byte-format constants verbatim; cross-check test expectations against src-tauri's |
| Locale dispatch hot path overhead | Low | Low | `OnceLock<Locale>` cache; settings read at most once per process |
| Windows compile break from libc usage | Medium | Medium | `drain_stdin_responses` is the only libc usage; gate with `#[cfg(unix)]` and provide a no-op stub for Windows |
| Existing internal flags collide with new subcommands | Very Low | High | Internal flags are `--`-prefixed; new subcommands are bare words. Dispatch arm checks bare words first; flag dispatch unchanged |
| Test flakiness on stdin drain (Kitty) | Low | Medium | Drain is bounded to ~2 s VTIME timeout; no test directly invokes drain (Unix-only, real TTY) |

## Open Questions

- [ ] Should `cli::active_locale` derive its `Language` setting from
      `crate::settings::load_settings()` (full settings file load) or
      from a lightweight env var lookup? — Decision deferred to
      Phase 1; default to settings load for parity with the rest of
      native-poc; revisit if hot-path overhead is observed.
- [ ] Should integration tests spawn the binary or call `cli::run`
      in-process? — Decision deferred to Phase 4; default plan is
      in-process invocation for CI speed, plus one spawn-based smoke
      test per subcommand.
- [ ] How exactly to overwrite the cached active locale from test code
      (e.g. a `#[cfg(test)] pub fn set_active_locale_for_test(loc:
      Locale)` helper, or pass `Locale` as a per-call argument to
      messages)? — Decision deferred to Phase 1 (default: provide a
      cfg(test)-only setter on `cli::active_locale` plus
      `messages::*` helpers that accept `Locale` directly so
      pure-unit tests need not touch the cache).

## Success Metrics

- [ ] All `cli::*` subcommands behave identically to the src-tauri
      versions (within the Phase A + B scope).
- [ ] `cargo tree -p emterm-native-poc` does not contain `rust-i18n`.
- [ ] Unit + integration tests pass under
      `CARGO_TARGET_DIR=native-poc/target cargo test --manifest-path
      native-poc/Cargo.toml`.
- [ ] Release binary at `native-poc/target-host/release/emterm-native-poc`
      builds without error.
- [ ] Manual verification: each subcommand against a real fixture in
      a real native-poc terminal triggers the expected viewer / image
      output.
