# Implementation Plan: Settings Panel

## Overview

Settings panel implementation for eMterm that allows users to configure font size through a dedicated tab with category-based navigation. Settings are persisted to JSON file and applied at application startup.

## Objectives

- Implement settings tab UI with category navigation layout
- Enable font size configuration (8-32pt range) with real-time preview
- Persist settings to `~/.config/emterm/settings.json` via Tauri commands
- Apply settings at application startup

## Prerequisites

### Development Environment

- Rust 1.70+ with Tauri dependencies
- Bun (package manager and bundler)
- Node.js for TypeScript tooling

### Dependencies

- `@tauri-apps/api` for invoke commands
- `serde` / `serde_json` for Rust serialization
- Tauri's `app_config_dir()` API for config path resolution

### Knowledge Requirements

- Tauri command pattern (invoke/command)
- TypeScript module structure in this project
- Existing TabManager and SettingsPanel architecture
- CSS variable-based theming

## Architecture Overview

### Technology Stack

- **Backend**: Rust (Tauri)
- **Frontend**: Vanilla TypeScript
- **Bundler**: Bun
- **Styling**: CSS with CSS variables

### Design Approach

Three-layer architecture:
1. **Persistence Layer** (Rust): Read/write settings JSON file
2. **Service Layer** (TypeScript): Bridge between UI and backend commands
3. **Presentation Layer** (TypeScript): Settings panel UI component

### Component Interaction

```
User Input --> SettingsPanel --> SettingsService --> Tauri Command --> File
                    |
                    +--> SettingsApplier --> CSS Variables --> Terminal UI

Startup --> main.ts --> SettingsService.load() --> SettingsApplier --> CSS Variables
```

## Implementation Phases

### Phase 1: Rust Settings Persistence

**Goal**: Backend commands to load/save settings from JSON file

**Files to Create**: None

**Files to Modify**:
- `src-tauri/src/commands/config.rs`
- `src-tauri/src/lib.rs`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings | Settings data structure with font_size field | - | Serializable/deserializable |
| get_config_path | Resolve config directory path | App handle available | Returns path to settings.json |
| load_settings | Read and parse settings file | - | Returns AppSettings or defaults |
| save_settings | Write settings to file | Valid AppSettings | File written, directory created if needed |

**Processing Flow**:

```
load_settings Flow:
1. Resolve config directory path using app_config_dir()
2. Construct settings.json path
3. Read file
   +-- File exists --> Parse JSON --> Apply defaults for missing fields --> Return AppSettings
   +-- File not found --> Return AppSettings with DEFAULT_FONT_SIZE
   +-- Parse error --> Log warning, return AppSettings with DEFAULT_FONT_SIZE
4. Ensure font_size is never null in response (apply DEFAULT_FONT_SIZE if null)

save_settings Flow:
1. Resolve config directory path
2. Create directory if not exists
3. Serialize AppSettings to JSON
4. Write to file
   +-- Success --> Return Ok
   +-- Error --> Return error message
```

**Implementation Steps**:

1. **Define AppSettings struct and defaults**
   - Responsibility: Hold settings data with font_size
   - Key considerations:
     - Define `DEFAULT_FONT_SIZE: u32 = 16` constant
     - Use `u32` for font_size in response (always populated)
     - Use `Option<u32>` internally for file parsing to detect missing values
     - Derive Serialize, Deserialize

2. **Implement path resolution**
   - Responsibility: Get platform-specific config path
   - Key considerations:
     - Use Tauri's app_config_dir API
     - Handle path construction safely

3. **Implement load_settings command**
   - Responsibility: Read settings from file, apply defaults, return fully populated settings
   - Key considerations:
     - Handle missing file gracefully (return defaults)
     - Handle corrupted JSON gracefully (return defaults, log warning)
     - Apply DEFAULT_FONT_SIZE when font_size is null or missing
     - Always return valid font_size value (never null)

4. **Implement save_settings command**
   - Responsibility: Validate and write settings to file
   - Key considerations:
     - Validate font_size range (8-32) before saving
     - Return error "font_size must be between 8 and 32" if out of range
     - Create directory if not exists
     - Return meaningful error messages

5. **Register commands in invoke_handler**
   - Responsibility: Expose commands to frontend

**Dependencies**:
- Requires: None
- Blocks: Phase 2, Phase 5

**Testing Approach**:

