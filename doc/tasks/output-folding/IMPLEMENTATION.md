# Implementation Plan: Command Output Folding

## Overview

Implement command output folding that allows users to collapse OSC 133 output zones (C→D) and custom OSC 777;emterm;fold regions into single summary lines on the Canvas renderer, toggled by mouse click.

## Objectives

- Identify foldable regions from OSC 133 C→D marker pairs and custom OSC 777;emterm;fold sequences
- Render fold summary lines on Canvas with command name, line count, and exit code
- Provide click-to-toggle fold/unfold with proper scroll adjustment
- Integrate with existing search and prompt jump features (auto-expand on navigation)
- Add a global fold_enabled setting (default: ON)

## Prerequisites

### Development Environment

- Rust toolchain (for settings struct modification)
- Bun (TypeScript build and test runner)
- Docker (for CI testing)

### Dependencies

- Existing SemanticZoneTracker (OSC 133 markers)
- Existing Canvas renderer (line rendering pipeline)
- Existing OSC handler (EmtermExtension routing)
- Existing settings infrastructure (Rust serde + TypeScript types)

### Knowledge Requirements

- Canvas 2D rendering pipeline (two-pass: background then text)
- `getVisibleLines()` / `scrollOffset` mechanism for scrollback rendering
- OSC dispatch flow: Rust parser → JSON → TypeScript handler
- Settings pattern: Rust `serde(default)` + TypeScript `AppSettings` interface

## Architecture Overview

### Technology Stack

- **Language**: Rust (backend settings) + TypeScript (frontend logic and rendering)
- **Rendering**: Canvas 2D API
- **State Management**: In-memory per terminal session

### Design Approach

The fold feature introduces a logical layer between the raw scrollback buffer and the Canvas renderer. A FoldManager maintains fold regions and their collapsed/expanded state. The renderer queries the FoldManager to determine which lines to skip (collapsed regions) and where to insert summary lines.

### Component Interaction

```
SemanticZoneTracker ──→ FoldManager ←── OSC Handler (fold verb)
                            │
                            ▼
                     Canvas Renderer
                     (fold-aware getVisibleLines)
                            │
                            ▼
                     Mouse click handler
                     (toggle fold on click)
```

## Implementation Phases

### Phase 1: FoldManager Core

**Goal**: Create the FoldManager class with region registration, state management, and line mapping logic. All logic unit-testable without Canvas or DOM.

**Files to Create**:
- `src/terminal/fold-manager.ts` - FoldManager class
- `src/terminal/fold-manager.test.ts` - Unit tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FoldRegion (interface) | Data structure for a fold region | - | Contains startLine, endLine, collapsed state, source, label/command, exitCode, lineCount |
| FoldManager | Manage fold regions and line mapping | - | Regions can be registered, toggled, queried, and pruned |
| registerOsc133Region | Register a C→D region from semantic markers | Valid startLine < endLine | Region stored in map with source "osc133" |
| registerCustomRegion | Register a begin→end region from custom OSC | Valid startLine < endLine | Region stored in map with source "custom" |
| toggleFold | Toggle collapsed state of region at line | Region exists at lineIndex | Collapsed state flipped, returns success |
| getRegionAtLine | Find fold region containing given line | - | Returns region or null |
| getCollapsedRegions | List all collapsed regions sorted by startLine | - | Sorted array of collapsed FoldRegions |
| displayLineToActual | Map display row to actual buffer line | Collapsed regions exist | Returns actual line index accounting for folds |
| actualLineToDisplay | Map actual buffer line to display row | Collapsed regions exist | Returns display line index accounting for folds |
| pruneBeforeLine | Remove/adjust regions when scrollback is trimmed | lineIndex is trim boundary | Regions before boundary removed, indices adjusted |
| unfoldAll | Expand all collapsed regions | - | All regions set to collapsed=false |

**Processing Flow**:
```
1. Region Registration
   ├─ OSC 133 path → extract C/D pairs from SemanticZoneTracker → registerOsc133Region
   └─ Custom OSC path → fold begin/end markers → registerCustomRegion
2. Line Mapping (for rendering)
   ├─ Iterate collapsed regions sorted by startLine
   ├─ For each collapsed region: subtract (endLine - startLine) lines, add 1 summary line
   └─ Build cumulative offset table for efficient lookup
3. Toggle Fold
   ├─ Find region containing clicked line
   ├─ Flip collapsed state
   └─ Return delta (number of lines hidden/shown) for scroll adjustment
```

