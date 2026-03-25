# Feature: Status Bar

## Overview

A configurable status bar displayed at the bottom of the application window, outside the terminal screen area. It supports template variables for real-time context display (time, cwd, git branch) and an OSC protocol for external process content injection. Default OFF, enabled from settings.

## Objectives

- Provide persistent context information (time, cwd, git branch) without leaving the terminal
- Enable external processes to display custom content via OSC 777;statusbar protocol
- Allow full appearance customization and user-defined commands with configurable refresh rates

## User Stories

### US1: Enable Status Bar
As a terminal user, I want to enable a status bar from settings, so that I can see context information at a glance.

**Acceptance Criteria:**
- [ ] Status bar toggle in settings panel (default OFF)
- [ ] When enabled, status bar appears at bottom of window
- [ ] Default display: left = `{time}`, right = `{cwd}`

### US2: View Git Branch Status
As a developer, I want to see the current git branch with color indicating dirty/clean state, so that I know the repository status.

**Acceptance Criteria:**
- [ ] `{git_branch}` template variable resolves to current branch name
- [ ] Branch text color changes based on git state (dirty/clean)
- [ ] Updates at configurable refresh rate

### US3: External Content via OSC
As a script developer, I want to send status information to the status bar via OSC sequences, so that I can display custom context from shell scripts.

**Acceptance Criteria:**
- [ ] OSC 777;statusbar;set;left;content sets left section of OSC layer
- [ ] OSC 777;statusbar;clear clears OSC layer
- [ ] HTML tags in OSC content are stripped
- [ ] OSC layer auto-shows when content is set

### US4: Custom Commands
As a power user, I want to define custom commands that periodically execute and display results in the status bar, so that I can show arbitrary system information.

**Acceptance Criteria:**
- [ ] Custom commands defined in settings with name, executable path (no arguments), interval_ms
- [ ] Referenced in templates as `{cmd:name}`
- [ ] Each command runs at its own interval
- [ ] Only a single executable path is accepted (no arguments, no shell expansion)

## Technical Requirements

### Functional Requirements

- **FR1: Layer Structure** - Status bar has 3 layers (top to bottom): OSC layer (1 line, hidden when empty), Application layer line 1, Application layer line 2 (default empty, hidden). Each layer has left/right sections. Maximum 3 lines total.

- **FR2: Template Variable System** - Support `{time}`, `{cwd}`, `{git_branch}`, `{cmd:name}` variables. Each variable has an individual refresh rate (ms, default 1000ms). Variables are resolved and rendered into template strings.

- **FR3: Time Variable** - `{time}` displays current time. Format is configurable in settings (format string like HH:MM:SS). Has its own refresh rate setting.

- **FR4: CWD Variable** - `{cwd}` displays basename of current working directory. Updated via polling and Shell Integration events (OSC 7, OSC 133).

- **FR5: Git Branch Variable** - `{git_branch}` displays current git branch name. Text color changes based on git state (dirty/clean etc.).

- **FR6: Custom Command Variable** - `{cmd:name}` executes a user-defined command and displays stdout. Commands are defined in settings under `statusbar_custom_commands` with a single executable path (no arguments allowed) and individual `interval_ms` (default 1000ms). This restriction simplifies security validation and prevents shell injection.

- **FR7: OSC Protocol** - OSC 777;statusbar;... protocol with commands: `set;left;content`, `set;right;content`, `clear`, `clear;left`, `clear;right`, `show`, `hide`. show/hide controls OSC layer only.

- **FR8: Settings UI** - "Status Bar" subsection under UI settings with: enable/disable toggle, template strings for each app layer line (left/right), time format, custom command definitions (inline add/edit/delete), font size.

- **FR9: Default Display** - When enabled with default settings: 1 line (app layer line 1), left = `{time}`, right = `{cwd}`.

- **FR10: Mux Mode Compatibility** - Status bar is always visible in mux mode regardless of mux state (when status bar is enabled).

### Non-Functional Requirements

- **NFR1 - Performance:** Template variable updates must not block or degrade terminal rendering performance. Custom command execution must be asynchronous.