*Unit Tests*:
- Test load returns DEFAULT_FONT_SIZE (16) when file missing
- Test load returns DEFAULT_FONT_SIZE (16) when font_size is null in file
- Test load parses valid JSON and returns saved font_size
- Test save creates directory if missing
- Test save writes valid JSON
- Test save rejects font_size below 8
- Test save rejects font_size above 32
- Test save accepts font_size in valid range (8-32)
- Test round-trip: save then load

*Manual Testing*:
- [ ] Verify settings.json created at expected path
- [ ] Verify JSON format matches specification

**Acceptance Criteria**:
- [ ] `DEFAULT_FONT_SIZE` constant defined as 16
- [ ] `load_settings` command returns AppSettings with valid font_size (never null)
- [ ] `load_settings` returns DEFAULT_FONT_SIZE when file missing
- [ ] `load_settings` returns DEFAULT_FONT_SIZE when font_size is null in file
- [ ] `save_settings` validates font_size range (8-32)
- [ ] `save_settings` returns error for out-of-range font_size
- [ ] `save_settings` command writes JSON file
- [ ] `save_settings` creates ~/.config/emterm/ if missing
- [ ] Commands registered and callable from frontend

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Config directory permissions on different platforms
  - **Mitigation**: Use Tauri's cross-platform APIs, test on target platforms

---

### Phase 2: Frontend Settings Service

**Goal**: TypeScript service layer to load/save settings via Tauri commands

**Files to Create**:
- `src/settings/types.ts`
- `src/settings/settings-service.ts`
- `src/settings/settings-applier.ts`

**Files to Modify**:
- `src/settings/index.ts`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings interface | TypeScript type matching Rust struct | - | Type-safe settings access |
| SettingsService | Load/save settings via invoke | Tauri available | Settings persisted/loaded |
| applySettingsToCSS | Update CSS variables from settings | Settings loaded | CSS variables updated |

Note: Default value management is handled in Rust backend. Frontend receives fully populated settings (font_size is always a valid number).

**Processing Flow**:

```
SettingsService.load() Flow:
1. Invoke load_settings command
2. Receive AppSettings from backend (font_size is always valid, never null)
3. Return settings object

SettingsService.save() Flow:
1. Receive settings to save
2. Invoke save_settings command with settings
3. Handle success/error

applySettingsToCSS() Flow:
1. Use font_size directly from settings (no null check needed)
2. Calculate line height if needed
3. Update :root CSS variables
   +-- --terminal-font-size
   +-- --terminal-line-height (optional)
```

**Implementation Steps**:

1. **Create types.ts**
   - Responsibility: Define AppSettings interface
   - Key considerations:
     - Match Rust struct exactly (snake_case for JSON compatibility)
     - font_size is `number` (not `number | null`) since backend provides defaults

2. **Implement SettingsService**
   - Responsibility: Encapsulate Tauri invoke calls
   - Key considerations:
     - Static methods for simplicity
     - Handle invoke errors gracefully

3. **Implement settings applier**
   - Responsibility: Apply settings to DOM via CSS variables
   - Key considerations:
     - Update document.documentElement style
     - Calculate derived values if needed

4. **Update module exports**
   - Responsibility: Export new components

**Dependencies**:
- Requires: Phase 1 (Rust commands)
- Blocks: Phase 3, Phase 5

**Testing Approach**:

*Unit Tests*:
- Test applySettingsToCSS updates CSS variables with font_size value

*Integration Tests*:
- Test load/save round-trip via mocked invoke

**Acceptance Criteria**:
- [ ] AppSettings interface defined with font_size: number (not nullable)
- [ ] SettingsService.load() returns AppSettings with valid font_size
- [ ] SettingsService.save() persists settings
- [ ] applySettingsToCSS() updates --terminal-font-size

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Type mismatch between TypeScript and Rust
  - **Mitigation**: Use consistent naming (snake_case in JSON), test serialization

---

### Phase 3: Settings UI

**Goal**: Replace placeholder with functional settings panel UI

**Files to Create**:
- `src/styles/settings-panel.css`

**Files to Modify**:
- `src/settings/settings-panel.ts`
- `src/styles.css`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Category navigation | Display category list (left side) | Panel rendered | Categories visible, Appearance active |
| Content area | Display settings for active category | Category selected | Settings controls rendered |
| Font size input | Number input with 8-32 range | Content rendered | User can input value |
| Real-time preview | Apply changes immediately | Input value changes | Terminal font updated |
| Auto-save | Save on blur/Enter | Value confirmed | Settings persisted |

