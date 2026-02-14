# Implementation Plan: Unicode Width Calculation via WebAssembly

## Overview

Port the Unicode character width calculation module (`src/terminal/unicode.ts`) to Rust/WebAssembly, providing both a batch classification API and individual function exports. Establish the WASM build pipeline (wasm-pack) as a foundation for future WASM expansions.

## Objectives

- Port all 10 Unicode property functions from TypeScript to Rust/WASM
- Provide a batch classification API that returns packed properties in a single JS-WASM call
- Establish the wasm-pack build pipeline integrated with the existing Bun/Tauri toolchain
- Achieve >= 1.5x performance improvement over the TypeScript implementation

## Prerequisites

### Development Environment

- Rust toolchain (rustup, cargo) - already available for src-tauri
- wasm-pack CLI installed
- wasm32-unknown-unknown target added to rustup

### Dependencies

- `wasm-bindgen = "0.2"` (Rust crate for WASM bindings)
- No additional npm dependencies (wasm-pack generates JS bindings)

### Knowledge Requirements

- wasm-bindgen attribute macros and data types
- wasm-pack build targets (`--target web` for Tauri WebView)
- WASM module initialization in browser/WebView context

## Architecture Overview

### Technology Stack

- **Language**: Rust (WASM module), TypeScript (glue code)
- **Build Tool**: wasm-pack (`--target web`)
- **Key Libraries**:
  - `wasm-bindgen` - Rust-JS interop bindings

### Design Approach

Pure function port: the Unicode property logic is entirely stateless (input: codepoint, output: width/flags). The Rust implementation mirrors the TypeScript logic exactly, using the same range checks and conditions. The batch API iterates codepoints in WASM linear memory and returns a packed byte array, minimizing JS-WASM boundary crossings.

### Component Interaction

```
wasm/src/unicode.rs  →  wasm/src/lib.rs  →  wasm/pkg/*.js  →  src/terminal/wasm/unicode.ts
(pure Rust functions)    (#[wasm_bindgen])    (auto-generated)   (TS glue + re-exports)
                                                                         ↓
                                                        print_handler.ts, grid.ts, state.ts
```

## Implementation Phases

### Phase 1: WASM Crate Setup and Build Pipeline

**Goal**: Create the `wasm/` Rust crate, establish wasm-pack build, and verify WASM loads in the Tauri WebView.

**Files to Create**:
- `wasm/Cargo.toml` - Crate manifest with wasm-bindgen dependency, `crate-type = ["cdylib"]`
- `wasm/src/lib.rs` - Minimal WASM export (a trivial function to verify the pipeline works)
- `wasm/src/unicode.rs` - Empty module placeholder

**Files to Modify**:
- `package.json` - Add `build:wasm` script, update `dev` and `build` to include WASM pre-build
- `.gitignore` - Add `wasm/pkg/` and `wasm/target/`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| wasm/Cargo.toml | Define crate as cdylib with wasm-bindgen | None | wasm-pack build succeeds |
| wasm/src/lib.rs | Export a trivial function via wasm_bindgen | Cargo.toml configured | Function callable from JS |
| build:wasm script | Run wasm-pack build --target web | wasm-pack installed | pkg/ directory populated with .wasm, .js, .d.ts |

**Processing Flow**:
```
1. Create wasm/ directory with Cargo.toml and minimal lib.rs
2. Run wasm-pack build → generates pkg/ artifacts
3. Add build:wasm to package.json scripts
4. Verify: import WASM module in browser console → trivial function returns expected value
```

**Implementation Steps**:

1. **Create Rust crate skeleton**
   - Set up Cargo.toml with cdylib crate type and wasm-bindgen dependency
   - Create lib.rs with a single trivial exported function
   - Key consideration: crate name determines generated JS module name

2. **Configure build pipeline**
   - Add `build:wasm` script to package.json
   - Chain WASM build before dev/build commands
   - Key consideration: pkg/ output path must be importable from src/

3. **Update .gitignore**
   - Add wasm/pkg/ and wasm/target/ entries

4. **Verify end-to-end**
   - Run `bun run build:wasm` and confirm pkg/ generation
   - Load in WebView and call the trivial function from JS console