- **NFR2 - Security:** OSC layer content: all HTML tags completely stripped (plain text only). Internal template content: full HTML/CSS supported (user's responsibility).

- **NFR3 - Platform:** Must work on both Linux and Windows.

- **NFR4 - Consistency:** Follow existing tab bar implementation patterns (WebView HTML/CSS). Use UI design tokens from doc/UI-DESIGN-GUIDELINES.yaml.

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────────────┐
│                   Terminal Screen                          │
├──────────────────────────────────────────────────────────┤
│ Status Bar Container (HTML/CSS, like tab bar)             │
│ ┌──────────────────────────────────────────────────────┐ │
│ │ OSC Layer:  [left]                          [right]  │ │ ← hidden when empty
│ ├──────────────────────────────────────────────────────┤ │
│ │ App Line 1: [left: {time}]           [right: {cwd}] │ │ ← default display
│ ├──────────────────────────────────────────────────────┤ │
│ │ App Line 2: [left]                          [right]  │ │ ← hidden when empty
│ └──────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

**Component Diagram:**

```
Settings (Rust)
  └─ statusbar_* fields → serialized to frontend

StatusBarUI (TypeScript)
  ├─ StatusBarRenderer       # DOM rendering, layer visibility
  ├─ TemplateEngine          # Variable resolution ({time}, {cwd}, etc.)
  ├─ VariableProvider        # Polling/event-driven data collection
  │   ├─ TimeProvider        # Clock with configurable format
  │   ├─ CwdProvider         # From OSC 7 / polling
  │   ├─ GitBranchProvider   # git branch + dirty state
  │   └─ CustomCmdProvider   # User-defined command execution
  └─ OscLayerController      # OSC 777;statusbar command handler

OSC Handler (osc-handler.ts)
  └─ case 100 (OSC 777): route "statusbar" verb → OscLayerController
```

### Data Flow

**Template Variable Update:**
```
Timer/Event → VariableProvider → TemplateEngine → StatusBarRenderer → DOM
```

**OSC Content Update:**
```
PTY → WASM Parser → OSC 777 callback → osc-handler.ts → OscLayerController → DOM
```

**Settings Change:**
```
Settings Panel → Rust Backend → Frontend Event → StatusBarUI.applySettings()
```

### OSC Protocol Design

**Format:** `ESC ] 777 ; statusbar ; <command> ST`

Within the existing OSC 777 routing (osc-handler.ts case 100), add a new verb:

```typescript
// In handleOscCallback, case 100:
if (verb === "emterm" && params[0] === "statusbar") {
  // Route to status bar OSC handler
  statusBarOscHandler(params.slice(1));
}
```

**Commands:**

| Raw Sequence | Parsed Command | Action |
|---|---|---|
| `ESC]777;statusbar;set;left;Hello WorldST` | `set;left;Hello World` | Set OSC layer left text |
| `ESC]777;statusbar;set;right;contentST` | `set;right;content` | Set OSC layer right text |
| `ESC]777;statusbar;clearST` | `clear` | Clear all OSC content |
| `ESC]777;statusbar;clear;leftST` | `clear;left` | Clear left only |
| `ESC]777;statusbar;clear;rightST` | `clear;right` | Clear right only |
| `ESC]777;statusbar;showST` | `show` | Show OSC layer |
| `ESC]777;statusbar;hideST` | `hide` | Hide OSC layer |

**Content sanitization:** Strip all HTML tags from OSC content before rendering. Use a simple regex or DOM-based approach to remove tags.

### Settings Schema

**Rust AppSettings additions:**

```rust
// Status Bar
statusbar_enabled: bool,                    // default: false
statusbar_app_line1_left: String,           // default: "{time}"
statusbar_app_line1_right: String,          // default: "{cwd}"
statusbar_app_line2_left: String,           // default: ""
statusbar_app_line2_right: String,          // default: ""
statusbar_time_format: String,              // default: "HH:mm:ss"
statusbar_custom_commands: HashMap<String, CustomCommand>,  // default: {}
statusbar_font_size: Option<f32>,           // default: None (use UI default)
statusbar_refresh_rates: HashMap<String, u64>,  // per-variable rates in ms
```

```rust
#[derive(Serialize, Deserialize, Clone)]
struct CustomCommand {
    executable: String, // Single executable path only, no arguments
    interval_ms: u64,   // default: 1000
}
```

**TypeScript AppSettings additions:**

```typescript
// Status Bar
statusbar_enabled: boolean;
statusbar_app_line1_left: string;
statusbar_app_line1_right: string;
statusbar_app_line2_left: string;
statusbar_app_line2_right: string;
statusbar_time_format: string;
statusbar_custom_commands: Record<string, { executable: string; interval_ms: number }>;
statusbar_font_size: number | null;
statusbar_refresh_rates: Record<string, number>;
```

### Git Branch Color Logic

Git state detection via command execution:

```
git rev-parse --abbrev-ref HEAD   → branch name
git status --porcelain             → dirty state
```

Color mapping:
- Clean (no changes): default/green
- Dirty (uncommitted changes): yellow/orange
- Untracked files only: dim color

Exact colors follow UI design tokens.

### Dependencies

**Internal Dependencies:**
- `src/terminal-app/osc-handler.ts`: Add statusbar verb routing in OSC 777 handler
- `src/settings/types.ts`: Add statusbar settings fields to AppSettings
- `src-tauri/src/settings/`: Add Rust settings struct fields with defaults
- `src/settings/settings-sections.ts`: Add Status Bar category
- `src/styles/`: Add status bar CSS

**External Dependencies:**
- None (uses existing Tauri commands for shell execution)

### File Structure

```
src/
├── status-bar/
│   ├── index.ts                # StatusBarUI main class
│   ├── renderer.ts             # DOM rendering, layer management
│   ├── template-engine.ts      # Template variable parsing and resolution
│   ├── providers/
│   │   ├── types.ts            # VariableProvider interface
│   │   ├── time-provider.ts    # {time} variable
│   │   ├── cwd-provider.ts     # {cwd} variable
│   │   ├── git-provider.ts     # {git_branch} variable
│   │   └── command-provider.ts # {cmd:name} variables
│   └── osc-controller.ts       # OSC layer content management
├── styles/
│   └── status-bar.css          # Status bar styles
```

## Test Scenarios

### Unit Tests
- [ ] TemplateEngine: parse template string and identify variables
- [ ] TemplateEngine: resolve variables with provider values
- [ ] TemplateEngine: handle unknown variables (render empty)
- [ ] OscController: set left/right content
- [ ] OscController: clear all, clear left, clear right
- [ ] OscController: show/hide layer
- [ ] OscController: HTML tag stripping on set content
- [ ] TimeProvider: format time with configurable format string
- [ ] GitProvider: parse branch name from git output
- [ ] GitProvider: determine dirty/clean state
- [ ] CwdProvider: extract basename from path
- [ ] Renderer: layer visibility based on content

### Integration Tests
- [ ] OSC 777;statusbar;set;left;content routes correctly through osc-handler
- [ ] Settings change triggers status bar re-render
- [ ] Status bar visibility toggles with settings

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (WebdriverIO + tauri-driver)
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] Status bar appears when enabled in settings
- [ ] Status bar hidden when disabled (default)

