# Feature: Settings Panel

## Overview

Settings panel implementation for eMterm that allows users to configure font size. The panel opens as a dedicated tab with a category-based navigation layout. Settings are persisted to a JSON file and applied at application startup.

## Objectives

- Implement settings tab UI with category navigation
- Enable font size configuration (8-32pt range)
- Persist settings to `~/.config/emterm/settings.json`
- Apply settings at application startup
- Real-time preview of font size changes

## User Stories

### US1: Open Settings
As a user, I want to click the gear button to open the settings panel, so that I can configure the application.

**Acceptance Criteria:**
- [ ] Gear button in tab bar opens settings tab
- [ ] Settings tab is singleton (clicking gear when open activates existing tab)
- [ ] Settings tab can be closed like other tabs

### US2: Change Font Size
As a user, I want to change the terminal font size, so that I can customize the text readability.

**Acceptance Criteria:**
- [ ] Font size input accepts values 8-32
- [ ] Changes are reflected immediately in terminal tabs
- [ ] Settings are saved automatically on blur/Enter

### US3: Persistent Settings
As a user, I want my settings to persist across app restarts, so that I don't have to reconfigure each time.

**Acceptance Criteria:**
- [ ] Settings are saved to `~/.config/emterm/settings.json`
- [ ] Settings are loaded and applied at startup
- [ ] Missing settings file uses default values

## Technical Requirements

### Functional Requirements
- **FR1:** Settings tab opens via gear button (singleton behavior)
- **FR2:** Font size configurable in 8-32pt range via number input
- **FR3:** Font size changes apply immediately to all terminal tabs
- **FR4:** Settings auto-save on input blur or Enter key
- **FR5:** Settings load and apply at application startup
- **FR6:** Default values managed in backend (Rust), frontend receives fully populated settings

### Non-Functional Requirements
- **NFR1 - Performance:** Font size changes reflect within 16ms
- **NFR2 - Extensibility:** Settings structure supports future additions

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Frontend (TypeScript)                │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │SettingsPanel│  │SettingsService│  │SettingsApplier│ │
│  │   (UI)      │──│  (Load/Save)  │──│ (CSS vars)    │  │
│  └─────────────┘  └──────────────┘  └───────────────┘  │
│         │                │                              │
│         │                │ invoke                       │
├─────────┴────────────────┴──────────────────────────────┤
│                    Backend (Rust/Tauri)                 │
├─────────────────────────────────────────────────────────┤
│  ┌──────────────────────────────────────────────────┐  │
│  │              config.rs                            │  │
│  │  load_settings / save_settings commands           │  │
│  └──────────────────────────────────────────────────┘  │
│                          │                              │
│                          ▼                              │
│  ┌──────────────────────────────────────────────────┐  │
│  │         ~/.config/emterm/settings.json           │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Data Flow

```
User Input → SettingsPanel → SettingsService.save() → Tauri Command → File
                   │
                   └→ SettingsApplier.applySettingsToCSS() → CSS Variables → Terminal UI

Startup → SettingsService.load() → Tauri Command → File
                   │
                   └→ SettingsApplier.applySettingsToCSS() → CSS Variables → Terminal UI
```

### API Design

#### Tauri Command: load_settings

**Request:**
```rust
#[tauri::command]
pub fn load_settings() -> Result<AppSettings, String>
```

**Response (Success):**
```json
{
  "font_size": 13
}
```

**Behavior:**
- File exists with valid font_size: Returns saved value
- File exists without font_size or with null: Returns DEFAULT_FONT_SIZE (13)
- File not found: Returns DEFAULT_FONT_SIZE (13)
- File corrupted: Returns DEFAULT_FONT_SIZE (13), logs warning

Note: The backend always returns a valid `font_size` value (never null). Default value management is handled in Rust.

#### Tauri Command: save_settings

**Request:**
```rust
#[tauri::command]
pub fn save_settings(settings: AppSettings) -> Result<(), String>
```

**Input:**
```json
{
  "font_size": 13
}
```

**Validation:**
- `font_size`: Must be in range 8-32 (if provided)
- Out of range values return `Err("font_size must be between 8 and 32")`

**Response (Success):** `Ok(())`

**Response (Error - Validation):** `Err("font_size must be between 8 and 32")`

**Response (Error - IO):** `Err("Failed to write settings file")`

### File Structure

```
src-tauri/
└── src/
    └── commands/
        └── config.rs          # Add load_settings, save_settings

src/
├── settings/
│   ├── index.ts               # Module exports (modify)
│   ├── types.ts               # AppSettings interface (new)
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

### Settings File Format

**Path:** `~/.config/emterm/settings.json`

```json
{
  "font_size": 13
}
```

### TypeScript Types

```typescript
// src/settings/types.ts