**Dependencies**:
- Requires: wasm-pack installed on system
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Manual Verification*:
- `bun run build:wasm` exits with code 0
- `wasm/pkg/` contains .wasm, .js, .d.ts files
- Trivial function callable from WebView console

**Acceptance Criteria**:
- [ ] `wasm/Cargo.toml` exists and `wasm-pack build --target web` succeeds
- [ ] `bun run build:wasm` generates `wasm/pkg/` with .wasm, .js, .d.ts
- [ ] .gitignore includes wasm/pkg/ and wasm/target/
- [ ] Trivial WASM function is callable from the WebView

**Estimated Effort**: 小 (1 day)

---

### Phase 2: Unicode Functions Rust Implementation

**Goal**: Implement all 10 Unicode property functions in Rust with the batch classification API, verified by Rust unit tests.

**Files to Create**:
- `wasm/src/unicode.rs` - All Unicode property functions (pure Rust, no wasm_bindgen)

**Files to Modify**:
- `wasm/src/lib.rs` - Add `#[wasm_bindgen]` exports: `classify_codepoints`, `char_width`, `string_width`, and individual property checks

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| unicode.rs::char_width | Return display width (0/1/2) for a codepoint | Valid u32 | Same result as TS charWidth() |
| unicode.rs::classify_codepoint | Pack all properties into a single byte | Valid u32 | Byte with correct bit flags |
| lib.rs::classify_codepoints | Iterate string codepoints, return packed Vec<u8> | Valid UTF-8 string | Vec<u8> with one byte per codepoint |
| lib.rs::string_width | Sum char_width over all codepoints in string | Valid UTF-8 string | Total display width as u32 |

**Processing Flow**:
```
1. Implement individual property check functions in unicode.rs
   ├─ Each mirrors the corresponding TypeScript function exactly
   └─ Uses the same range checks and conditions
2. Implement classify_codepoint combining all checks into packed byte
3. Implement classify_codepoints iterating over string codepoints
4. Implement string_width summing char_width results
5. Write Rust unit tests validating against known values from TS tests
```

**Implementation Steps**:

1. **Implement core property functions**
   - Port each function from TypeScript, preserving identical range checks
   - Functions: is_zero_width, is_emoji_presentation, is_extended_pictographic, is_regional_indicator, is_skin_tone_modifier, is_variation_selector, is_combining_char, is_wide_code_point
   - Key consideration: Rust u32 maps directly to Unicode codepoints, no string conversion needed

2. **Implement char_width and classify_codepoint**
   - char_width follows the same decision tree as the TS version (fast path for ASCII, then zero-width, emoji, Latin, wide, combining checks)
   - classify_codepoint packs width + all flags into a single u8 using bitwise OR
   - Key consideration: bit layout must match the spec (width in bits 0-1, flags in bits 2-7)

3. **Implement wasm_bindgen exports**
   - classify_codepoint: accepts u32, returns u8 (packed byte with width + flags)
   - classify_codepoints: accepts &str, iterates .chars(), returns Vec<u8>
   - char_width: accepts u32, returns u8
   - string_width: accepts &str, returns u32
   - Individual property checks (is_emoji_presentation, is_extended_pictographic, is_regional_indicator, is_skin_tone_modifier, is_variation_selector, is_combining_char): accept u32, return bool

4. **Write comprehensive Rust tests**
   - Use the same test values from unicode.test.ts
   - Cover: ASCII, control chars, CJK, emoji, combining, zero-width, boundary cases
   - Key consideration: tests run via `cargo test` without wasm-pack (pure Rust)

**Dependencies**:
- Requires: Phase 1 (crate skeleton)
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests (Rust)*:
- All char_width values match TS charWidth for documented codepoints
- classify_codepoint byte layout verified for each property independently
- classify_codepoints on mixed strings returns correct length and values
- string_width matches TS stringWidth for test strings
- Edge cases: empty string, single char, max codepoint (U+10FFFF)

**Acceptance Criteria**:
- [ ] All 10 Unicode property functions implemented in Rust
- [ ] classify_codepoints batch API returns correct packed bytes
- [ ] `cargo test` passes all Rust unit tests
- [ ] `wasm-pack build` succeeds with all exports

**Estimated Effort**: 中 (2-3 days)

---

### Phase 3: TypeScript Integration

