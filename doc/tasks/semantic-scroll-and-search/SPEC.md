# Feature: Semantic Scroll / Prompt Jump & In-Terminal Text Search

## Overview

Implement OSC 133 (Semantic Prompts) support for zone-aware navigation and a full-featured in-terminal text search. OSC 133 enables the terminal to identify prompt/command/output zones, allowing users to jump between prompts. The search feature provides incremental search across the entire scrollback buffer with regex support and match highlighting on the canvas renderer.

## Objectives

- Parse and record OSC 133 A/B/C/D markers for semantic zone identification
- Enable prompt-to-prompt navigation via keyboard shortcuts
- Provide incremental text search across scrollback and screen buffers
- Highlight search matches directly on the canvas renderer
- Support regex mode and case-sensitivity toggles

## User Stories

### US1: Prompt Jump Navigation
As a Claude Code user, I want to jump between shell prompts in the scrollback, so that I can quickly navigate to previous command interactions.

**Acceptance Criteria:**
- [ ] Ctrl+Shift+Up jumps to the previous prompt marker
- [ ] Ctrl+Shift+Down jumps to the next prompt marker
- [ ] When no more prompts exist above, scroll to top of scrollback
- [ ] When no more prompts exist below, scroll to bottom (latest output)
- [ ] Keybinds are configurable via KeybindSettings

### US2: In-Terminal Text Search
As a terminal user, I want to search for text in the terminal output, so that I can find file names, error messages, and function names mentioned in past output.

**Acceptance Criteria:**
- [ ] Ctrl+Shift+F opens a floating search bar at the top of the terminal
- [ ] Search is incremental (updates as user types)
- [ ] All matches are highlighted on the canvas with a distinct background color
- [ ] Current match has a different (more prominent) highlight color
- [ ] Hit count displayed as "N/M" (current/total)
- [ ] Enter moves to next match, Shift+Enter moves to previous match
- [ ] Matches wrap around (last → first, first → last)
- [ ] Esc closes the search bar and clears highlights
- [ ] Regex toggle button switches between plain text and regex modes
- [ ] Case sensitivity toggle button
- [ ] Invalid regex patterns show an error indicator without crashing

## Technical Requirements

### Functional Requirements

- **FR1:** Rust ANSI parser recognizes OSC 133 sequences and emits `OscAction::SemanticPrompt` variants
- **FR2:** TypeScript OSC handler records semantic zone markers (prompt start, command start, output start, output end with exit code)
- **FR3:** Zone markers are stored as a list associated with line indices in the scrollback + screen buffer
- **FR4:** Zone markers are pruned when scrollback lines are discarded (max scrollback exceeded)
- **FR5:** Prompt jump navigates to the nearest prompt marker above/below current scroll position
- **FR6:** Search operates over the combined scrollback buffer and current screen buffer
- **FR7:** Search supports plain text mode (default) and regex mode (toggle)
- **FR8:** Search supports case-insensitive (default) and case-sensitive modes (toggle)
- **FR9:** Search matches are highlighted on the canvas renderer
- **FR10:** Current match is visually distinct from other matches

### Non-Functional Requirements

- **NFR1 - Performance:** Prompt jump completes in < 10ms
- **NFR2 - Performance:** Incremental search over 10,000 lines completes in < 50ms
- **NFR3 - Performance:** Visible match highlight rendering completes within one frame (< 16ms)
- **NFR4 - Security:** Regex execution has timeout protection against ReDoS
- **NFR5 - Usability:** Search bar does not block terminal input when unfocused

## Implementation Approach

### Architecture