**Implementation Steps**:

1. **Define FoldRegion interface and FoldManager class**
   - Region identified by startLine-based ID
   - Map<string, FoldRegion> for O(1) lookup by ID
   - Key considerations:
     - Regions must not overlap
     - Line count = endLine - startLine (exclusive of start, as start becomes summary line)

2. **Implement line mapping**
   - Cached sorted array of collapsed regions for binary search
   - displayLineToActual: walk collapsed regions, accumulate offsets
   - actualLineToDisplay: reverse mapping
   - Key considerations:
     - Summary line occupies 1 display row (replaces the collapsed range)
     - Cache invalidated on fold toggle or region change

3. **Implement pruning**
   - Mirror SemanticZoneTracker.pruneBeforeLine pattern
   - Remove regions entirely before boundary, adjust remaining
   - Handle partial overlap: region spanning the boundary is removed

**Dependencies**:
- Requires: None (standalone data structure)
- Blocks: Phase 2 (OSC integration), Phase 3 (rendering), Phase 4 (mouse)

**Testing Approach**:

*Unit Tests*:

| ID | Scenario | Expected Result |
|----|----------|-----------------|
| T1-1 | Register OSC 133 region | Region stored with correct properties |
| T1-2 | Register custom region | Region stored with label, no exitCode |
| T1-3 | Toggle fold | collapsed flips, returns true |
| T1-4 | Toggle fold on non-existent line | Returns false |
| T1-5 | getRegionAtLine (inside region) | Returns region |
| T1-6 | getRegionAtLine (outside region) | Returns null |
| T1-7 | displayLineToActual with 0 folds | Identity mapping |
| T1-8 | displayLineToActual with 1 fold | Lines after fold shifted |
| T1-9 | displayLineToActual with multiple folds | Cumulative offsets correct |
| T1-10 | actualLineToDisplay | Reverse of displayLineToActual |
| T1-11 | Prune removes old regions | Regions before boundary gone |
| T1-12 | Prune adjusts remaining indices | Indices shifted correctly |
| T1-13 | Prune partial overlap | Region spanning boundary removed |
| T1-14 | unfoldAll | All regions expanded |
| T1-15 | Disabled state prevents toggle | toggleFold returns false when disabled |
| T1-16 | Region with 0 lines rejected | Not registered |
| T1-17 | Region with 1 line accepted | Registered |

**Acceptance Criteria**:
- [ ] FoldManager correctly registers and retrieves fold regions
- [ ] Line mapping produces correct display-to-actual conversions
- [ ] Pruning correctly handles scrollback trimming
- [ ] All unit tests pass

**Estimated Effort**: 小

---

### Phase 2: OSC Integration and Region Detection

**Goal**: Wire fold region detection into the OSC handler pipeline — both OSC 133 zone pairs and custom OSC 777;emterm;fold sequences.

**Files to Modify**:
- `src/terminal/handlers/osc_handlers.ts` - Add fold verb routing in handleEmtermExtension
- `src/terminal/handlers/types.ts` - Add getFoldManager() to TerminalStateAccessor
- `src/terminal/state.ts` - Instantiate FoldManager, expose via accessor, wire pruning

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleFoldCommand | Route fold begin/end to FoldManager | EmtermExtension with verb="fold" | Pending fold tracked or region registered |
| PendingFoldBegin tracking | Track incomplete fold begin markers | fold;begin received | Stored with lineIndex and label |
| OSC 133 region detection | Detect C→D pairs and register with FoldManager | C and D markers recorded | FoldRegion registered on D marker receipt |
| Command text extraction | Get command text from B marker's line | B marker exists before C marker | commandText populated in FoldRegion |

**Processing Flow**:
```
1. Custom OSC fold
   ├─ fold;begin;{label} → store pending begin (lineIndex, label)
   │   └─ If previous pending begin exists → discard it
   └─ fold;end → if pending begin exists → register custom region → clear pending
       └─ If no pending begin → ignore
2. OSC 133 zone detection
   ├─ On D marker receipt → look back in markers for matching C
   ├─ Extract command text from B marker's line (between B and C)
   └─ Register OSC 133 region with FoldManager
```

**Implementation Steps**:

1. **Add FoldManager to TerminalState**
   - Instantiate in constructor alongside SemanticZoneTracker
   - Add to TerminalStateAccessor interface
   - Hook into scrollback pruning (call foldManager.pruneBeforeLine)

2. **Handle custom OSC fold verb**
   - In handleEmtermExtension, route verb="fold" to new handler
   - Track pending begin state per terminal state
   - On end, compute line range and register with FoldManager

3. **Detect OSC 133 C→D pairs**
   - On receipt of D marker, search backwards in SemanticZoneTracker for matching C
   - Also find preceding B marker to extract command text
   - Extract text content of the B marker's line from scrollback buffer
   - Register region with FoldManager

**Dependencies**:
- Requires: Phase 1 (FoldManager)
- Blocks: Phase 3 (rendering needs regions)

**Testing Approach**:

*Unit Tests*:

| ID | Scenario | Expected Result |
|----|----------|-----------------|
| T2-1 | fold;begin;label creates pending | Pending fold stored |
| T2-2 | fold;end completes region | Region registered in FoldManager |
| T2-3 | fold;end without begin | Silently ignored |
| T2-4 | Consecutive begin discards previous | Only latest begin used |
| T2-5 | OSC 133 D marker creates region | C→D region registered |
| T2-6 | Command text extracted from B line | commandText populated |
| T2-7 | C without D: no region | Region not created |

**Acceptance Criteria**:
- [ ] Custom OSC fold;begin/end creates fold regions
- [ ] OSC 133 C→D pairs automatically create fold regions
- [ ] Command text correctly extracted from B marker line
- [ ] FoldManager integrated into TerminalState lifecycle

**Estimated Effort**: 小

---

### Phase 3: Canvas Renderer Integration

**Goal**: Make the Canvas renderer fold-aware — skip collapsed lines, render summary lines, and support fold-adjusted scroll positions.

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` - Fold-aware getVisibleLines, summary line rendering
- `src/terminal-app/index.ts` - Scroll offset adjustment on fold toggle

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| getVisibleLinesWithFolding | Modified getVisibleLines that accounts for folds | FoldManager has collapsed regions | Returns lines with collapsed ranges replaced by null markers |
| renderSummaryLine | Draw a fold summary line at a given row | Region data available | Summary line rendered with background, icon, text, line count |
| forceRender (modified) | Integrate fold-aware rendering into existing pipeline | FoldManager available | Collapsed regions skipped, summary lines rendered |

**Processing Flow**:
```
1. forceRender called
   ├─ Get collapsed regions from FoldManager
   ├─ For each display row:
   │   ├─ Map display row to actual line via FoldManager
   │   ├─ If actual line is start of collapsed region → render summary line
   │   └─ Otherwise → render normal line (existing pipeline)
   └─ Render search highlights (with fold offset adjustment)
2. Scroll adjustment on fold/unfold
   ├─ If fold is above viewport → adjust scrollOffset by delta
   └─ If fold is within or below viewport → no scroll adjustment needed