**Goal**: Create TypeScript glue code, update all consumers to use WASM, and verify correctness via existing tests.

**Files to Create**:
- `src/terminal/wasm/loader.ts` - WASM module initialization
- `src/terminal/wasm/unicode.ts` - TypeScript interface wrapping WASM exports, bit flag constants

**Files to Modify**:
- `src/main.ts` - Call initWasm() at application startup before terminal initialization
- `src/terminal/handlers/print_handler.ts` - Replace per-character Unicode function calls with batch API
- `src/terminal/grid.ts` - Change charWidth import to WASM glue
- `src/terminal/state.ts` - Change isEmojiPresentation import to WASM glue
- `src/terminal/index.ts` - Change re-export source to WASM glue

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| wasm/loader.ts | Load and initialize WASM module | .wasm file in pkg/ | Module ready for function calls |
| wasm/unicode.ts | Provide TS-friendly wrappers and bit constants | WASM initialized | Same API surface as original unicode.ts |
| print_handler.ts (modified) | Use batch classification for character processing | WASM initialized | Reduced JS-WASM boundary crossings |

**Processing Flow**:
```
1. Create loader.ts with initWasm() async function
2. Create wasm/unicode.ts with:
   ├─ Bit flag constants (WIDTH_MASK, COMBINING, etc.)
   ├─ classifyCodepoints() wrapper
   ├─ charWidth(), stringWidth(), isWideChar() wrappers
   └─ Individual property check wrappers
3. Update main.ts to call initWasm() early in startup
4. Update print_handler.ts to use WASM-backed Unicode functions:
   ├─ handlePrintDispatch remains per-character (ANSI parser dispatches Print(char) individually)
   ├─ Grapheme buffer logic: use WASM-backed individual property functions
   └─ Slow path: use classify_codepoint for combined width + property lookup
5. Update grid.ts, state.ts, index.ts import paths
6. Verify all existing tests pass
```

**Implementation Steps**:

1. **Create WASM loader**
   - Async initialization function that loads the .wasm binary
   - Guard against double initialization
   - Key consideration: must complete before any terminal processing begins

2. **Create TypeScript Unicode interface**
   - Export same function signatures as original unicode.ts
   - Export bit flag constants for batch API consumers
   - classifyCodepoints wraps WASM classify_codepoints
   - Key consideration: charWidth(char) must accept string (like original), extract codepoint internally

3. **Integrate WASM initialization in main.ts**
   - Call initWasm() in the main() function before terminal setup
   - Key consideration: initWasm is async; must await before proceeding

4. **Update print_handler.ts to use WASM-backed functions**
   - Import classifyCodepoint, bit flag constants, and individual property functions from wasm/unicode.ts
   - Note: handlePrintDispatch processes one character at a time (ANSI parser dispatches `Print(char)` individually). The batch API cannot be used here.
   - Replace multiple per-character property calls with single `classifyCodepoint(cp)` call + bit decoding where possible
   - For grapheme buffer logic (where individual property checks are needed in conditional branches), use WASM-backed individual functions (isVariationSelector, isSkinToneModifier, etc.)
   - Key consideration: the grapheme buffer section checks properties conditionally, so classify_codepoint is most beneficial in the slow path (`handlePrintSlow`) where charWidth + property checks happen together

5. **Update remaining consumers**
   - grid.ts: change import source for charWidth
   - state.ts: change import source for isEmojiPresentation
   - index.ts: change re-export source
   - Key consideration: all imports must resolve after WASM is initialized

**Dependencies**:
- Requires: Phase 2 (Rust functions complete)
- Blocks: Phase 4

**Testing Approach**:

*Integration Tests (TypeScript, bun test)*:
- All existing unicode.test.ts test cases pass when using WASM-backed functions
- print_handler behavior unchanged (existing handler tests pass)
- WASM module loads without errors

*Manual Testing*:
- `bun tauri dev` starts without errors
- Terminal renders text correctly (ASCII, CJK, emoji)
- Grapheme cluster handling works (emoji sequences, flags)