**Processing Flow**:

```
Panel Initialization:
1. Load current settings via SettingsService
2. Render category navigation
3. Render content area with Appearance section
4. Populate font size input with current value
5. Attach event handlers

Font Size Change Flow:
1. User modifies input value
2. On input event --> Apply immediately (real-time preview)
   +-- Get input value
   +-- Validate range (8-32)
   +-- Call applySettingsToCSS with new value
3. On blur/Enter event --> Save settings
   +-- Get current value
   +-- Call SettingsService.save()
```

**Implementation Steps**:

1. **Create CSS file for settings panel**
   - Responsibility: Style the panel layout
   - Key considerations:
     - Dark theme consistent with app
     - Category nav 160px fixed width
     - Content area flex: 1
     - Focus style: #007acc border

2. **Implement category navigation**
   - Responsibility: Render and manage category selection
   - Key considerations:
     - Three categories: Appearance (active), Terminal, Keybinds (greyed out)
     - Only Appearance is interactive in this phase

3. **Implement Appearance section**
   - Responsibility: Render font size setting
   - Key considerations:
     - Number input type with min=8, max=32
     - Label "Font Size" with "pt" unit suffix
     - Range hint text "Range: 8-32pt"

4. **Implement real-time preview**
   - Responsibility: Update terminal font on input change
   - Key considerations:
     - Listen to input event for immediate feedback
     - Validate before applying

5. **Implement auto-save**
   - Responsibility: Persist on blur and Enter
   - Key considerations:
     - Listen to blur and keydown (Enter) events
     - Compare with last saved value; only save if changed
     - Track lastSavedValue in component state
     - Update lastSavedValue after successful save

6. **Add CSS import to main styles**
   - Responsibility: Include new stylesheet

**Dependencies**:
- Requires: Phase 2 (SettingsService, applySettingsToCSS)
- Blocks: Phase 4 (TabManager integration)

**Testing Approach**:

*Unit Tests*:
- Test panel renders correctly
- Test input validates range

*Manual Testing*:
- [ ] Panel displays with correct layout
- [ ] Font size input accepts 8-32 values
- [ ] Terminal font updates immediately on input change
- [ ] Settings saved on blur
- [ ] Settings saved on Enter key
- [ ] Same value does not trigger redundant save (check logs/network)

**Acceptance Criteria**:
- [ ] Settings panel has category nav (160px) + content area layout
- [ ] Appearance category active, others greyed out
- [ ] Font size number input with min=8, max=32
- [ ] Input change reflects immediately in terminal
- [ ] Settings auto-save on blur and Enter
- [ ] Redundant saves prevented (same value not saved twice)

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: Input validation edge cases
  - **Mitigation**: Use HTML5 number input constraints, add explicit validation
- **Risk**: Race conditions with rapid input changes
  - **Mitigation**: Debounce save calls if needed

---

### Phase 4: TabManager Integration

**Goal**: Integrate SettingsPanel lifecycle with TabManager

**Files to Modify**:
- `src/tab-bar/tab-manager.ts`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| settingsPanels map | Store SettingsPanel instances by tab ID | TabManager initialized | Panel instances tracked |
| createSettingsTab | Create and initialize SettingsPanel | Tab creation requested | Panel created and stored |
| cleanupTabResources | Dispose SettingsPanel on tab close | Tab close requested | Panel disposed, resources freed |

**Processing Flow**:

```
Settings Tab Creation:
1. Create tab container (existing logic)
2. Create SettingsPanel instance with container
3. Call panel.init()
4. Store panel in settingsPanels map
5. Continue existing tab creation flow

Settings Tab Close:
1. Check if tab has associated SettingsPanel
   +-- Yes --> Call panel.dispose()
           --> Remove from settingsPanels map
   +-- No --> Skip (terminal tab)
2. Continue existing cleanup flow
```

**Implementation Steps**:

1. **Add settingsPanels storage**
   - Responsibility: Track SettingsPanel instances
   - Key considerations:
     - Use Map<string, SettingsPanel> parallel to terminalApps

2. **Modify createSettingsTab**
   - Responsibility: Create and initialize SettingsPanel
   - Key considerations:
     - Import SettingsPanel from settings module
     - Pass container to constructor
     - Call init() after creation