```

**Implementation Steps**:

1. **Modify getVisibleLines to be fold-aware**
   - Accept FoldManager as parameter
   - When iterating buffer lines, skip lines inside collapsed regions
   - Insert a sentinel/null entry where summary lines should appear
   - Key considerations:
     - Must maintain correct mapping between display row and actual line
     - Total visible rows unchanged (still fills the viewport)

2. **Implement summary line rendering**
   - Semi-transparent bar background across full line width
   - Left: ▶ icon (collapsed) or ▼ (expanded, when hovering foldable zone)
   - Text: command name or label, truncated at ~80 chars
   - Right portion: "— {N} lines" + "(exit {code})" if applicable
   - Color: dim for exit 0, red (#ff6b6b) for non-zero
   - Key considerations:
     - Use same font metrics as terminal text
     - Background: semi-transparent overlay (e.g., rgba(60, 60, 80, 0.3))

3. **Adjust scroll offset on fold/unfold**
   - When a region above the viewport is folded, reduce scrollOffset by (lineCount - 1)
   - When a region above the viewport is unfolded, increase scrollOffset by (lineCount - 1)
   - When fold is within viewport, no offset change needed

**Dependencies**:
- Requires: Phase 1 (FoldManager), Phase 2 (regions populated)
- Blocks: Phase 4 (mouse interaction)

**Testing Approach**:

*Integration Tests (manual visual verification)*:

| ID | Scenario | Expected Result |
|----|----------|-----------------|
| T3-1 | Collapsed region shows summary line | 1 line with ▶ icon, command, line count |
| T3-2 | Exit code 0 summary | Normal dim text |
| T3-3 | Non-zero exit code summary | Red-tinted text |
| T3-4 | Custom fold with label | Label shown instead of command |
| T3-5 | Scroll offset adjusts on fold above viewport | Viewport doesn't jump |
| T3-6 | Search highlights adjust for fold offsets | Highlights at correct positions |

**Acceptance Criteria**:
- [ ] Collapsed regions display as summary lines on Canvas
- [ ] Summary lines show correct command/label, line count, exit code
- [ ] Exit code coloring works (dim for 0, red for non-zero)
- [ ] Scroll position doesn't jump when folding/unfolding

**Estimated Effort**: 中

---

### Phase 4: Mouse Interaction and Feature Integration

**Goal**: Enable click-to-toggle fold/unfold, cursor feedback, and integrate with search and prompt jump.

**Files to Modify**:
- `src/terminal-app/index.ts` - Click handler for fold toggle, search/prompt jump integration
- `src/terminal/canvas-renderer.ts` - Cursor style change on hover

**Files to Create (if needed)**:
- None (logic added to existing files)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleFoldClick | Detect click on foldable area and toggle | Canvas coordinates available | Fold toggled, re-render triggered |
| hover feedback | Change cursor to pointer over fold areas | Mouse position tracked | Cursor style updated |
| Search integration | Auto-expand fold when search navigates into it | Search match in folded region | Region expanded, match visible |
| Prompt jump integration | Auto-expand fold when jumping to prompt inside it | Jump target in folded region | Region expanded, target visible |

**Processing Flow**:
```
1. Click handler
   ├─ Convert pixel coordinates to display row
   ├─ Map display row to actual line via FoldManager
   ├─ Check if actual line is in a fold region
   │   ├─ Summary line clicked → toggle fold (expand)
   │   └─ Output zone header clicked → toggle fold (collapse)
   └─ Adjust scroll offset and re-render
2. Search integration
   ├─ When scrollToCurrentMatch is called
   ├─ Check if match lineIndex is inside a collapsed region
   └─ If so → expand the region first, then scroll to match
3. Prompt jump integration
   ├─ When jump-to-prompt is executed
   ├─ Check if target line is inside a collapsed region
   └─ If so → expand the region first, then scroll to target
```

**Implementation Steps**:

1. **Add click handler for fold toggle**
   - In the existing click event listener on terminal container
   - Convert click Y coordinate to display row using charHeight
   - Use FoldManager to determine if clicked line is a fold summary or foldable zone
   - Toggle fold, adjust scroll, force re-render
   - Key considerations:
     - Must not interfere with existing URL click handler (Ctrl+click)
     - Must not interfere with text selection
     - Only trigger on plain left-click (no modifiers)

2. **Add hover cursor feedback**
   - On mousemove, check if mouse is over a foldable region or summary line
   - Set container cursor style to "pointer" or "default" accordingly

3. **Integrate with search**
   - In scrollToCurrentMatch, check if match is in collapsed region
   - If so, expand the region before scrolling
   - Recalculate match positions after expansion

4. **Integrate with prompt jump**
   - In prompt jump handler, check if target is in collapsed region
   - If so, expand the region before scrolling

**Dependencies**:
- Requires: Phase 3 (rendering must work)
- Blocks: None

**Testing Approach**:

*Manual Testing*:

| ID | Scenario | Expected Result |
|----|----------|-----------------|
| T4-1 | Click summary line | Region expands, full output visible |
| T4-2 | Click output zone header | Region collapses to summary |
| T4-3 | Hover over summary line | Cursor changes to pointer |
| T4-4 | Search finds match in folded region | Region auto-expands, match highlighted |
| T4-5 | Prompt jump to folded region | Region auto-expands, prompt visible |
| T4-6 | Ctrl+click on URL in fold area | URL opens (no fold toggle) |
| T4-7 | Text selection in fold area | Selection works (no fold toggle) |

**Acceptance Criteria**:
- [ ] Click toggles fold/unfold correctly
- [ ] Cursor shows pointer on foldable areas
- [ ] Search auto-expands folded regions containing matches
- [ ] Prompt jump auto-expands folded regions
- [ ] No interference with URL click, text selection, or PTY mouse tracking

**Estimated Effort**: 中

---

### Phase 5: Settings Integration

**Goal**: Add fold_enabled setting to Rust backend and TypeScript frontend, with settings UI toggle.

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - Add fold_enabled field to AppSettings
- `src/settings/types.ts` - Add fold_enabled to AppSettings interface
- `src/settings/settings-sections.ts` - Add fold toggle to Terminal settings section
- `src/settings/settings-applier.ts` - Apply fold_enabled to FoldManager
- `src-tauri/locales/en.json` - Add translation key
- `src-tauri/locales/ja.json` - Add translation key
- `src/i18n/locales/en.json` - Add frontend translation key
- `src/i18n/locales/ja.json` - Add frontend translation key

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings.fold_enabled (Rust) | Persist fold setting | - | Serialized/deserialized with default true |
| AppSettings.fold_enabled (TS) | Mirror Rust setting | - | Available to frontend |
| Settings UI toggle | ON/OFF switch in Terminal section | Settings panel rendered | Toggle visible and functional |
| applyFoldEnabled | Apply setting to FoldManager | FoldManager exists | Enabled/disabled, unfoldAll on disable |

**Processing Flow**:
```
1. Settings load
   ├─ Rust deserializes fold_enabled (default: true)
   └─ Frontend receives and applies to FoldManager
