# Feature: Unicode Width Calculation via WebAssembly

## Overview

Port the Unicode character width calculation module (`src/terminal/unicode.ts`, ~370 lines) to Rust/WebAssembly. The WASM module provides a batch classification API that returns packed Unicode properties for each codepoint in a string, minimizing JS-WASM boundary crossings. Individual function APIs are also provided for single-character operations.

## Objectives

- Port all Unicode property functions from TypeScript to Rust/WASM
- Provide a batch classification API (`classify_codepoints`) that returns packed properties per codepoint
- Establish the WASM build pipeline (wasm-pack) for future WASM expansions
- Achieve 1.5-2x performance improvement over the TypeScript implementation

## User Stories

### US1: Batch Unicode Classification in Print Handler

As the print handler, I want to classify all codepoints in a PTY data chunk in a single WASM call, so that per-character JS-WASM boundary overhead is eliminated.

**Acceptance Criteria:**
- [ ] `classify_codepoints(text)` returns a `Uint8Array` with one byte per codepoint
- [ ] Each byte contains packed width (2 bits) and property flags (6 bits)
- [ ] Results are identical to the current TypeScript implementation for all test cases

### US2: Single Character Width in Grid

As the grid module, I want to get the display width of a single character via WASM, so that cell creation uses the optimized implementation.

**Acceptance Criteria:**
- [ ] `char_width(codepoint)` returns 0, 1, or 2
- [ ] Results match the TypeScript `charWidth()` for all Unicode codepoints

### US3: String Width Calculation

As any module needing total string width, I want to compute it in a single WASM call, so that iteration happens in WASM linear memory without JS overhead.

**Acceptance Criteria:**
- [ ] `string_width(text)` returns the total display width
- [ ] Results match the TypeScript `stringWidth()` for all test strings

## Technical Requirements

### Functional Requirements

- **FR1:** All 10 functions from `unicode.ts` are implemented in Rust with identical behavior
- **FR2:** Single-codepoint classification API (`classify_codepoint`) returns a packed byte with width + property flags
- **FR2b:** Batch API (`classify_codepoints`) accepts a UTF-8 string and returns `Vec<u8>` (one byte per codepoint)
- **FR3:** Individual APIs (`char_width`, `string_width`, and all property check functions) are exported via `#[wasm_bindgen]`
- **FR4:** Unicode 17.0 / Emoji 17.0 coverage (matching existing implementation)
- **FR5:** WASM module initializes at application startup before any terminal processing

### Non-Functional Requirements