3. **Modify cleanupTabResources**
   - Responsibility: Dispose SettingsPanel on close
   - Key considerations:
     - Check settingsPanels map for tab ID
     - Call dispose() if found
     - Remove from map

**Dependencies**:
- Requires: Phase 3 (SettingsPanel implementation)
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- Test SettingsPanel created when settings tab opened
- Test SettingsPanel disposed when settings tab closed

*Integration Tests*:
- Test settings tab lifecycle (create, close)
- Test singleton behavior (gear button with existing tab)

**Acceptance Criteria**:
- [ ] SettingsPanel created in createSettingsTab()
- [ ] SettingsPanel.dispose() called in cleanupTabResources()
- [ ] Settings tab functions correctly (open, use, close)

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Memory leaks from undisposed panels
  - **Mitigation**: Ensure cleanup path always calls dispose

---

### Phase 5: Startup Settings

**Goal**: Load and apply settings at application startup

**Files to Modify**:
- `src/main.ts`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| loadAndApplySettings | Load settings and apply to CSS | App starting | Settings reflected in UI |

**Processing Flow**:

```
Startup Flow (modified):
1. Initialize console bridge (existing)
2. Load settings via SettingsService.load()
3. Apply settings via applySettingsToCSS()
4. Continue with TabManager initialization (existing)
5. Create initial tab (existing)
```

**Implementation Steps**:

1. **Import settings modules**
   - Responsibility: Make service and applier available

2. **Add startup settings loading**
   - Responsibility: Load and apply before UI initialization
   - Key considerations:
     - Place before TabManager creation
     - Handle load errors gracefully (use defaults)
     - Log any errors for debugging

**Dependencies**:
- Requires: Phase 1 (Rust commands), Phase 2 (SettingsService)
- Blocks: None

**Testing Approach**:

*Manual Testing*:
- [ ] Change font size, restart app, verify font size persisted
- [ ] Delete settings file, restart app, verify default font size used
- [ ] Corrupt settings file, restart app, verify app starts with defaults

**Acceptance Criteria**:
- [ ] Settings loaded at startup
- [ ] Font size applied to CSS variables at startup
- [ ] Missing settings file handled gracefully
- [ ] Corrupted settings file handled gracefully

**Estimated Effort**: Small (1 day)

**Risks and Mitigation**:
- **Risk**: Startup delay from settings load
  - **Mitigation**: Load is async, UI can render with defaults first

---

## Complete File Structure

```
src-tauri/
└── src/
    └── commands/
        └── config.rs          # Add AppSettings, load_settings, save_settings

src/
├── settings/
│   ├── index.ts               # Module exports (modify)
│   ├── types.ts               # AppSettings interface, defaults (new)
│   ├── settings-service.ts    # Load/save service (new)
│   ├── settings-applier.ts    # CSS variable updater (new)
│   └── settings-panel.ts      # UI component (modify)
├── styles/
│   └── settings-panel.css     # Panel styles (new)
├── tab-bar/
│   └── tab-manager.ts         # SettingsPanel integration (modify)
├── main.ts                    # Startup settings load (modify)
└── styles.css                 # CSS import (modify)
```

**File Descriptions**:

| File | Purpose |
|------|---------|
| `config.rs` | Rust commands for settings persistence |
| `types.ts` | TypeScript types matching Rust structs |
| `settings-service.ts` | Invoke wrapper for load/save |
| `settings-applier.ts` | CSS variable update logic |
| `settings-panel.ts` | Settings tab UI component |
| `settings-panel.css` | Styling for settings panel |
| `tab-manager.ts` | SettingsPanel lifecycle management |
| `main.ts` | Startup settings application |

## Testing Strategy

### Unit Testing

**Approach**:
- Bun test for TypeScript
- Cargo test for Rust
- Mock Tauri invoke in frontend tests

**Test Coverage Goals**:
- Rust commands: 80%+
- TypeScript services: 80%+
- UI components: 60%+

**Key Test Areas**:

1. **Settings Persistence** (Rust)
   - Load with missing file
   - Load with valid file
   - Load with corrupted file
   - Save creates directory
   - Save writes valid JSON

2. **Settings Service** (TypeScript)
   - Load returns settings
   - Save persists settings
   - Error handling

3. **Settings Applier** (TypeScript)
   - Updates CSS variables with font_size value

4. **Settings Panel** (TypeScript)
   - Renders correctly
   - Input validation
   - Event handling

