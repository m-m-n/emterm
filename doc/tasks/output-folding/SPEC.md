# Feature: Command Output Folding

## Overview

Enable users to fold (collapse) command output zones in the terminal scrollback. Leverages existing OSC 133 semantic zone markers (C: output start, D: output end) to identify foldable regions, and introduces a custom OSC extension (OSC 777;emterm;fold) for explicit fold region specification from CLI tools. Folded regions are displayed as a single summary line on the Canvas renderer.

## Objectives

- Allow users to collapse long command outputs into a single summary line via mouse click
- Display informative summary lines showing command name, line count, and exit code
- Support custom OSC 777;emterm;fold;begin/end for CLI-driven fold region specification
- Maintain fold state per session with proper scrollback pruning integration
- Provide a global ON/OFF setting for the feature

## User Stories

### US1: Fold Command Output

As a terminal user, I want to click on a command output zone to fold it into a summary line, so that I can reduce visual clutter when reviewing past commands.

**Acceptance Criteria:**
- [ ] Clicking on an output zone (OSC 133 C→D region) folds it into a single summary line
- [ ] Summary line shows: `▶ {command} — {N} lines (exit {code})`
- [ ] Exit code 0 uses normal (dim) text color
- [ ] Non-zero exit code uses red-tinted text color
- [ ] Summary line has a semi-transparent bar background

### US2: Unfold Command Output

As a terminal user, I want to click on a folded summary line to expand it back to the full output, so that I can view the details when needed.

**Acceptance Criteria:**
- [ ] Clicking the summary line restores the full output
- [ ] Scroll position adjusts so the user's viewport doesn't jump
- [ ] The ▶ icon changes to ▼ when expanded (indicating foldable state)

### US3: CLI-Driven Fold Regions

As a CLI tool developer, I want to emit OSC sequences to define fold regions, so that my tool's output can be folded in eMterm.

**Acceptance Criteria:**
- [ ] `ESC ] 777 ; emterm ; fold ; begin ; {label} ST` starts a fold region
- [ ] `ESC ] 777 ; emterm ; fold ; end ST` ends a fold region
- [ ] Summary line uses the label instead of a command name
- [ ] No exit code is shown for custom fold regions
- [ ] Consecutive begin without end invalidates the previous begin
- [ ] Orphaned end (without begin) is silently ignored

### US4: Feature Toggle

As a user, I want to enable or disable the folding feature in settings, so that I can choose whether fold functionality is available.

**Acceptance Criteria:**
- [ ] A global ON/OFF toggle exists in settings
- [ ] Disabling unfolds all currently folded regions
- [ ] Re-enabling does not auto-fold anything (manual trigger only)
- [ ] Default is ON (enabled)

## Technical Requirements

### Functional Requirements

- **FR1:** Identify foldable regions from OSC 133 C→D marker pairs in SemanticZoneTracker
- **FR2:** Identify foldable regions from OSC 777;emterm;fold;begin/end sequences
- **FR3:** Render a summary line on Canvas when a region is folded
- **FR4:** Toggle fold state via mouse click on the summary line or output zone header
- **FR5:** Maintain fold state per fold region for the duration of the terminal session
- **FR6:** Adjust scroll offset when folding/unfolding to prevent viewport jumps
- **FR7:** Prune fold state when scrollback lines are discarded
- **FR8:** Extract command text from the B (command start) marker's line for summary display
- **FR9:** Parse OSC 777;emterm;fold;begin;{label} and fold;end in Rust backend
- **FR10:** Provide a global enable/disable setting (default: enabled)

### Non-Functional Requirements

- **NFR1 - Performance:** Fold/unfold toggle completes within a single frame (< 16ms)
- **NFR2 - Performance:** Summary line rendering integrates into existing Canvas render pipeline with minimal overhead
- **NFR3 - Performance:** Fold state lookup is O(1) per region
- **NFR4 - Security:** Fold labels rendered on Canvas (no XSS risk); label strings truncated at display time
- **NFR5 - Usability:** Mouse cursor changes to pointer over clickable fold regions

## Implementation Approach

### Architecture