**Component Diagram:**
```
┌──────────────────────────────────────────────────────────┐
│                    Rust Backend                           │
│  ┌─────────────┐    ┌──────────────┐                     │
│  │ ANSI Parser  │───→│  OscAction   │                     │
│  │ (parser.rs)  │    │::Semantic    │                     │
│  │              │    │  Prompt      │                     │
│  └─────────────┘    └──────┬───────┘                     │
└────────────────────────────┼─────────────────────────────┘
                             │ JSON
┌────────────────────────────┼─────────────────────────────┐
│                  TypeScript Frontend                      │
│                            ▼                              │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ OSC Handler  │─→│ Semantic     │  │ Search State   │  │
│  │              │  │ Zone Tracker │  │ Manager        │  │
│  └──────────────┘  └──────┬───────┘  └───────┬────────┘  │
│                           │                   │           │
│                    ┌──────▼───────────────────▼────────┐  │
│                    │         Terminal State            │  │
│                    │  (scrollback + screen buffer)     │  │
│                    └──────────────┬────────────────────┘  │
│                                  │                        │
│                    ┌─────────────▼─────────────────────┐  │
│                    │      Canvas Renderer              │  │
│                    │  (+ match highlight overlay)      │  │
│                    └──────────────────────────────────┘  │
│                                                          │
│  ┌──────────────────────────────────────────────────┐    │
│  │              Search Bar UI (DOM)                  │    │
│  │  [input] [.*] [Aa] [N/M] [↑] [↓] [×]            │    │
│  └──────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────┘
```

### Data Flow

#### OSC 133 Processing
```
Shell (bash/zsh/fish)
  → emits OSC 133 ; {A|B|C|D} [; params] ST
  → Rust parser (parser.rs)
  → OscAction::SemanticPrompt { zone_type, exit_code? }
  → JSON serialization
  → TypeScript osc_handlers.ts
  → SemanticZoneTracker.addMarker(type, lineIndex)
```

#### Search Flow
```
User types in search bar
  → SearchStateManager.setQuery(text)
  → Iterate scrollback + screen lines
  → Build SearchMatch[] array
  → Update currentMatchIndex
  → Notify canvas renderer of match positions
  → Canvas renderer draws highlight rectangles on matched cells
```

### Rust Changes

#### New OscAction Variant (sequence.rs)

```rust
pub enum OscAction {
    // ... existing variants ...

    /// OSC 133 - Semantic Prompt zones
    SemanticPrompt {
        /// Zone type: "A" (prompt start), "B" (command start),
        /// "C" (output start), "D" (output end)
        zone_type: String,
        /// Exit code (only for zone_type "D")
        exit_code: Option<i32>,
    },
}
```

#### Parser Changes (parser.rs)

In the OSC string handler, add recognition for `133;` prefix:
- Parse `133;A` → SemanticPrompt { zone_type: "A", exit_code: None }
- Parse `133;B` → SemanticPrompt { zone_type: "B", exit_code: None }
- Parse `133;C` → SemanticPrompt { zone_type: "C", exit_code: None }
- Parse `133;D;{code}` → SemanticPrompt { zone_type: "D", exit_code: Some(code) }
- Parse `133;D` → SemanticPrompt { zone_type: "D", exit_code: Some(0) }

### TypeScript Changes

#### New Files

```
src/
├── terminal/
│   ├── semantic-zone.ts          # SemanticZoneTracker class
│   └── search/
│       ├── search-state.ts       # SearchStateManager class
│       ├── search-bar.ts         # Search bar UI component (DOM)
│       └── search-bar.css        # Search bar styles
```

#### Modified Files

```
src/
├── terminal/
│   ├── handlers/
│   │   └── osc_handlers.ts    # Add SemanticPrompt case
│   ├── state.ts               # Add SemanticZoneTracker, search state
│   ├── canvas-renderer.ts     # Add search match highlight rendering
│   └── handlers/types.ts      # Update TerminalStateAccessor
├── terminal-app/
│   ├── handlers/keyboard.ts   # Add prompt jump keybinds
│   └── index.ts               # Wire up search bar
├── settings/
│   └── types.ts               # Add keybind entries
└── types/
    └── terminal.ts            # Add SemanticPrompt to OscAction type
```

#### SemanticZoneTracker (semantic-zone.ts)

