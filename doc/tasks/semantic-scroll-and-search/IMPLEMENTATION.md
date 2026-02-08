# Implementation Plan: Semantic Scroll / Prompt Jump & In-Terminal Text Search

## Overview

Implement OSC 133 (Semantic Prompts) support and in-terminal text search for eMterm. The Rust ANSI parser will recognize OSC 133 sequences, the TypeScript frontend will track semantic zones and provide prompt-to-prompt navigation, and a full-featured search bar will enable incremental text search across the entire scrollback buffer with canvas-based highlighting.

## Objectives

- Parse OSC 133 A/B/C/D markers in the Rust ANSI parser and propagate to TypeScript
- Record semantic zone markers and enable prompt-to-prompt keyboard navigation
- Implement incremental text search with regex support, case-sensitivity toggle, and canvas highlights
- Provide a floating search bar UI with match navigation

## Prerequisites

### Development Environment
- Rust toolchain (for Tauri backend)
- Bun (package manager and test runner)
- Docker (for testing)

### Dependencies
- No new external dependencies required
- All features use standard Web APIs and existing project infrastructure

### Knowledge Requirements
- eMterm OSC dispatch pipeline (Rust parser → JSON → TypeScript handler)
- Canvas renderer architecture (dirty tracking, scrollback offset)
- Settings pattern (Rust `define_keybinds!` macro + TypeScript `KeybindSettings`)
- Keybind matcher system

## Architecture Overview

### Technology Stack
- **Backend**: Rust (ANSI parser, settings)
- **Frontend**: Vanilla TypeScript (terminal state, rendering, UI)
- **Build**: Bun

### Design Approach

Two independent feature tracks sharing a common OSC 133 foundation:
1. **OSC 133 → Prompt Jump**: Parser → Zone Tracker → Keyboard Handler → Scroll
2. **Search**: Search State → Canvas Highlight → Search Bar UI

### Component Interaction

```
Rust Parser (parser.rs)
    │ emits OscAction::SemanticPrompt
    ▼
TypeScript OSC Handler (osc_handlers.ts)
    │ dispatches to
    ▼
SemanticZoneTracker (semantic-zone.ts)    SearchStateManager (search-state.ts)
    │ stores markers                          │ manages matches
    ▼                                         ▼
TerminalState (state.ts)                  CanvasRenderer (canvas-renderer.ts)
    │ provides scroll/buffer access           │ draws highlights
    ▼                                         ▼
KeyboardHandler (keyboard.ts)             SearchBar (search-bar.ts)
    │ handles jump keybinds                   │ DOM component
    ▼                                         │
CanvasRenderer.scrollUp/Down              TerminalApp (index.ts)
                                              │ wires everything together
```

## Implementation Phases

### Phase 1: OSC 133 Foundation

**Goal**: Rust parser recognizes OSC 133 sequences and TypeScript records zone markers. Verified by unit tests on both sides.

**Files to Create**:
- `src/terminal/semantic-zone.ts` - SemanticZoneTracker class
- `src/terminal/semantic-zone.test.ts` - Unit tests

**Files to Modify**:
- `src-tauri/src/ansi/sequence.rs` - Add `SemanticPrompt` variant to `OscAction`
- `src-tauri/src/ansi/parser.rs` - Add OSC 133 recognition in `dispatch_osc()`
- `src/types/terminal.ts` - Add `SemanticPrompt` to `OscAction` type union
- `src/terminal/handlers/osc_handlers.ts` - Add `SemanticPrompt` case
- `src/terminal/handlers/types.ts` - Add zone tracker accessor to `TerminalStateAccessor`
- `src/terminal/state.ts` - Instantiate SemanticZoneTracker, integrate with scrollback pruning

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `OscAction::SemanticPrompt` | Represent parsed OSC 133 data | Raw OSC buffer with "133;X" prefix | Structured variant with zone_type and optional exit_code |
| `SemanticZoneTracker` | Store and query semantic markers | Markers added in chronological order | Markers retrievable by position, prunable by line index |

**Processing Flow**:
```
1. Rust parser receives OSC string starting with "133;"
   ├─ Subcommand is A, B, C → SemanticPrompt { zone_type, exit_code: None }
   ├─ Subcommand is D → Parse optional exit code, default 0
   └─ Unknown subcommand → Ignore (log warning)
2. TypeScript OSC handler receives SemanticPrompt action
3. Compute absolute line index (scrollback length + cursor row)
4. Add marker to SemanticZoneTracker
```

**Implementation Steps**:

1. **Add Rust OscAction variant**
   - Add `SemanticPrompt { zone_type: String, exit_code: Option<i32> }` to `OscAction` enum
   - Implement parsing in `dispatch_osc()` for `osc_param == 133`

2. **Add TypeScript type and handler**
   - Extend `OscAction` type in `terminal.ts`
   - Add dispatch case in `osc_handlers.ts`

3. **Create SemanticZoneTracker**
   - Marker storage with absolute line indices
   - Binary search for `findPrevPrompt` / `findNextPrompt`
   - `pruneBeforeLine()` for scrollback trimming synchronization

4. **Integrate with TerminalState**
   - Instantiate tracker in constructor
   - Hook into scrollback pruning to call `pruneBeforeLine()`
   - Expose via `TerminalStateAccessor`

5. **Handle alternate buffer**
   - When alternate buffer is active, ignore OSC 133 markers (do not record)
   - Check `isAlternateBuffer()` in OSC handler before adding markers

**Dependencies**:
- Requires: None (foundational phase)
- Blocks: Phase 2 (Prompt Jump)

**Testing Approach**:

*Unit Tests (Rust)*:
- Parse OSC 133;A, 133;B, 133;C, 133;D with and without exit code
- Unknown subcommands are ignored
- Serialization to JSON is correct

*Unit Tests (TypeScript)*:
- SemanticZoneTracker add/retrieve markers
- findPrevPrompt / findNextPrompt boundary conditions
- pruneBeforeLine correctly removes and adjusts indices
- Empty tracker returns null for find operations

**Acceptance Criteria**:
- [ ] OSC 133;A/B/C/D correctly parsed in Rust and emitted as SemanticPrompt
- [ ] TypeScript records markers with correct absolute line indices
- [ ] Scrollback pruning correctly removes stale markers
- [ ] All unit tests pass

**Estimated Effort**: 小 (1-2 days)

---

### Phase 2: Prompt Jump

**Goal**: Users can jump between prompts using configurable keyboard shortcuts. Verified by scrolling to the correct positions.

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - Add `jump_to_prev_prompt` and `jump_to_next_prompt` to `define_keybinds!`
- `src/settings/types.ts` - Add fields to `KeybindSettings`
- `src/i18n/locales/en.json` - Add keybind labels
- `src/i18n/locales/ja.json` - Add keybind labels
- `src/terminal-app/handlers/keyboard.ts` - Add prompt jump handling
- `src/terminal/canvas-renderer.ts` - Add `setScrollOffset()` method for programmatic scroll positioning

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| KeybindSettings additions | Store configurable prompt jump keybinds | Settings loaded | Keybinds accessible via SettingsService |
| Prompt jump handler | Find target prompt and scroll to it | Keybind triggered, zone tracker available | Scroll position updated to target prompt line |

**Processing Flow**:
```
1. User presses jump keybind (Ctrl+Shift+Up or Down)
2. KeyboardHandler matches keybind
3. Get current scroll position (absolute line)
   ├─ scrollback length - scrollOffset = current top line
4. Query SemanticZoneTracker for prev/next prompt
   ├─ Found → Calculate new scrollOffset for target line
   ├─ Prev not found (already at top) → Scroll to top of scrollback
   └─ Next not found (at bottom) → Scroll to bottom (offset = 0)
5. Update renderer scroll position
6. Trigger re-render
```

**Implementation Steps**:

1. **Add keybind settings**
   - Add entries to `define_keybinds!` macro with defaults "Ctrl+Shift+ArrowUp" / "Ctrl+Shift+ArrowDown"
   - Mirror in TypeScript `KeybindSettings` and i18n locales
   - Add settings UI entries

2. **Add scroll position setter**
   - Canvas renderer needs a method to set scroll offset directly (not just relative scroll)
   - Convert absolute line index to scroll offset

3. **Implement keyboard handler**
   - Match prompt jump keybinds in `handleKeyDown()`
   - Query zone tracker, compute target scroll offset, update renderer
   - Key considerations: Must check `isActiveTab()` and not interfere with IME

**Dependencies**:
- Requires: Phase 1 (SemanticZoneTracker)
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- Keybind settings defaults are correct
- Scroll offset calculation from absolute line index

*Manual Testing*:
- Prompt jump navigates correctly with bash/zsh/fish OSC 133 output
- Keybinds work without interfering with terminal input