### Edge Cases
- [ ] All template variables empty: app layer lines are hidden
- [ ] Very long content: text truncation with ellipsis
- [ ] Custom command fails/times out: display empty or error indicator
- [ ] Rapid OSC set/clear sequences: no flickering or race conditions
- [ ] Window resize: status bar reflows correctly
- [ ] Multiple tabs: each tab has independent status bar state (cwd, git)

### Performance Tests
- [ ] Template variable polling does not increase CPU usage when terminal is idle
- [ ] Custom command execution does not block PTY data processing

## Security Considerations

- **Input Validation:** OSC content is stripped of all HTML tags before DOM insertion (XSS prevention)
- **Internal Content:** Template-resolved content supports full HTML (user-configured templates are trusted)
- **Custom Commands:** Only a single executable path is accepted (no arguments, no shell expansion). This prevents shell injection attacks. Executed via the existing Tauri shell command infrastructure
- **Data Protection:** No sensitive data is stored; all status bar content is ephemeral

## Error Handling

### Error Cases

| Scenario | Handling |
|---|---|
| Custom command execution fails | Display empty string for that variable |
| Custom command times out | Kill process, display empty string |
| Git command fails (not a git repo) | Display empty string for `{git_branch}` |
| Invalid template variable name | Render as empty string |
| Invalid OSC statusbar command | Ignore silently (log at debug level) |
| Invalid time format string | Fall back to default format |

## Performance Optimization

### Optimization Strategies
- Differential rendering: only update DOM elements whose content has changed
- Per-variable polling: each variable polls independently at its own rate
- Pause polling for inactive/background tabs
- Git status check: combine branch + dirty into single command where possible

### Caching Strategy
- Git branch/status: cache result until next poll interval
- CWD: cache until OSC 7 event or next poll
- Custom command output: cache until next interval

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance: no visible impact on terminal rendering
- [ ] Security: OSC content HTML stripping verified
- [ ] Settings UI complete with all configuration options
- [ ] Works on Linux and Windows
- [ ] Code review completed

## Open Questions

> **Note**: No unresolved requirements.

## Implementation Phases

### Phase 1: Core Infrastructure
**Goals:** Basic status bar rendering, settings, layer structure
**Deliverables:**
- StatusBarUI with renderer and layer management
- Settings fields (Rust + TypeScript)
- Settings panel "Status Bar" category
- CSS styles
- Enable/disable toggle

### Phase 2: Template Variables
**Goals:** Template engine and variable providers
**Deliverables:**
- TemplateEngine with variable parsing
- TimeProvider, CwdProvider, GitBranchProvider
- Individual refresh rate support
- Git branch color based on state

### Phase 3: OSC Protocol + Custom Commands
**Goals:** External content support and custom command execution
**Deliverables:**
- OSC 777;statusbar routing in osc-handler.ts
- OscLayerController with set/clear/show/hide
- HTML tag stripping
- CustomCmdProvider with interval-based execution

## References

- Requirements document: `doc/tasks/status-bar/要件定義書.md`
- Tab bar implementation (reference pattern): `src/tab-bar/`
- OSC handler: `src/terminal-app/osc-handler.ts`
- Settings types: `src/settings/types.ts`
- UI Design Guidelines: `doc/UI-DESIGN-GUIDELINES.yaml`