**Component Diagram:**
```
┌──────────────────────────────────────────────────────────────┐
│                     Rust Backend                              │
│  ┌──────────────┐    ┌─────────────────┐                     │
│  │ ANSI Parser   │───→│ OscAction       │                     │
│  │ (parser.rs)   │    │ ::EmtermExtension (existing, fold verb)│
│  │               │    │ ::SemanticPrompt│  (existing)         │
│  └──────────────┘    └───────┬─────────┘                     │
└───────────────────────────────┼──────────────────────────────┘
                                │ JSON
┌───────────────────────────────┼──────────────────────────────┐
│                    TypeScript Frontend                         │
│                               ▼                               │
│  ┌───────────────┐   ┌───────────────┐   ┌────────────────┐  │
│  │ OSC Handler   │──→│ Fold Manager  │   │ Semantic Zone  │  │
│  │               │   │   (NEW)       │←──│ Tracker        │  │
│  └───────────────┘   └───────┬───────┘   └────────────────┘  │
│                              │                                │
│                    ┌─────────▼────────────────────────────┐   │
│                    │  Canvas Renderer                     │   │
│                    │  + Fold summary line rendering (NEW) │   │
│                    │  + Fold-aware line mapping (NEW)     │   │
│                    └─────────────────────────────────────┘   │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │               Mouse Handler (MODIFIED)                  │  │
│  │  + Click detection on fold regions                      │  │
│  └─────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────┘
```

### Data Flow

#### Fold via OSC 133 Zones
```
Shell emits OSC 133 C/D markers
  → SemanticZoneTracker records C and D markers with line indices
  → FoldManager identifies C→D pairs as foldable regions
  → User clicks on output zone header
  → FoldManager toggles fold state
  → Canvas Renderer adjusts line mapping (skips folded lines, inserts summary line)
  → Scroll offset adjusted
  → Re-render
```

#### Fold via Custom OSC
```
CLI emits OSC 777;emterm;fold;begin;{label}
  → Rust parser emits EmtermExtension action with fold verb
  → OSC handler records fold begin marker with label
CLI emits OSC 777;emterm;fold;end
  → OSC handler records fold end marker
  → FoldManager identifies begin→end pair as foldable region
  → Same fold/unfold flow as OSC 133
```

### New TypeScript Components

#### FoldManager (`src/terminal/fold-manager.ts`)

```typescript
interface FoldRegion {
  /** Unique identifier for this fold region */
  id: string;
  /** Absolute line index of fold start */
  startLine: number;
  /** Absolute line index of fold end (inclusive) */
  endLine: number;
  /** Whether currently collapsed */
  collapsed: boolean;
  /** Source of this fold region */
  source: "osc133" | "custom";
  /** Command text (for osc133 source, extracted from B marker line) */
  commandText?: string;
  /** Fold label (for custom source) */
  label?: string;
  /** Exit code (for osc133 source, from D marker) */
  exitCode?: number;
  /** Number of lines in the fold region */
  lineCount: number;
}

class FoldManager {
  private regions: Map<string, FoldRegion>;
  private enabled: boolean;

  /** Register a foldable region from OSC 133 C→D pair */
  registerOsc133Region(
    startLine: number,
    endLine: number,
    commandText: string,
    exitCode?: number
  ): void;

  /** Register a foldable region from custom OSC fold */
  registerCustomRegion(
    startLine: number,
    endLine: number,
    label: string
  ): void;

  /** Toggle fold state for a region containing the given line */
  toggleFold(lineIndex: number): boolean;

  /** Get fold region at a specific line (if any) */
  getRegionAtLine(lineIndex: number): FoldRegion | null;

  /** Get all collapsed regions (for line mapping) */
  getCollapsedRegions(): FoldRegion[];

  /** Calculate the display line offset caused by folding */
  calculateFoldOffset(lineIndex: number): number;

  /** Map a display line index to an actual line index */
  displayLineToActual(displayLine: number): number;

  /** Map an actual line index to a display line index */
  actualLineToDisplay(actualLine: number): number;

  /** Unfold all regions */
  unfoldAll(): void;

  /** Prune regions for discarded scrollback lines */
  pruneBeforeLine(lineIndex: number): void;

  /** Enable/disable folding */
  setEnabled(enabled: boolean): void;

  /** Check if folding is enabled */
  isEnabled(): boolean;
}
```

#### Custom OSC Fold Tracking