export interface AppSettings {
  font_size: number;  // Always has a valid value (backend provides defaults)
}
```

Note: Default value management is handled in Rust backend. Frontend receives fully populated settings.

### CSS Variables

```css
:root {
  --terminal-font-size: 13px;
  --terminal-line-height: 1.2;
}
```

### UI Layout

```
┌─────────────────────────────────────────────────────────┐
│ Settings                                            [×] │
├──────────────┬──────────────────────────────────────────┤
│              │                                          │
│  Categories  │  Settings Content                        │
│   (160px)    │      (flex: 1)                          │
│              │                                          │
│  ┌─────────┐ │  ┌────────────────────────────────────┐  │
│  │Appearance│ │  │ Appearance                         │  │
│  └─────────┘ │  │                                    │  │
│  ┌─────────┐ │  │ Font Size                          │  │
│  │Terminal │ │  │ ┌──────┐                           │  │
│  └─────────┘ │  │ │  13  │ pt                        │  │
│  ┌─────────┐ │  │ └──────┘                           │  │
│  │Keybinds │ │  │ Range: 8-32pt                      │  │
│  └─────────┘ │  │                                    │  │
│              │  └────────────────────────────────────┘  │
└──────────────┴──────────────────────────────────────────┘
```

**Spacing:**
| Element | Size |
|---------|------|
| Category nav width | 160px (fixed) |
| Content area | flex: 1 |
| Padding | 24px |
| Section gap | 24px |
| Label-input gap | 8px |
| Number input width | 80px |

**Focus style:** Border color `#007acc`

### Dependencies

**Internal Dependencies:**
- `src/tab-bar/tab-manager.ts`: Tab creation and management
- `src/styles.css`: Global CSS imports

**External Dependencies:**
- `@tauri-apps/api`: Tauri invoke for commands

## Implementation Phases

### Phase 1: Rust Settings Persistence

**Files:**
- `src-tauri/src/commands/config.rs` (modify)
- `src-tauri/src/lib.rs` (modify)

**Tasks:**
- Add `AppSettings` struct with `font_size: Option<u32>`
- Implement `get_config_path()` using `app_config_dir()`
- Implement `load_settings` command
- Implement `save_settings` command
- Register commands in `invoke_handler`

### Phase 2: Frontend Settings Service

**Files:**
- `src/settings/types.ts` (new)
- `src/settings/settings-service.ts` (new)
- `src/settings/settings-applier.ts` (new)
- `src/settings/index.ts` (modify)

**Tasks:**
- Define `AppSettings` interface and defaults
- Implement `SettingsService` with load/save methods
- Implement `applySettingsToCSS()` for CSS variable updates
- Export new modules

### Phase 3: Settings UI

**Files:**
- `src/settings/settings-panel.ts` (modify)
- `src/styles/settings-panel.css` (new)
- `src/styles.css` (modify)

**Tasks:**
- Replace placeholder with category nav + content layout
- Implement font size number input (8-32 range)
- Add real-time preview on input change
- Add auto-save on blur/Enter
- Style with dark theme

### Phase 4: TabManager Integration

**Files:**
- `src/tab-bar/tab-manager.ts` (modify)

**Tasks:**
- Store `SettingsPanel` instance reference
- Create `SettingsPanel` in `createSettingsTab()`
- Call `SettingsPanel.dispose()` in `cleanupTabResources()`

### Phase 5: Startup Settings

**Files:**
- `src/main.ts` (modify)

**Tasks:**
- Load settings at startup via `settingsService.load()`
- Apply settings via `applySettingsToCSS()`

## Test Scenarios

### Unit Tests
- [ ] `load_settings` returns default font_size (13) when file missing
- [ ] `load_settings` returns default font_size (13) when font_size is null in file
- [ ] `load_settings` returns saved values when file exists with valid font_size
- [ ] `save_settings` creates config directory if missing
- [ ] `save_settings` writes valid JSON
- [ ] `applySettingsToCSS` updates CSS variables

### Integration Tests
- [ ] Settings tab opens on gear button click
- [ ] Settings tab is singleton
- [ ] Font size change updates terminal immediately
- [ ] Settings persist after tab close and reopen
- [ ] Settings persist after app restart

### E2E Tests
- [ ] Full flow: open settings, change font size, verify terminal update
- [ ] Full flow: change settings, restart app, verify settings loaded

### Edge Cases
- [ ] Font size at minimum (8pt)
- [ ] Font size at maximum (32pt)
- [ ] Invalid font size rejected
- [ ] Corrupted settings file handled gracefully
- [ ] Missing config directory created on save

## Error Handling

### Error Cases

| Scenario | Handling |
|----------|----------|
| Settings file not found | Use default values |
| Settings file corrupted | Use default values, log warning |
| Config directory missing | Create on save |
| File write failure | Return error, log error |
| Invalid font size input (frontend) | Reject input (HTML validation) |
| Invalid font size range (backend) | Return error from save_settings |

## Success Criteria

- [ ] Gear button opens settings tab (singleton)
- [ ] Font size configurable in 8-32pt range
- [ ] Font size changes reflect immediately in terminal
- [ ] Settings save automatically on blur/Enter
- [ ] Settings persist across app restarts
- [ ] All unit tests pass
- [ ] Build succeeds (`cargo test`, `bun run typecheck`)

## Verification

```bash
# Build verification
cargo test --manifest-path src-tauri/Cargo.toml
bun run typecheck

# Manual verification
bun tauri dev
# 1. Click gear button → Settings tab opens
# 2. Change font size → Terminal font updates immediately
# 3. Restart app → Settings preserved

# Settings file check
cat ~/.config/emterm/settings.json
```