### Integration Testing

**Scenarios**:
1. Settings tab opens on gear button click
2. Settings tab is singleton (second click activates existing)
3. Font size change updates terminal immediately
4. Settings persist after tab close and reopen
5. Settings persist after app restart

### Manual Testing Checklist

- [ ] Gear button opens settings tab
- [ ] Settings tab layout matches design (160px nav + content)
- [ ] Font size input accepts 8-32 values
- [ ] Invalid values rejected
- [ ] Terminal font changes immediately on input
- [ ] Settings saved on blur
- [ ] Settings saved on Enter
- [ ] App restart preserves settings
- [ ] Missing config file uses defaults
- [ ] Corrupted config file uses defaults

## Dependencies

### External Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| @tauri-apps/api | existing | Invoke Tauri commands |
| serde | existing | Rust JSON serialization |
| serde_json | existing | Rust JSON parsing |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Rust commands (no dependencies)
2. Phase 2: Frontend service (depends on Phase 1)
3. Phase 3: Settings UI (depends on Phase 2)
4. Phase 4: TabManager integration (depends on Phase 3)
5. Phase 5: Startup loading (depends on Phase 1, 2)

**Component Dependencies**:
- `settings-service.ts` depends on Tauri invoke
- `settings-panel.ts` depends on settings-service, settings-applier
- `tab-manager.ts` depends on settings-panel
- `main.ts` depends on settings-service, settings-applier

## Risk Assessment

### Technical Risks

1. **Cross-platform Config Path**
   - **Risk**: Config path resolution differs by platform
   - **Likelihood**: Low (Tauri handles this)
   - **Impact**: Medium
   - **Mitigation**: Use Tauri's app_config_dir() API

2. **JSON Parse Errors**
   - **Risk**: Corrupted settings file crashes app
   - **Likelihood**: Low
   - **Impact**: High
   - **Mitigation**: Graceful fallback to defaults, log warning

### Implementation Risks

1. **Type Mismatch**
   - **Risk**: TypeScript and Rust types diverge
   - **Mitigation**: Test serialization round-trip

2. **Event Handler Leaks**
   - **Risk**: Panel dispose doesn't clean up handlers
   - **Mitigation**: Track and remove all event listeners in dispose()

## Performance Considerations

1. **Font Size Changes**
   - CSS variable updates are synchronous and fast
   - Terminal re-render triggered by CSS change
   - Should meet 16ms requirement

2. **Settings Load at Startup**
   - File read is async, non-blocking
   - Applied before UI visible
   - Negligible impact on startup time

## Security Considerations

1. **File Path Safety**
   - Use Tauri's app_config_dir() for safe path
   - No user-controlled path components

2. **Input Validation**
   - HTML5 number input constraints
   - Server-side validation (min/max)

## Open Questions

### From Specification

None - all requirements are clear.

### Implementation-Specific

None - implementation approach is straightforward.

## Future Enhancements

Items deferred to later phases (from specification):

- **Terminal category**: Scrollback lines, cursor shape
- **Keybinds category**: Custom keyboard shortcuts
- **Appearance category**: Font family, color scheme

## Success Metrics

### Functional Completeness
- [ ] All MVP features implemented (settings tab, font size, persistence)
- [ ] All test scenarios pass
- [ ] Error handling works correctly

### Quality Metrics
- [ ] Rust tests pass: `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] TypeScript types check: `bun run typecheck`
- [ ] No runtime errors in manual testing

### Performance Metrics
- [ ] Font size change reflects within 16ms (NFR1)

### User Experience
- [ ] Intuitive settings navigation
- [ ] Clear input validation feedback
- [ ] Settings persist as expected

## References

- **Requirements**: `doc/tasks/settings-panel/要件定義書.md`
- **Technical Specification**: `doc/tasks/settings-panel/SPEC.md`
- **Tauri Config Path**: https://v2.tauri.app/reference/javascript/api/namespacepath/
- **Existing Code**: `src/settings/settings-panel.ts`, `src-tauri/src/commands/config.rs`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm approach with stakeholders
   - Address any questions

2. **Begin Implementation**
   - Start with Phase 1 (Rust commands)
   - Follow TDD approach where practical
   - Commit incrementally

3. **Verification**
   - Run tests after each phase
   - Manual testing per checklist
   - `/sdd.3-verify-plan` for consistency check