Track pending fold begin markers in the OSC handler:

```typescript
// In osc_handlers.ts or a dedicated fold-osc-handler.ts
interface PendingFoldBegin {
  lineIndex: number;
  label: string;
}
```

### Rust Backend Changes

#### New EmtermExtension Verb: fold

In `parser.rs` (or `osc.rs`), the existing OSC 777;emterm dispatcher already routes to `EmtermExtension`. The fold verb will be handled alongside the existing markdown verb:

- `777;emterm;fold;begin;{label}` → `EmtermExtension { verb: "fold", params: ["begin", label] }`
- `777;emterm;fold;end` → `EmtermExtension { verb: "fold", params: ["end"] }`

No new Rust enum variant needed — reuse existing `EmtermExtension` with verb routing in TypeScript.

### Canvas Renderer Changes

#### Fold-Aware Line Mapping

The Canvas renderer needs to account for folded regions when mapping display lines to actual buffer lines:

1. **During render:** Skip lines inside collapsed regions
2. **Insert summary lines:** Where a collapsed region exists, render a summary line instead
3. **Hit testing:** Convert click coordinates to actual line indices, accounting for fold offsets

#### Summary Line Rendering

Render summary lines with:
- Semi-transparent bar background (e.g., `rgba(60, 60, 80, 0.3)`)
- Left-aligned: `▶` icon + command/label text
- Right-aligned or inline: `— {N} lines (exit {code})`
- Text color: normal (dim) for exit 0, red (#ff6b6b) for non-zero exit code

### Mouse Handler Changes

- Detect clicks on fold summary lines and output zone headers
- Delegate to FoldManager.toggleFold()
- Set cursor to "pointer" on hover over foldable areas

### Settings Changes

#### Rust Settings (commands/config.rs)

```rust
pub struct AppSettings {
    // ... existing fields ...

    /// Enable command output folding
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub fold_enabled: bool,
}
```

#### TypeScript Settings (types.ts)

```typescript
interface TerminalSettings {
  // ... existing fields ...
  fold_enabled: boolean;  // default: true
}
```

### Integration with Existing Features

#### Search Integration

When search finds matches inside a folded region:
- The fold region should be auto-expanded to show the match
- This ensures search results are always visible

#### Prompt Jump Integration

When prompt jump navigates to a line inside a folded region:
- The fold region should be auto-expanded
- The jumped-to line should be visible after expansion

### Dependencies

**Internal Dependencies:**
- `terminal/semantic-zone.ts`: SemanticZoneTracker for OSC 133 markers
- `terminal/state.ts`: TerminalState for scrollback access
- `terminal/canvas-renderer.ts`: For summary line rendering
- `terminal/handlers/osc_handlers.ts`: For OSC dispatch
- `settings/types.ts`: For fold_enabled setting

**External Dependencies:**
- None (all built with standard Web APIs and Canvas)

### File Structure

```
src/
├── terminal/
│   ├── fold-manager.ts              # FoldManager class (NEW)
│   ├── fold-manager.test.ts         # FoldManager unit tests (NEW)
│   ├── handlers/
│   │   ├── osc_handlers.ts          # Add fold verb handling (MODIFIED)
│   │   └── types.ts                 # Add getFoldManager (MODIFIED)
│   ├── semantic-zone.ts             # (existing, used for C/D marker queries)
│   ├── canvas-renderer.ts           # Fold-aware rendering (MODIFIED)
│   └── state.ts                     # Integrate FoldManager (MODIFIED)
├── terminal-app/
│   └── index.ts                     # Wire up fold click handling (MODIFIED)
├── settings/
│   ├── types.ts                     # Add fold_enabled (MODIFIED)
│   ├── settings-sections.ts         # Add fold toggle (MODIFIED)
│   └── settings-applier.ts          # Apply fold_enabled (MODIFIED)
├── i18n/
│   └── locales/
│       ├── en.json                  # Add fold setting label (MODIFIED)
│       └── ja.json                  # Add fold setting label (MODIFIED)
└── types/
    └── terminal.ts                  # (existing, no changes expected)

src-tauri/
├── src/
│   └── commands/
│       └── config.rs                # Add fold_enabled setting (MODIFIED)
└── locales/
    ├── en.json                      # Add fold validation key (MODIFIED)
    └── ja.json                      # Add fold validation key (MODIFIED)
```

## Test Scenarios

### Unit Tests

- [ ] FoldManager: Register OSC 133 region and retrieve by line index
- [ ] FoldManager: Register custom OSC region and retrieve by line index
- [ ] FoldManager: Toggle fold state (collapse and expand)
- [ ] FoldManager: Calculate fold offset with multiple collapsed regions
- [ ] FoldManager: Display line to actual line mapping
- [ ] FoldManager: Actual line to display line mapping
- [ ] FoldManager: Prune regions when scrollback is trimmed
- [ ] FoldManager: Unfold all regions
- [ ] FoldManager: Disabled state prevents toggle
- [ ] FoldManager: Region with 0 lines is not registered
- [ ] FoldManager: Region with 1 line is registered
- [ ] OSC Handler: fold;begin;{label} creates pending fold
- [ ] OSC Handler: fold;end completes pending fold
- [ ] OSC Handler: fold;end without begin is ignored
- [ ] OSC Handler: Consecutive begin invalidates previous begin

### Integration Tests

- [ ] OSC 133 C→D region becomes foldable after D marker received
- [ ] Folding adjusts scroll position correctly
- [ ] Unfolding adjusts scroll position correctly
- [ ] Search result inside folded region triggers auto-expand
- [ ] Prompt jump into folded region triggers auto-expand
- [ ] Setting fold_enabled to false unfolds all regions

### Edge Cases

- [ ] C marker without matching D: region is not foldable (open-ended)
- [ ] D marker without preceding C: ignored
- [ ] Very long command text: truncated in summary line
- [ ] Very long label: truncated in summary line
- [ ] Multiple fold regions adjacent to each other
- [ ] Fold region at the very top of scrollback
- [ ] Fold region at the very bottom (current screen)
- [ ] Scrollback pruning removes part of a fold region
- [ ] Alternate buffer active: fold markers from primary buffer preserved
- [ ] Tab switch preserves fold state

### Performance Tests

- [ ] 100+ fold regions: render performance within 16ms per frame
- [ ] Rapid fold/unfold toggling: no memory leaks or state corruption

## Security Considerations

- **Canvas Rendering:** Summary line text rendered on Canvas, eliminating XSS risk
- **Label Sanitization:** Custom OSC labels are plain strings rendered on Canvas; no HTML interpretation
- **Length Limits:** Labels and command text truncated at display time to prevent rendering issues

## Error Handling

### Error Scenarios

| Error | Condition | Handling |
|-------|-----------|---------|
| Incomplete fold region | C marker without D | Region not registered as foldable |
| Orphaned fold end | end without begin | Silently ignored |
| Invalid fold label | Empty or excessively long label | Use "..." as fallback, truncate at 80 chars |
| Fold in alternate buffer | Alt buffer markers | Ignored (folding only in primary buffer) |

## Performance Optimization

### Strategies

- **Cached fold offset table:** Pre-compute cumulative fold offsets for O(log n) line mapping via binary search
- **Lazy region detection:** Only scan for C→D pairs when fold interaction occurs, not on every marker addition
- **Render pipeline integration:** Summary lines rendered in the same pass as regular lines, no extra render pass

## Success Criteria

- [ ] All functional requirements implemented and tested
- [ ] All unit test scenarios pass
- [ ] Performance meets specified goals (< 16ms fold toggle)
- [ ] OSC 133 fold regions correctly identified from bash/zsh output
- [ ] Custom OSC fold regions work with emterm CLI
- [ ] Summary line visually clear and informative
- [ ] No regressions in search, prompt jump, or general terminal operation
- [ ] Setting toggle works correctly

## Open Questions

- [ ] Detailed behavior when search match is inside folded region: auto-expand the region, or show an indicator on the summary line? (Current decision: auto-expand)

## References

- Existing OSC 133 implementation: `doc/tasks/semantic-scroll-and-search/SPEC.md`
- SemanticZoneTracker: `src/terminal/semantic-zone.ts`
- OSC handler: `src/terminal/handlers/osc_handlers.ts`
- Canvas renderer: `src/terminal/canvas-renderer.ts`
- Feature proposal: `tmp/AI-features.md` (item 3)