**Acceptance Criteria**:
- [ ] Ctrl+Shift+Up jumps to previous prompt
- [ ] Ctrl+Shift+Down jumps to next prompt
- [ ] Boundary behavior correct (no prompt above → scroll to top, none below → scroll to bottom)
- [ ] Keybinds configurable in settings
- [ ] No interference with IME or terminal input

**Estimated Effort**: 小 (1-2 days)

---

### Phase 3: Search Engine

**Goal**: Core search logic that finds matches across scrollback and screen buffers, supporting plain text, regex, and case-sensitivity options. Verified by unit tests.

**Files to Create**:
- `src/terminal/search/search-state.ts` - SearchStateManager class
- `src/terminal/search/search-state.test.ts` - Unit tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SearchStateManager | Execute search queries and manage match list | Query and options set | Matches array populated, currentMatchIndex valid |
| SearchMatch | Represent a match location | Search executed | lineIndex, startCol, endCol set |

**Processing Flow**:
```
1. Query text updated
   ├─ Empty → Clear matches, return
   ├─ Regex mode → Compile regex pattern
   │   ├─ Invalid → Set error state, return
   │   └─ Valid → Use regex for matching
   └─ Plain text → Escape special chars if needed
2. Iterate all lines (scrollback + screen)
   ├─ Extract plain text from Line cells
   ├─ Find all matches in line text
   └─ Record match positions (lineIndex, startCol, endCol)
3. Set currentMatchIndex to nearest match to current view
4. Expose matches for rendering
```

**Implementation Steps**:

1. **Create SearchStateManager**
   - Query/options management
   - Line text extraction (iterate cells, concatenate characters)
   - Plain text search (case-insensitive by default)
   - Regex search with error handling
   - Match list with wrap-around navigation

2. **Implement ReDoS protection**
   - Use `performance.now()` to measure elapsed time during search
   - Check elapsed time every N lines (e.g., every 100 lines)
   - If search exceeds 200ms, abort and set timeout error state
   - Error state propagation to UI (show timeout message in search bar)

3. **Implement match navigation**
   - nextMatch / prevMatch with wrap-around
   - getVisibleMatches for efficient rendering

4. **Performance optimizations**
   - Text extraction cache: cache plain text representation per line, invalidate on line content change
   - Incremental search: deferred to implementation phase (full re-search is acceptable for MVP given < 50ms target on 10k lines)
   - Debounce: not applied initially; add 50ms debounce only if search latency exceeds target during testing

5. **Buffer change handling**
   - When new output arrives while search is active: search results are retained until next user input event (typing in search bar)
   - On scrollback pruning: call `SearchStateManager.invalidate()` to clear stale matches and trigger re-search on next input
   - SearchMatch absolute line indices are not adjusted on pruning (full re-search is simpler and more reliable)

**Dependencies**:
- Requires: None (independent of Phase 1/2)
- Blocks: Phase 4 (Search UI)

**Testing Approach**:

*Unit Tests*:
- Plain text search finds matches in multiple lines
- Case-insensitive search (default behavior)
- Case-sensitive search when toggled
- Regex search with valid patterns
- Invalid regex returns error state, no crash
- nextMatch/prevMatch wrap around correctly
- getVisibleMatches returns correct subset
- Empty query produces no matches
- Wide characters (CJK) handled correctly
- Multiple matches on single line

**Acceptance Criteria**:
- [ ] Plain text search finds all occurrences
- [ ] Regex search works with valid patterns
- [ ] Invalid regex patterns produce error state without crash
- [ ] Case sensitivity toggle works correctly
- [ ] Match navigation wraps around
- [ ] Performance: search < 50ms on 10,000 lines

**Estimated Effort**: 中 (2-3 days)

**Risks and Mitigation**:
- **Risk**: ReDoS on pathological patterns
  - **Mitigation**: Implement timeout or abort mechanism

---

### Phase 4: Search UI & Canvas Highlight

**Goal**: Complete search experience with floating search bar, canvas highlights, and keyboard navigation. Verified by manual testing.

**Files to Create**:
- `src/terminal/search/search-bar.ts` - Search bar DOM component
- `src/terminal/search/search-bar.css` - Search bar styles (bundled via Bun)

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` - Add search highlight rendering
- `src/terminal-app/index.ts` - Wire search bar to TerminalApp
- `src/terminal-app/handlers/keyboard.ts` - Add search keybind handling (open/close)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SearchBar | DOM component for search input and controls | Container element available | Renders floating bar, emits search events |
| Search highlight renderer | Draw match rectangles on canvas | Matches and cell dimensions known | Highlighted rectangles drawn over matching cells |
| TerminalApp integration | Wire search lifecycle | All search components created | Open/close/navigate working end-to-end |

**Processing Flow**:
```
1. User presses Ctrl+Shift+F
   ├─ Search bar not open → Create and show, focus input
   └─ Already open → Focus input, select all text