**Acceptance Criteria**:
- [ ] initWasm() called in main.ts before terminal initialization
- [ ] All existing TypeScript tests pass (`bun test`)
- [ ] print_handler.ts uses WASM-backed Unicode functions (classifyCodepoint + individual property checks)
- [ ] grid.ts, state.ts, index.ts updated to import from wasm/unicode.ts
- [ ] `bun tauri dev` starts and terminal works correctly

**Estimated Effort**: 中 (2-3 days)

---

### Phase 4: Verification and Benchmarking

**Goal**: Cross-validate WASM results against TypeScript implementation, measure performance, and ensure CI compatibility.

**Files to Create**:
- `wasm/benches/unicode_bench.rs` - Rust benchmark (optional, for WASM-level perf measurement)

**Files to Modify**:
- `docker-compose.e2e.yml` - Add wasm-pack to Docker build image (if needed for CI)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cross-validation test | Compare WASM vs TS for large codepoint ranges | Both implementations available | All results match |
| Performance benchmark | Measure WASM vs TS execution time | Both implementations available | >= 1.5x improvement documented |

**Processing Flow**:
```
1. Write cross-validation test comparing WASM and TS for comprehensive codepoint ranges
2. Run benchmark comparing classify_codepoints performance (WASM vs TS loop)
3. Verify Docker build includes wasm-pack for CI
4. Document results and clean up
```

**Implementation Steps**:

1. **Cross-validation testing**
   - Test full BMP range (U+0000 to U+FFFF) comparing WASM char_width vs TS charWidth
   - Test SMP emoji blocks (U+1F000 to U+1FFFF) - covers Regional Indicators, Skin Tone Modifiers, Emoji_Presentation, Extended_Pictographic
   - Test Variation Selectors Supplement (U+E0100 to U+E01EF)
   - Test all property functions individually (isEmojiPresentation, isExtendedPictographic, etc.) for documented ranges
   - Key consideration: keep original unicode.ts available for comparison

2. **Performance benchmarking**
   - Measure time for classifyCodepoints on realistic PTY data (mixed ASCII/CJK/emoji)
   - Compare against equivalent TS loop calling charWidth per character
   - Key consideration: run multiple iterations and report median

3. **CI/Docker integration**
   - Verify wasm-pack is available or installable in the Docker build image
   - Add wasm build step to CI pipeline if needed

**Dependencies**:
- Requires: Phase 3 (integration complete)
- Blocks: None

**Testing Approach**:

*Cross-validation*:
- Iterate U+0000..U+FFFF comparing results between WASM and TS (full BMP)
- Iterate U+1F000..U+1FFFF comparing results (SMP emoji blocks)
- Iterate U+E0100..U+E01EF comparing results (Variation Selectors Supplement)
- Validate all individual property functions for their documented ranges

*Performance*:
- Benchmark with 10,000 character mixed string
- Target: >= 1.5x improvement

**Acceptance Criteria**:
- [ ] Cross-validation passes for full BMP, SMP emoji blocks (U+1F000..U+1FFFF), and VS Supplement (U+E0100..U+E01EF)
- [ ] Performance benchmark shows >= 1.5x improvement
- [ ] Docker build can produce WASM artifacts

**Estimated Effort**: 小 (1-2 days)

---

## Complete File Structure

```
wasm/                                    # NEW: WASM crate
├── Cargo.toml                           # Crate manifest (cdylib, wasm-bindgen)
├── src/
│   ├── lib.rs                           # #[wasm_bindgen] exports
│   └── unicode.rs                       # Unicode property functions (pure Rust, #[cfg(test)] inline tests)
├── benches/
│   └── unicode_bench.rs                 # Performance benchmarks (optional)
└── pkg/                                 # wasm-pack output (gitignored)
    ├── emterm_wasm.js
    ├── emterm_wasm_bg.wasm
    └── emterm_wasm.d.ts

src/terminal/wasm/                       # NEW: TypeScript glue
├── loader.ts                            # WASM module initialization
└── unicode.ts                           # TS interface + bit flag constants

src/terminal/unicode.ts                  # EXISTING: kept for test reference
src/terminal/handlers/print_handler.ts   # MODIFIED: use WASM-backed functions
src/terminal/grid.ts                     # MODIFIED: import from wasm/unicode.ts
src/terminal/state.ts                    # MODIFIED: import from wasm/unicode.ts
src/terminal/index.ts                    # MODIFIED: re-export from wasm/unicode.ts
src/main.ts                              # MODIFIED: call initWasm() at startup

package.json                             # MODIFIED: add build:wasm script
.gitignore                               # MODIFIED: add wasm/pkg/, wasm/target/
```