- **NFR1 - Performance:** Unicode width calculation 1.5x faster than TypeScript (measured by benchmark)
- **NFR2 - Size:** WASM binary < 100KB (Unicode lookup tables only)
- **NFR3 - Compatibility:** Works in Tauri WebView (WebKit/Chromium depending on platform)

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│ wasm/ (Rust Crate)                              │
│                                                  │
│  src/unicode.rs                                  │
│    - char_width(cp) -> u8                        │
│    - is_emoji_presentation(cp) -> bool           │
│    - is_wide_code_point(cp) -> bool              │
│    - is_combining_char(cp) -> bool               │
│    - is_zero_width(cp) -> bool                   │
│    - is_extended_pictographic(cp) -> bool         │
│    - is_regional_indicator(cp) -> bool            │
│    - is_skin_tone_modifier(cp) -> bool            │
│    - is_variation_selector(cp) -> bool            │
│                                                  │
│  src/lib.rs (#[wasm_bindgen] exports)            │
│    - classify_codepoint(cp: u32) -> u8           │
│    - classify_codepoints(text: &str) -> Vec<u8>  │
│    - char_width(cp: u32) -> u8                   │
│    - string_width(text: &str) -> u32             │
│    - is_emoji_presentation(cp: u32) -> bool      │
│    - is_extended_pictographic(cp: u32) -> bool   │
│    - is_regional_indicator(cp: u32) -> bool      │
│    - is_skin_tone_modifier(cp: u32) -> bool      │
│    - is_variation_selector(cp: u32) -> bool      │
│    - is_combining_char(cp: u32) -> bool          │
│                                                  │
│  pkg/ (wasm-pack output, gitignored)             │
│    - emterm_wasm.js        (JS bindings)         │
│    - emterm_wasm_bg.wasm   (WASM binary)         │
│    - emterm_wasm.d.ts      (TypeScript types)    │
└──────────────────────┬──────────────────────────┘
                       │ import
                       ↓
┌─────────────────────────────────────────────────┐
│ src/terminal/wasm/ (TypeScript Glue)            │
│                                                  │
│  loader.ts                                       │
│    - initWasm(): Promise<void>                   │
│    - Loads and instantiates WASM module          │
│                                                  │
│  unicode.ts                                      │
│    - Re-exports with TS-friendly signatures      │
│    - classifyCodepoint(cp) -> number (packed)    │
│    - classifyCodepoints(text) -> Uint8Array      │
│    - charWidth(char) -> number                   │
│    - stringWidth(str) -> number                  │
│    - Bit flag constants (WIDTH_MASK, etc.)        │
│    - isEmojiPresentation(cp) -> boolean          │
│    - isExtendedPictographic(cp) -> boolean       │
│    - isRegionalIndicator(cp) -> boolean          │
│    - isSkinToneModifier(cp) -> boolean           │
│    - isVariationSelector(cp) -> boolean          │
│    - isCombiningChar(cp) -> boolean              │
└──────────────────────┬──────────────────────────┘
                       │ import
                       ↓
┌─────────────────────────────────────────────────┐
│ Consumers (existing code, import path change)   │
│                                                  │
│  handlers/print_handler.ts                       │
│    → Uses classifyCodepoint() per-char API       │
│  grid.ts                                         │
│    → Uses charWidth() individual API             │
│  state.ts                                        │
│    → Uses isEmojiPresentation() individual API   │
│  index.ts                                        │
│    → Re-exports from wasm/unicode.ts             │
└─────────────────────────────────────────────────┘
```

### Data Flow

**Batch Classification (print_handler hot path):**
```
PTY Data (string)
  → classifyCodepoints(text)           [JS → WASM, 1 call]
  → Rust iterates codepoints           [WASM linear memory]
  → Pack properties into Vec<u8>       [WASM linear memory]
  → Return Uint8Array                  [WASM → JS, 1 return]
  → JS iterates with pre-computed data [JS only, no more WASM calls]
```

**Individual Width (grid.ts):**
```
Character (string)
  → codePointAt(0)                     [JS]
  → char_width(cp)                     [JS → WASM → JS, 1 call]
```

### Batch API Byte Layout

Each codepoint is classified into a single byte:

```
Bit 7    Bit 6    Bit 5    Bit 4    Bit 3    Bit 2    Bit 1    Bit 0
┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬────────┐
│VAR_SEL │SKIN_T  │REG_IND │EXT_PIC │EMO_PRE │COMBIN  │ width  │ width  │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴────────┘
```

| Bits | Name | Description |
|------|------|-------------|
| 0-1 | width | Display width: 0, 1, or 2 (value 3 is reserved, must not be produced) |
| 2 | COMBINING | `is_combining_char` |
| 3 | EMOJI_PRES | `is_emoji_presentation` |
| 4 | EXT_PICTOGRAPHIC | `is_extended_pictographic` |
| 5 | REGIONAL_IND | `is_regional_indicator` |
| 6 | SKIN_TONE | `is_skin_tone_modifier` |
| 7 | VARIATION_SEL | `is_variation_selector` |

### File Structure

```
wasm/
├── Cargo.toml
├── src/
│   ├── lib.rs              # #[wasm_bindgen] exports
│   └── unicode.rs          # Unicode property functions (pure Rust, #[cfg(test)] inline tests)
└── pkg/                    # wasm-pack output (gitignored)
    ├── emterm_wasm.js
    ├── emterm_wasm_bg.wasm
    └── emterm_wasm.d.ts

src/terminal/
├── wasm/
│   ├── loader.ts           # WASM initialization
│   └── unicode.ts          # TS interface + re-exports
├── unicode.ts              # Original TS (kept for test reference)
└── handlers/
    └── print_handler.ts    # Updated to use WASM-backed functions
```

### Dependencies

**WASM Crate (wasm/Cargo.toml):**
- `wasm-bindgen = "0.2"` - Rust-JS bindings

**No additional npm dependencies required.** wasm-pack generates the JS bindings.

### Build Pipeline

**package.json scripts:**
```json
{
  "build:wasm": "cd wasm && wasm-pack build --target web --out-dir pkg",
  "dev": "bun run build:wasm && bun run --hot serve.ts",
  "build": "bun run build:wasm && bun build src/index.html --outdir dist --minify"
}
```

**.gitignore additions:**
```
wasm/pkg/
wasm/target/
```

### WASM Module Initialization

```typescript
// src/terminal/wasm/loader.ts
import init from "../../../wasm/pkg/emterm_wasm.js";

let initialized = false;

export async function initWasm(): Promise<void> {
  if (initialized) return;
  await init();
  initialized = true;
}
```

Called from `src/main.ts` at application startup, before terminal initialization.

### TypeScript Glue Interface

```typescript
// src/terminal/wasm/unicode.ts
import {
  classify_codepoints,
  char_width as wasm_char_width,
  string_width as wasm_string_width,
} from "../../../wasm/pkg/emterm_wasm.js";

// Bit flag constants
export const WIDTH_MASK       = 0b00000011;
export const COMBINING        = 0b00000100;
export const EMOJI_PRES       = 0b00001000;
export const EXT_PICTOGRAPHIC = 0b00010000;
export const REGIONAL_IND     = 0b00100000;
export const SKIN_TONE        = 0b01000000;
export const VARIATION_SEL    = 0b10000000;

export function classifyCodepoints(text: string): Uint8Array {
  return classify_codepoints(text);
}

export function charWidth(char: string): number {
  if (char.length === 0) return 0;
  const cp = char.codePointAt(0);
  if (cp === undefined) return 0;
  return wasm_char_width(cp);
}

export function stringWidth(str: string): number {
  return wasm_string_width(str);
}

export function isWideChar(char: string): boolean {
  return charWidth(char) === 2;
}

// Individual property checks (direct WASM exports)
export function isEmojiPresentation(cp: number): boolean {
  return wasm_is_emoji_presentation(cp);
}

export function isExtendedPictographic(cp: number): boolean {
  return wasm_is_extended_pictographic(cp);
}

export function isRegionalIndicator(cp: number): boolean {
  return wasm_is_regional_indicator(cp);
}

export function isSkinToneModifier(cp: number): boolean {
  return wasm_is_skin_tone_modifier(cp);
}

export function isVariationSelector(cp: number): boolean {
  return wasm_is_variation_selector(cp);
}

export function isCombiningChar(cp: number): boolean {
  return wasm_is_combining_char(cp);
}

// Single-codepoint packed classification (for hot path optimization)
export function classifyCodepoint(cp: number): number {
  return wasm_classify_codepoint(cp);
}
```

### Print Handler Integration

The ANSI parser dispatches characters one at a time via `handlePrintDispatch(state, char)`.
The batch API (`classifyCodepoints`) cannot be used here. Instead, `print_handler` uses
the single-codepoint classification API to replace multiple individual property calls with
one WASM call + bit decoding:

```typescript
// src/terminal/handlers/print_handler.ts (conceptual change)

// Before: multiple per-character WASM calls
// const width = charWidth(char);
// if (isVariationSelector(cp)) { ... }
// if (isSkinToneModifier(cp)) { ... }
// if (isExtendedPictographic(cp)) { ... }

// After: single classify_codepoint call + bit decoding
const p = classifyCodepoint(cp);
const width = p & WIDTH_MASK;
if (p & VARIATION_SEL) { /* handle variation selector */ }
if (p & SKIN_TONE) { /* handle skin tone modifier */ }
if (p & REGIONAL_IND) { /* handle regional indicator */ }
if (p & COMBINING) { /* handle combining character */ }
if (p & EXT_PICTOGRAPHIC) { /* handle extended pictographic */ }
```

The batch API (`classifyCodepoints`) is used only for `stringWidth` and future bulk operations
where a complete string is available.

## Test Scenarios

### Unit Tests (Rust)

- [ ] `char_width`: ASCII (0x20-0x7E) returns 1
- [ ] `char_width`: C0 control (0x00-0x1F) returns 0
- [ ] `char_width`: CJK Unified Ideographs returns 2
- [ ] `char_width`: Hiragana/Katakana returns 2
- [ ] `char_width`: Fullwidth forms returns 2
- [ ] `char_width`: Hangul Syllables returns 2
- [ ] `char_width`: Halfwidth forms returns 1
- [ ] `char_width`: Emoji_Presentation=Yes returns 2
- [ ] `char_width`: Non-Emoji_Presentation BMP symbols returns 1
- [ ] `char_width`: Combining characters returns 0
- [ ] `char_width`: Zero-width characters (ZWJ, VS, ZWNBSP) returns 0
- [ ] `is_emoji_presentation`: Matches TypeScript implementation for all documented codepoints
- [ ] `is_extended_pictographic`: Matches TypeScript implementation
- [ ] `classify_codepoints`: Empty string returns empty array
- [ ] `classify_codepoints`: Mixed ASCII/CJK/Emoji string returns correct packed bytes
- [ ] `classify_codepoints`: Surrogate pair characters are handled correctly
- [ ] `string_width`: Sum of char widths for various strings

### Integration Tests (TypeScript, via bun test)

- [ ] WASM module loads successfully
- [ ] `charWidth()` via WASM matches all existing `unicode.test.ts` cases
- [ ] `classifyCodepoints()` returns correct Uint8Array for test strings
- [ ] `print_handler` produces identical terminal state with WASM vs original TS
- [ ] `stringWidth()` via WASM matches existing behavior

### Performance Tests

- [ ] Benchmark: `charWidth` on 10,000 mixed characters (WASM vs TS)
- [ ] Benchmark: `classifyCodepoints` on large PTY output chunk
- [ ] Benchmark: `stringWidth` on long strings

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| WASM load failure | .wasm file missing or corrupt | Application fails to start with error message |
| Invalid codepoint | `char_width` called with value > 0x10FFFF | Returns 0 (safe default) |

## Performance Optimization

### Optimization Strategies

- **Batch API:** Single JS-WASM boundary crossing per PTY data chunk
- **Packed byte format:** Minimal memory allocation (1 byte per codepoint)
- **LLVM optimization:** Rust compiler optimizes branch conditions into lookup tables
- **ASCII fast path:** Early return for 0x20-0x7E (most common case)

## Success Criteria

- [ ] All existing `unicode.test.ts` tests pass with WASM implementation
- [ ] All Rust unit tests pass
- [ ] `bun run build:wasm` succeeds
- [ ] `bun tauri dev` loads WASM module and terminal works correctly
- [ ] `print_handler.ts` uses WASM-backed Unicode functions (single-codepoint classification API)
- [ ] Performance benchmark shows >= 1.5x improvement

## Implementation Phases

### Phase 1: WASM Crate Setup
**Goals:** Establish build pipeline and basic exports
**Deliverables:**
- `wasm/` directory with Cargo.toml
- wasm-pack build working
- `build:wasm` script in package.json
- Basic "hello world" WASM export verifiable in browser console

### Phase 2: Unicode Functions Implementation
**Goals:** Port all Unicode property functions to Rust
**Deliverables:**
- `wasm/src/unicode.rs` with all functions
- Rust unit tests covering all Unicode ranges
- `classify_codepoints` batch API
- Individual function exports

### Phase 3: TypeScript Integration
**Goals:** Connect WASM to existing frontend code
**Deliverables:**
- `src/terminal/wasm/loader.ts` and `unicode.ts`
- Import path changes in consumers
- `print_handler.ts` batch API integration
- All existing TypeScript tests passing

### Phase 4: Verification and Benchmarking
**Goals:** Ensure correctness and measure performance
**Deliverables:**
- Cross-validation between TS and WASM implementations
- Performance benchmarks
- CI/CD integration (Docker wasm-pack)

## References

- WASM investigation report: `tmp/wasm.md`
- Current TypeScript implementation: `src/terminal/unicode.ts`
- Current tests: `src/terminal/unicode.test.ts`
- wasm-pack documentation: https://rustwasm.github.io/wasm-pack/
- wasm-bindgen documentation: https://rustwasm.github.io/wasm-bindgen/