2. Settings change
   ├─ User toggles fold_enabled in settings UI
   ├─ Setting saved via Rust backend
   ├─ applyFoldEnabled called
   │   ├─ ON → FoldManager.setEnabled(true)
   │   └─ OFF → FoldManager.setEnabled(false) + unfoldAll()
   └─ Re-render terminal
```

**Implementation Steps**:

1. **Add Rust setting field**
   - Add `fold_enabled: bool` with `serde(default = "default_true")` and null deserializer
   - Follows existing pattern (same as `url_detection`)

2. **Add TypeScript setting field**
   - Add to AppSettings interface
   - Add to settings-applier

3. **Add settings UI toggle**
   - Add toggle switch in Terminal section (similar to existing booleans like `url_detection`)
   - Add i18n keys for label

**Dependencies**:
- Requires: Phase 1 (FoldManager exists)
- Blocks: None (can be done in parallel with Phases 3-4)

**Testing Approach**:

*Unit Tests*:

| ID | Scenario | Expected Result |
|----|----------|-----------------|
| T5-1 | Default fold_enabled is true | Setting defaults to ON |
| T5-2 | Deserialize null fold_enabled | Defaults to true |
| T5-3 | Disable unfolds all | All regions expanded on disable |

*Manual Testing*:

| ID | Scenario | Expected Result |
|----|----------|-----------------|
| T5-4 | Toggle fold setting in UI | Setting persists and takes effect |

**Acceptance Criteria**:
- [ ] fold_enabled setting exists in Rust with correct default
- [ ] TypeScript setting mirrors Rust
- [ ] Settings UI shows toggle
- [ ] Disabling unfolds all regions

**Estimated Effort**: 小

---

## Complete File Structure

```
src/
├── terminal/
│   ├── fold-manager.ts              # FoldManager class (NEW)
│   ├── fold-manager.test.ts         # FoldManager unit tests (NEW)
│   ├── semantic-zone.ts             # (existing, queried for C/D pairs)
│   ├── handlers/
│   │   ├── osc_handlers.ts          # Add fold verb handling (MODIFIED)
│   │   └── types.ts                 # Add getFoldManager (MODIFIED)
│   ├── canvas-renderer.ts           # Fold-aware rendering (MODIFIED)
│   └── state.ts                     # Integrate FoldManager (MODIFIED)
├── terminal-app/
│   └── index.ts                     # Click handling, search/prompt integration (MODIFIED)
├── settings/
│   ├── types.ts                     # Add fold_enabled (MODIFIED)
│   ├── settings-sections.ts         # Add fold toggle (MODIFIED)
│   └── settings-applier.ts          # Apply fold_enabled (MODIFIED)
├── i18n/
│   └── locales/
│       ├── en.json                  # Add fold setting label (MODIFIED)
│       └── ja.json                  # Add fold setting label (MODIFIED)
└── types/
    └── terminal.ts                  # (existing, no changes needed)

src-tauri/
├── src/
│   └── commands/
│       └── config.rs                # Add fold_enabled setting (MODIFIED)
└── locales/
    ├── en.json                      # Add fold validation key (MODIFIED)
    └── ja.json                      # Add fold validation key (MODIFIED)