2. User types in search input
   → Trigger SearchStateManager.executeSearch()
   → Canvas renderer re-renders with highlights
   → Update hit count display
3. User navigates matches (Enter / Shift+Enter)
   → Move currentMatchIndex
   → If match is off-screen, update scroll offset
   → Re-render highlights
4. User presses Esc or close button
   → Hide search bar
   → Clear highlights
   → Return focus to terminal
```

**Implementation Steps**:

1. **Create search bar DOM component**
   - Input field, toggle buttons (regex, case), hit count, navigation buttons, close button
   - Right-aligned at terminal top
   - Styling: semi-transparent dark background, border, rounded corners
   - Key considerations: Must not interfere with terminal key handling when unfocused

2. **Implement canvas highlight rendering**
   - After main render pass, overlay semi-transparent rectangles on matched cells
   - All matches: yellow background (rgba-based)
   - Current match: orange background (more prominent)
   - Only render highlights for visible lines (performance)
   - Drawing order: text → URL highlight → search highlight → selection highlight (topmost)

3. **Handle tab switching**
   - Search state (SearchStateManager) is per-tab (owned by each TerminalState)
   - Search bar visibility is per-tab: when switching tabs, hide/show search bar based on target tab's search state
   - Search bar UI instance is shared; its state is swapped when tabs change

4. **Wire into TerminalApp**
   - Add search keybind handler to KeyboardHandler
   - Create SearchBar instance in TerminalApp
   - Connect search events: input → execute search → re-render
   - Connect navigation events: enter/shift+enter → next/prev match → scroll if needed
   - Handle close: hide bar, clear state, restore focus

4. **Handle scroll-to-match**
   - When navigating to a match that is off-screen, adjust scroll offset
   - Convert match lineIndex to scroll offset relative to scrollback length

**Dependencies**:
- Requires: Phase 3 (SearchStateManager)
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- Search bar renders correctly with all controls
- Search bar emits correct events on input/toggle/navigate/close

*Manual Testing*:
- [ ] Ctrl+Shift+F opens search bar
- [ ] Incremental search highlights matches on canvas
- [ ] Current match distinct from other matches
- [ ] Enter/Shift+Enter navigate matches with wrap-around
- [ ] Hit count display updates correctly
- [ ] Regex and case toggle buttons work
- [ ] Invalid regex shows error indicator
- [ ] Esc closes bar and clears highlights
- [ ] Focus behavior: search bar captures input, terminal unaffected when bar unfocused
- [ ] Scroll-to-match works for off-screen matches

**Acceptance Criteria**:
- [ ] Search bar opens with Ctrl+Shift+F
- [ ] Matches highlighted on canvas with two distinct colors
- [ ] Hit count displayed as "N/M"
- [ ] Match navigation works with Enter/Shift+Enter
- [ ] Regex and case sensitivity toggles functional
- [ ] Esc closes and clears everything
- [ ] No regressions in terminal input, rendering, or scrolling

**Estimated Effort**: 中 (3-4 days)

---

## Complete File Structure

```
src-tauri/src/
├── ansi/
│   ├── parser.rs              # Modified: OSC 133 recognition in dispatch_osc()
│   └── sequence.rs            # Modified: SemanticPrompt variant in OscAction
├── commands/
│   └── config.rs              # Modified: jump_to_prev_prompt, jump_to_next_prompt in define_keybinds!

src/
├── terminal/
│   ├── semantic-zone.ts       # New: SemanticZoneTracker class
│   ├── semantic-zone.test.ts  # New: Unit tests
│   ├── state.ts               # Modified: Integrate zone tracker + scrollback sync
│   ├── canvas-renderer.ts     # Modified: Search highlight rendering, setScrollOffset()
│   ├── handlers/
│   │   ├── osc_handlers.ts    # Modified: SemanticPrompt case
│   │   └── types.ts           # Modified: TerminalStateAccessor additions
│   └── search/
│       ├── search-state.ts    # New: SearchStateManager class
│       ├── search-state.test.ts # New: Unit tests
│       ├── search-bar.ts      # New: Search bar DOM component
│       └── search-bar.css     # New: Search bar styles
├── terminal-app/
│   ├── index.ts               # Modified: Wire search bar
│   └── handlers/
│       └── keyboard.ts        # Modified: Prompt jump + search keybinds
├── settings/
│   └── types.ts               # Modified: KeybindSettings additions
├── types/
│   └── terminal.ts            # Modified: SemanticPrompt in OscAction type
└── i18n/
    └── locales/
        ├── en.json            # Modified: Keybind labels
        └── ja.json            # Modified: Keybind labels