```typescript
interface SemanticMarker {
  type: "A" | "B" | "C" | "D";
  /** Absolute line index (scrollback offset + screen row) */
  lineIndex: number;
  exitCode?: number;
}

class SemanticZoneTracker {
  private markers: SemanticMarker[] = [];

  addMarker(type: string, lineIndex: number, exitCode?: number): void;
  getPromptMarkers(): SemanticMarker[];  // Returns only type "A" markers
  findPrevPrompt(currentLine: number): SemanticMarker | null;
  findNextPrompt(currentLine: number): SemanticMarker | null;
  pruneBeforeLine(lineIndex: number): void;  // Remove markers for discarded lines
  clear(): void;
}
```

#### SearchStateManager (search-state.ts)

```typescript
interface SearchMatch {
  lineIndex: number;  // Absolute line index (scrollback + screen)
  startCol: number;
  endCol: number;     // Exclusive
}

interface SearchOptions {
  isRegex: boolean;
  caseSensitive: boolean;
}

class SearchStateManager {
  query: string;
  options: SearchOptions;
  matches: SearchMatch[];
  currentMatchIndex: number;

  setQuery(query: string): void;
  setOptions(options: Partial<SearchOptions>): void;
  executeSearch(scrollback: Line[], screen: Line[]): void;
  nextMatch(): SearchMatch | null;
  prevMatch(): SearchMatch | null;
  getVisibleMatches(startLine: number, endLine: number): SearchMatch[];
  clear(): void;
}
```

#### Canvas Renderer Changes

Add a method to draw highlight rectangles for search matches:
```typescript
// In canvas-renderer.ts
renderSearchHighlights(matches: SearchMatch[], currentMatch: SearchMatch | null): void;
```

This renders semi-transparent rectangles over matched cell ranges:
- All matches: yellow semi-transparent background (e.g., rgba(255, 200, 0, 0.3))
- Current match: orange semi-transparent background (e.g., rgba(255, 140, 0, 0.5))

### Dependencies

**Internal Dependencies:**
- `terminal/buffer.ts`: ScreenBuffer for accessing screen lines
- `terminal/state.ts`: TerminalState for scrollback access
- `terminal/grid.ts`: Line/Cell for text extraction
- `terminal/canvas-renderer.ts`: For highlight rendering
- `terminal/handlers/osc_handlers.ts`: For OSC dispatch
- `keybind/matcher.ts`: For keybind matching
- `settings/types.ts`: For KeybindSettings

**External Dependencies:**
- None (all built with standard Web APIs)

### KeybindSettings Additions

```typescript
interface KeybindSettings {
  // ... existing fields ...
  jump_to_prev_prompt: string;  // Default: "Ctrl+Shift+ArrowUp"
  jump_to_next_prompt: string;  // Default: "Ctrl+Shift+ArrowDown"
  // search already exists
}
```

Corresponding Rust settings struct update with serde defaults.

## Test Scenarios

### Unit Tests (Rust)

- [ ] Parse OSC 133;A correctly
- [ ] Parse OSC 133;B correctly
- [ ] Parse OSC 133;C correctly
- [ ] Parse OSC 133;D;0 with exit code
- [ ] Parse OSC 133;D without exit code (defaults to 0)
- [ ] Ignore unknown OSC 133 sub-commands (e.g., OSC 133;X)
- [ ] Serialize SemanticPrompt action to JSON correctly

### Unit Tests (TypeScript)

- [ ] SemanticZoneTracker: Add and retrieve markers
- [ ] SemanticZoneTracker: findPrevPrompt returns correct marker
- [ ] SemanticZoneTracker: findNextPrompt returns correct marker
- [ ] SemanticZoneTracker: findPrevPrompt returns null when none above
- [ ] SemanticZoneTracker: findNextPrompt returns null when none below
- [ ] SemanticZoneTracker: pruneBeforeLine removes old markers and adjusts indices
- [ ] SearchStateManager: Plain text search finds matches
- [ ] SearchStateManager: Case-insensitive search (default)
- [ ] SearchStateManager: Case-sensitive search
- [ ] SearchStateManager: Regex search
- [ ] SearchStateManager: Invalid regex does not crash
- [ ] SearchStateManager: nextMatch wraps around
- [ ] SearchStateManager: prevMatch wraps around
- [ ] SearchStateManager: getVisibleMatches returns only matches in range
- [ ] SearchStateManager: Empty query returns no matches