## Testing Strategy

### Unit Testing (Rust)

**Approach**: Standard `#[cfg(test)]` module in unicode.rs, runnable via `cargo test`

**Test Coverage Goals**:
- Unicode property functions: 100% of documented ranges
- classify_codepoints: all property combinations
- char_width: all width categories (0, 1, 2)

### Integration Testing (TypeScript)

**Approach**: Existing `bun test` infrastructure with unicode.test.ts as baseline

**Test Coverage Goals**:
- All existing unicode.test.ts cases pass through WASM path
- WASM initialization does not break existing functionality

### Cross-Validation

**Approach**: Compare WASM and TS results for comprehensive codepoint ranges

### E2E Testing (Docker)

- [ ] `bun run build:wasm` succeeds in Docker environment
- [ ] `cargo test --manifest-path wasm/Cargo.toml` passes in Docker
- [ ] `bun test` passes with WASM integration

### Manual Testing

- [ ] `bun tauri dev` starts and terminal renders correctly
- [ ] CJK text, emoji, combining characters display properly
- [ ] No visual regressions in terminal output

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| wasm-bindgen | 0.2 | Rust-JS WASM bindings | Cargo dependency |
| wasm-pack | latest | WASM build tool | `cargo install wasm-pack` or system package |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: WASM crate setup (no dependencies)
2. Phase 2: Rust implementation (depends on Phase 1)
3. Phase 3: TypeScript integration (depends on Phase 2)
4. Phase 4: Verification (depends on Phase 3)

## Risk Assessment

### Technical Risks

1. **wasm-pack `--target web` compatibility with Bun bundler**
   - **Risk**: Generated JS bindings may not work with Bun's module resolution
   - **Likelihood**: Low (standard ES module output)
   - **Impact**: High (blocks integration)
   - **Mitigation**: Test early in Phase 1; fall back to `--target bundler` if needed

2. **WASM module initialization timing**
   - **Risk**: Terminal processing starts before WASM is ready
   - **Likelihood**: Low (async/await in main.ts)
   - **Impact**: High (runtime errors)
   - **Mitigation**: Await initWasm() before any terminal creation

3. **Batch API integration complexity in print_handler**
   - **Risk**: Per-character grapheme buffer logic makes batch integration limited
   - **Likelihood**: Medium
   - **Impact**: Low (individual function calls still work, just slower than batch)
   - **Mitigation**: Use batch where possible, individual calls where grapheme buffering requires it

## Performance Considerations

- **ASCII fast path**: Early return for 0x20-0x7E in Rust (same as TS)
- **Batch processing**: Single JS-WASM boundary crossing for classify_codepoints
- **Linear memory**: Property checks execute entirely in WASM linear memory
- **LLVM optimization**: Rust compiler may generate lookup tables for range checks

## Open Questions

### Implementation-Specific

- [ ] Does Bun's bundler handle wasm-pack `--target web` output correctly, or is `--target bundler` needed?
- [ ] Should the WASM .wasm file be embedded in the JS bundle or loaded separately?

## Success Metrics

### Functional Completeness
- [ ] All 10 Unicode functions ported to Rust
- [ ] Batch API working
- [ ] All existing tests pass

### Quality Metrics
- [ ] Rust unit tests cover all Unicode ranges from TS tests
- [ ] Cross-validation passes for full BMP
- [ ] No visual regressions

### Performance Metrics
- [ ] >= 1.5x improvement in Unicode width calculation
- [ ] WASM binary < 100KB

## References

- **Specification**: `doc/tasks/wasm-unicode-width/SPEC.md`
- **Requirements**: `doc/tasks/wasm-unicode-width/要件定義書.md`
- **Current TS implementation**: `src/terminal/unicode.ts`
- **Current TS tests**: `src/terminal/unicode.test.ts`
- **WASM report**: `tmp/wasm.md`
- **wasm-pack docs**: https://rustwasm.github.io/wasm-pack/
- **wasm-bindgen docs**: https://rustwasm.github.io/wasm-bindgen/