```

## Testing Strategy

### Unit Testing

**Approach**:
- Bun test runner for TypeScript
- Focused on FoldManager logic (pure data structure, no Canvas dependency)
- Table-driven tests for line mapping edge cases

**Test Coverage Goals**:
- FoldManager core logic: 90%+
- OSC handler fold routing: 80%+
- Settings integration: 70%+

### Integration Testing

**Docker (recommended)**:
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### Manual Testing

Items requiring visual verification:
- Summary line appearance and colors
- Fold/unfold click interaction
- Scroll position stability
- Cursor pointer feedback
- Search/prompt jump auto-expansion

## Dependencies

### External Dependencies

None.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: FoldManager core (no dependencies)
2. Phase 2: OSC integration (depends on Phase 1)
3. Phase 5: Settings (depends on Phase 1, can parallel with 3-4)
4. Phase 3: Canvas rendering (depends on Phases 1, 2)
5. Phase 4: Mouse interaction and integration (depends on Phase 3)

**Component Dependencies**:
- `fold-manager.ts` depends on nothing (standalone)
- `osc_handlers.ts` depends on `fold-manager.ts`, `semantic-zone.ts`, `state.ts`
- `canvas-renderer.ts` depends on `fold-manager.ts`
- `terminal-app/index.ts` depends on `fold-manager.ts`, `canvas-renderer.ts`

## Risk Assessment

### Technical Risks

1. **Scroll Offset Complexity**
   - **Risk**: Fold-aware scroll calculations may introduce subtle bugs (off-by-one, viewport jumps)
   - **Likelihood**: Medium
   - **Impact**: High (poor UX)
   - **Mitigation**: Thorough unit tests for line mapping; visual regression testing

2. **Canvas Render Performance**
   - **Risk**: Additional fold checks per render frame may slow rendering with many fold regions
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Cached sorted region list; binary search for offset calculation; benchmark with 100+ regions

3. **Search/Prompt Jump Integration**
   - **Risk**: Auto-expanding folds during search may cause unexpected scroll jumps or highlight misalignment
   - **Likelihood**: Medium
   - **Impact**: Medium
   - **Mitigation**: Recalculate positions after expansion; test with multiple folded regions

## Performance Considerations

1. **Fold Offset Calculation**: Pre-compute cumulative offset table, invalidate on fold toggle. Binary search for O(log n) lookup.
2. **Render Pipeline**: Summary lines use the same Canvas context and font metrics — no extra setup.
3. **Region Storage**: Map with string key for O(1) region lookup by ID.

## Security Considerations

1. **Canvas-only Rendering**: All fold labels and summary text rendered on Canvas. No DOM injection risk.
2. **Label Truncation**: Labels truncated at 80 characters at display time.
3. **No External Data**: All fold data originates from the PTY stream, same trust model as existing terminal content.

## Open Questions

### From Specification:
- [ ] Detailed auto-expand behavior for search (currently: expand region, then scroll to match)

### Implementation-Specific:
- None

## Success Metrics

### Functional Completeness
- [ ] All 10 functional requirements (FR1-FR10) implemented
- [ ] All unit and integration tests pass
- [ ] All 4 user stories satisfied

### Quality Metrics
- [ ] Test coverage: 90%+ for FoldManager
- [ ] No TypeScript type errors (`bun run typecheck` passes)
- [ ] No regressions in existing tests

### Performance Metrics
- [ ] Fold/unfold toggle < 16ms
- [ ] 100+ fold regions render within frame budget

### User Experience
- [ ] Summary lines clearly indicate fold state and content
- [ ] No viewport jumps on fold/unfold
- [ ] Cursor feedback on foldable areas

## References

- **Specification**: `doc/tasks/output-folding/SPEC.md`
- **Requirements**: `doc/tasks/output-folding/要件定義書.md`
- **Existing OSC 133 Implementation**: `doc/tasks/semantic-scroll-and-search/SPEC.md`
- **SemanticZoneTracker**: `src/terminal/semantic-zone.ts`
- **Canvas Renderer**: `src/terminal/canvas-renderer.ts`
- **OSC Handlers**: `src/terminal/handlers/osc_handlers.ts`
- **Settings Pattern**: `src-tauri/src/commands/config.rs`