### Integration Tests

- [ ] OSC 133 sequence → marker recorded in zone tracker
- [ ] Prompt jump scrolls to correct position
- [ ] Search highlights visible on canvas
- [ ] Search bar open/close lifecycle

### Edge Cases

- [ ] No prompt markers exist: jump does nothing
- [ ] Scrollback full (10,000 lines): markers for old lines are pruned
- [ ] Alternate buffer active: OSC 133 markers not recorded
- [ ] Wide characters (CJK): search correctly handles multi-cell characters
- [ ] Empty lines: search handles lines with no content
- [ ] Regex with special characters: proper escaping in plain text mode

## Security Considerations

- **ReDoS Prevention:** Regex search execution should have a timeout mechanism (e.g., abort if search takes > 100ms) or use a safe regex evaluation approach
- **Input Validation:** Search input is used as-is for text matching; no DOM injection risk since rendering is on canvas
- **No External Data:** All search data is local terminal content

## Error Handling

### Error Scenarios

| Error | Condition | Handling |
|-------|-----------|---------|
| Invalid regex | User enters invalid regex pattern | Show error indicator in search bar, do not execute search |
| ReDoS timeout | Complex regex on large buffer | Abort search, show timeout message |
| OSC 133 malformed | Unexpected format in OSC 133 data | Log warning, ignore the sequence |

## Performance Optimization

### Search Performance Strategy

1. **Text extraction cache:** Cache plain text representation of each line to avoid repeated cell iteration
2. **Incremental search:** When query is extended (typing more chars), filter existing matches instead of full re-search
3. **Visible-only highlight:** Only compute highlight rectangles for currently visible lines
4. **Debounce:** Optional debounce on search input (if performance becomes an issue)

### Prompt Jump Performance

- Markers stored in a sorted array for O(log n) binary search
- At most ~1000 markers expected for 10,000 lines of scrollback

## Implementation Phases

### Phase 1: OSC 133 Foundation
**Goals:** Parse and record OSC 133 markers
**Deliverables:**
- Rust parser changes for OSC 133
- TypeScript SemanticZoneTracker
- OSC handler integration
- Unit tests for parsing and tracking

### Phase 2: Prompt Jump
**Goals:** Enable prompt-to-prompt navigation
**Deliverables:**
- KeybindSettings additions
- Keyboard handler for prompt jump
- Scroll position management
- Integration with SemanticZoneTracker

### Phase 3: Search Engine
**Goals:** Core search functionality
**Deliverables:**
- SearchStateManager
- Plain text and regex search
- Case sensitivity toggle
- Match list management

### Phase 4: Search UI
**Goals:** Complete search user experience
**Deliverables:**
- Search bar DOM component
- Canvas renderer highlight integration
- Hit count display
- Match navigation (Enter/Shift+Enter)
- Open/close lifecycle

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Performance meets specified goals (search < 50ms on 10k lines)
- [ ] OSC 133 correctly parsed from bash/zsh output
- [ ] Search bar UI is usable and responsive
- [ ] Keybinds are configurable via settings
- [ ] No regressions in existing terminal functionality

## Open Questions

- [ ] None (all requirements confirmed)

## References

- OSC 133 Semantic Prompts specification (FinalTerm / iTerm2 / WezTerm)
- Existing eMterm OSC handler: `src/terminal/handlers/osc_handlers.ts`
- Existing canvas renderer: `src/terminal/canvas-renderer.ts`
- Existing keybind system: `src/keybind/matcher.ts`