```

## Testing Strategy

### Unit Testing

**Approach**:
- Rust: `cargo test` for parser changes
- TypeScript: `bun test` for zone tracker and search state

**Test Coverage Goals**:
- Parser (Rust): 100% for OSC 133 parsing paths
- SemanticZoneTracker: 90%+
- SearchStateManager: 90%+
- Search bar UI: 60% (DOM testing)

**Key Test Areas**:

| Area | Tests | Type |
|------|-------|------|
| OSC 133 parsing | 7 scenarios (A/B/C/D, exit code, unknown, JSON) | Rust unit |
| SemanticZoneTracker | 6 scenarios (add, find, prune, boundary, empty) | TS unit |
| SearchStateManager | 10+ scenarios (text, regex, case, navigation, edge) | TS unit |
| Search bar | Event emission, rendering | TS unit |

### Docker Testing

```bash
# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript typecheck
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Manual Testing

- Prompt jump with real shell output (bash/zsh with OSC 133 enabled)
- Search UI interaction and visual appearance
- Performance with large scrollback (10,000+ lines)

## Dependencies

### External Dependencies

None - all features use standard Web APIs.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: OSC 133 Foundation (no dependencies)
2. Phase 2: Prompt Jump (depends on Phase 1)
3. Phase 3: Search Engine (no dependencies, parallel with Phase 1/2)
4. Phase 4: Search UI (depends on Phase 3)

**Component Dependencies**:
- SemanticZoneTracker → TerminalState (scrollback line count)
- Prompt jump handler → SemanticZoneTracker, CanvasRenderer (scroll offset)
- SearchStateManager → TerminalState (scrollback + screen buffer access)
- Search bar → SearchStateManager, CanvasRenderer (highlights)

## Risk Assessment

### Technical Risks

1. **ReDoS with Complex Regex Patterns**
   - **Likelihood**: Medium
   - **Impact**: High (UI freeze)
   - **Mitigation**: Timeout mechanism or pattern complexity check

2. **Search Performance on Large Scrollback**
   - **Likelihood**: Low (10,000 lines is manageable)
   - **Impact**: Medium (perceived lag on search)
   - **Mitigation**: Text extraction cache, visible-only highlight rendering

3. **Canvas Highlight Rendering Performance**
   - **Likelihood**: Low
   - **Impact**: Medium (dropped frames)
   - **Mitigation**: Only compute highlights for visible lines, integrate with dirty tracking

## Performance Considerations

1. **Prompt Jump**: O(log n) binary search on sorted marker array
2. **Search Execution**: O(n) line scan, target < 50ms for 10,000 lines
3. **Highlight Rendering**: Only visible matches rendered, integrated with existing render loop
4. **Text Extraction**: Cache plain text per line to avoid repeated cell iteration

## Security Considerations

1. **ReDoS**: Regex execution timeout protects against malicious/complex patterns
2. **No DOM Injection**: All text rendering via canvas, search input used only for matching
3. **Local Data Only**: All search operates on local terminal buffer content

## Open Questions

None - all requirements confirmed in specification phase.

## Success Metrics

### Functional Completeness
- [ ] OSC 133 A/B/C/D parsed and recorded
- [ ] Prompt jump navigates correctly
- [ ] Search finds matches with plain text and regex
- [ ] All UI controls functional

### Quality Metrics
- [ ] All unit tests pass
- [ ] TypeScript typecheck passes
- [ ] No regressions in existing functionality

### Performance Metrics
- [ ] Prompt jump < 10ms
- [ ] Search < 50ms on 10,000 lines
- [ ] Highlight rendering within frame budget (16ms)

## References

- **Specification**: `doc/tasks/semantic-scroll-and-search/SPEC.md`
- **Requirements**: `doc/tasks/semantic-scroll-and-search/要件定義書.md`
- OSC 133 Semantic Prompts (FinalTerm / iTerm2 / WezTerm specification)
- Existing parser: `src-tauri/src/ansi/parser.rs` (dispatch_osc at line 624)
- Existing keybind macro: `src-tauri/src/commands/config.rs` (define_keybinds! at line 206)
- Existing scroll management: `src/terminal/canvas-renderer.ts` (scrollOffset at line 349)
