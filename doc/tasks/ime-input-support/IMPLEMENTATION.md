# Implementation Plan: IME Input Support for Japanese Text

## Overview
This implementation adds Japanese Input Method Editor (IME) support to the eMterm terminal emulator by creating a hidden input element to capture IME composition events and forward confirmed text to the PTY, while maintaining coexistence with existing keyboard handling for special keys.

## Objectives
- Enable Japanese text input using system IME (hiragana, katakana, kanji conversion)
- Position IME candidate windows at the terminal cursor location
- Maintain typing latency under 50ms for responsive user experience
- Ensure compatibility with existing keyboard handling for special keys (Ctrl+C, arrows, etc.)
- Support long-form Japanese text input (100+ characters) without performance degradation

## Prerequisites

### Development Environment
- Node.js / Bun runtime (already installed in project)
- TypeScript compiler (via `bun run typecheck`)
- Tauri development environment (already configured)

### Dependencies
- **Internal Dependencies**:
  - `PtyClient` (src/pty/client.ts) - Used to send text to PTY
  - `TerminalState` (src/terminal/state.ts) - Used to get cursor position
  - Existing `handleKeyDown` in src/main.ts - Must coexist with IME handlers
  - `charSize` global variable in src/main.ts - Used for position calculation

- **External Dependencies**:
  - Browser/WebView IME API (native composition events)
  - TextEncoder API (UTF-8 encoding)

### Knowledge Requirements
- Understanding of DOM Composition Events (compositionstart, compositionend, input)
- Familiarity with IME behavior across platforms (Linux/Windows/macOS)
- Understanding of eMterm's existing keyboard event flow
- Knowledge of PTY client write API

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (Vanilla, no framework)
- **Runtime**: Tauri WebView (Chromium-based)
- **Backend**: Rust (Tauri PTY integration)
- **Testing**: Bun test (manual testing only, no E2E required)

### Design Approach
**Hidden Input Element Pattern:**
This implementation uses a proven pattern from terminals like Tabby, where an invisible input element captures IME events while the terminal rendering remains separate. This approach:
- Leverages native browser IME support without custom implementation
- Maintains separation between input capture and rendering
- Allows OS-level IME candidate window positioning
- Preserves existing keyboard handling for special keys

**Event Flow Architecture:**
```
User Input → OS IME → Hidden Input Element → IME Event Handlers → PTY Client → Shell
                                            ↓
                              (Special Keys: Ctrl+C, etc.)
                                            ↓
                              Existing keydown Handler → PTY Client → Shell
```

### Component Interaction
- **Hidden Input Element**: Captures IME events, positioned at cursor location
- **IME Event Handlers**: Process `input` and `compositionend` events, encode to UTF-8, send to PTY
- **Focus Manager**: Ensures hidden input receives focus when terminal is clicked
- **Position Updater**: Synchronizes hidden input position with terminal cursor
- **Existing Keydown Handler**: Continues to handle special keys (Ctrl+C, arrows), coexists with IME

**Dependency Flow:**
```
initTerminal() → createHiddenInput() → setupIMEHandlers()
                                    → setupFocusManagement()
                                    → updateIMEPosition()
```

## Implementation Phases

### Phase 1: Hidden Input Element Creation and Focus Management

**Goal**: Create the invisible input element and ensure it receives focus when the terminal is active, enabling IME to function.

**Files to Create**:
None (all modifications to existing files)

**Files to Modify**:
- `src/main.ts`:
  - Add global variable `imeInput: HTMLInputElement | null = null`
  - Modify `initTerminal()` to create hidden input element
  - Modify terminal click handler to focus hidden input
  - Modify `cleanup()` to remove hidden input element

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| imeInput (global variable) | Hold reference to hidden input element | initTerminal() called | Element accessible for focus/positioning |
| Hidden Input Creation | Create invisible input element with IME-enabling styles | DOM ready | Input element appended to document.body |
| Focus Manager | Transfer focus to hidden input when terminal clicked | Hidden input exists | IME can receive composition events |

**Processing Flow**:
```
1. initTerminal() executes
   └─ Create input element with type="text"
   └─ Apply invisible styles (opacity: 0, position: fixed, 1px×1px)
   └─ Set attributes (autocomplete="off", pointer-events: none, z-index: -1)
   └─ Append to document.body
   └─ Store reference in global imeInput variable

2. Terminal click event occurs
   └─ Check if imeInput exists
   └─ Call imeInput.focus()
   └─ IME now ready to receive input

3. cleanup() executes
   └─ Check if imeInput exists and has parent
   └─ Remove from DOM
   └─ Set imeInput = null
```

**Implementation Steps**:

1. **Add Global Variable**
   - Add `let imeInput: HTMLInputElement | null = null;` at top of main.ts with other global state
   - Place after `charSize` declaration for logical grouping

2. **Create Hidden Input in initTerminal()**
   - After terminal element is found and validated
   - Before PTY client creation
   - Use `document.createElement("input")` to create element
   - Set `type="text"` to enable IME
   - Set `autocomplete="off"` to prevent autofill interference
   - Apply CSS via `style.cssText` for atomic update:
     ```typescript
     imeInput.style.cssText = `
       position: fixed;
       opacity: 0.01;        /* 完全な0ではなく微小な不透明度 */
       width: 2px;           /* 1pxではなく2px */
       height: 2px;
       pointer-events: none;
       z-index: -1;
       color: transparent;
       background: transparent;
       border: none;
       outline: none;
     `;
     ```
     - `position: fixed` - allows absolute positioning
     - `opacity: 0.01` - nearly invisible but functional (some platforms require non-zero opacity)
     - `width: 2px; height: 2px` - minimal size (2px for better platform compatibility)
     - `pointer-events: none` - prevent user interaction
     - `z-index: -1` - behind all other elements
     - `color/background: transparent` - ensure no visual artifacts
     - `border/outline: none` - remove default input styling
   - Append to `document.body`
   - Store reference in `imeInput`

3. **Modify Click Handler**
   - Locate existing `terminal.addEventListener("click", ...)` in initTerminal()
   - Replace current focus logic with:
     - Check if `imeInput` exists
     - Call `imeInput.focus()` instead of `terminal.focus()`
   - Keep `terminal.tabIndex = 0` for accessibility

4. **Add Cleanup**
   - In `cleanup()` function, after mouse event listener cleanup
   - Before `ptyClient.dispose()`
   - Check if `imeInput` exists and has `parentNode`
   - Remove from DOM using `imeInput.parentNode.removeChild(imeInput)`
   - Set `imeInput = null`

**Dependencies**:
- Requires: DOM ready, terminal element exists
- Blocks: Phase 2 (IME event handlers need input element to exist)

**Testing Approach**:

*Manual Testing*:
- [ ] Hidden input element is created on startup
- [ ] Hidden input is not visible to the user (opacity: 0.01, 2px×2px)
- [ ] Clicking terminal focuses the hidden input (verify with browser DevTools)
- [ ] Hidden input is removed on cleanup

**Acceptance Criteria**:
- [ ] Hidden input element exists after initTerminal() completes
- [ ] Element has correct styles (opacity: 0.01, 2px×2px, position: fixed)
- [ ] Clicking terminal focuses hidden input (verify via `document.activeElement`)
- [ ] Element is removed on cleanup (no memory leak)

**Estimated Effort**: 小 (1-2 hours)

**Risks and Mitigation**:
- **Risk**: Hidden input might interfere with existing focus management
  - **Mitigation**: Use `pointer-events: none` to prevent accidental interaction, keep terminal.tabIndex for accessibility

---

### Phase 2: IME Event Handlers and PTY Integration

**Goal**: Capture confirmed text from IME composition events and send it to the PTY as UTF-8 bytes, including support for Enter key confirmation.

**Files to Create**:
None (all modifications to existing files)

**Files to Modify**:
- `src/main.ts`:
  - Add `setupIMEHandlers(input: HTMLInputElement)` function
  - Call `setupIMEHandlers(imeInput)` from `initTerminal()` after hidden input creation

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| setupIMEHandlers | Attach event listeners to hidden input | Hidden input created | IME events captured |
| input Event Handler | Process non-composing input events, extract text, send to PTY | event.isComposing === false | Text sent to PTY, input cleared |
| compositionend Event Handler | Fallback handler for composition completion | Composition active | Text sent to PTY, input cleared |
| keydown Event Handler (on input) | Detect Enter key presses during composition | IME active | enterPressed flag set |
| Text Encoding | Convert string to UTF-8 bytes | Valid string input | Uint8Array ready for PTY |
| PTY Write | Send bytes to shell via ptyClient | ptyClient exists | Text appears in shell |

**Processing Flow**:
```
1. User activates IME and types "nihongo" (にほんご)
   └─ compositionstart fires (can be tracked optionally)
   └─ compositionupdate fires multiple times (ignored)

2. User presses Space to convert
   └─ IME shows candidates "日本語", "にほんご", etc.

3. User presses Enter to confirm
   └─ keydown event: key === "Enter" → set enterPressed = true
   └─ compositionend event fires
   └─ input event fires with isComposing === false

4. input Event Handler processes
   └─ Check event.isComposing === false → proceed
   └─ Read input.value → "日本語"
   └─ Encode to UTF-8 bytes using TextEncoder
   └─ Send to PTY via ptyClient.write(bytes)
   └─ If enterPressed === true → also send 0x0D (CR)
   └─ Clear input.value
   └─ Reset enterPressed = false

5. PTY receives bytes
   └─ Shell displays "日本語"
```

**Implementation Steps**:

1. **Create setupIMEHandlers Function**
   - Create new function after `setupMouseHandlers()`
   - Signature: `function setupIMEHandlers(input: HTMLInputElement): void`
   - Initialize local variables:
     - `let enterPressed = false;` to track Enter key state
     - `let lastSentValue = "";` to track last sent value for duplicate detection
     - `let lastSentTimestamp = 0;` to track last send time for duplicate detection

2. **Add keydown Event Listener (for Enter Detection)**
   - Listen to `keydown` event on input element
   - Check if `event.key === "Enter"`
   - Set `enterPressed = true` when Enter is detected
   - Purpose: Detect Enter key confirmation to send both text + newline

3. **Add input Event Listener (Primary Handler)**
   - Listen to `input` event on input element
   - Early return if `event.isComposing === true` (composition still in progress)
   - Read confirmed text from `input.value` (not `event.data` - more reliable)
   - Early return if value is empty string
   - **Duplicate detection**: Skip if same value was sent within 100ms
     ```typescript
     const now = Date.now();
     if (value === lastSentValue && now - lastSentTimestamp < 100) {
       input.value = "";
       return; // Skip duplicate
     }
     ```
   - Encode text using `new TextEncoder().encode(value)` → UTF-8 bytes
   - Wrap PTY write in try-catch:
     - Check if `ptyClient` exists
     - Call `await ptyClient.write(bytes)`
     - If `enterPressed === true`, also send `new Uint8Array([0x0d])` (CR)
     - Update duplicate tracking: `lastSentValue = value; lastSentTimestamp = now;`
     - Log error if write fails, do not throw
   - In finally block:
     - Clear `input.value = ""` (security: prevent data lingering)
     - Reset `enterPressed = false`

4. **Add compositionend Event Listener (Fallback)**
   - Listen to `compositionend` event
   - Read text from `input.value`
   - **Apply same duplicate detection** as input handler:
     ```typescript
     const now = Date.now();
     if (value === lastSentValue && now - lastSentTimestamp < 100) {
       input.value = "";
       return; // Skip duplicate
     }
     ```
   - Same processing as input handler (encoding, PTY write, Enter handling)
   - Rationale: Provides redundancy in case input event doesn't fire on some platforms

5. **Add compositionstart Event Listener (Flag Reset)**
   - Listen to `compositionstart` event
   - Reset `enterPressed = false` to ensure clean state at composition start
   - Rationale: Prevents stale flag from previous composition

6. **Add compositioncancel Event Listener (Cleanup)**
   - Listen to `compositioncancel` event (if browser supports it)
   - Reset `enterPressed = false`
   - Clear `input.value = ""`
   - Rationale: Ensures clean state when composition is cancelled

7. **Call setupIMEHandlers from initTerminal()**
   - After creating hidden input element
   - Before PTY spawn
   - Pass `imeInput` reference: `setupIMEHandlers(imeInput);`

**Dependencies**:
- Requires: Phase 1 (hidden input element must exist)
- Blocks: None (can be tested independently)

**Testing Approach**:

*Manual Testing*:
- [ ] Type "nihongo" (にほんご) and convert to "日本語" - confirmed text appears in shell
- [ ] Type katakana conversion (F7 key) - "ニホンゴ" appears correctly
- [ ] Press Enter to confirm - both confirmed text AND newline are sent
- [ ] Type 100+ characters - all characters appear without lag
- [ ] Confirm empty input (just press Enter) - only newline sent, no error

**Acceptance Criteria**:
- [ ] Confirmed Japanese text is sent to PTY correctly
- [ ] UTF-8 encoding is correct (no garbled characters)
- [ ] Enter key sends both confirmed text + CR (0x0D)
- [ ] input.value is cleared after each confirmation
- [ ] PTY write errors are logged but don't crash application
- [ ] Typing latency is under 50ms (measure from event to write completion)

**Estimated Effort**: 中 (4-6 hours)

**Risks and Mitigation**:
- **Risk**: Platform differences in event firing order (input vs compositionend)
  - **Mitigation**: Implement both handlers for redundancy, use input.value instead of event.data
- **Risk**: Enter key might not be detected correctly
  - **Mitigation**: Test on multiple platforms, use keydown event for reliable detection

---

### Phase 3: Cursor Position Synchronization for IME Candidate Window

**Goal**: Position the hidden input element at the terminal cursor location to ensure the OS IME candidate window appears near the cursor.

**Files to Create**:
None (all modifications to existing files)

**Files to Modify**:
- `src/main.ts`:
  - Add `updateIMEPosition()` function
  - Call `updateIMEPosition()` from appropriate locations (terminal state updates, resize)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| updateIMEPosition | Calculate and update hidden input element position | terminalState and imeInput exist | Input positioned at cursor |
| Cursor Position Reader | Get current cursor row/col from terminalState | terminalState initialized | Cursor coordinates available |
| Pixel Coordinate Converter | Convert row/col to pixel coordinates | charSize available | X/Y pixel values calculated |
| Position Applier | Set input element's left/top styles | Terminal container exists | Input element repositioned |
| Bottom Row Detector | Check if cursor is on last row | Cursor row known | Position adjusted if needed |

**Processing Flow**:
```
1. updateIMEPosition() is called
   └─ Validate imeInput and terminalState exist
   └─ Get cursor position (row, col) from terminalState.getCursor()
   └─ Get terminal rows from terminalState.getRows()

2. Calculate pixel coordinates
   └─ x = cursor.col * charSize.width
   └─ y = cursor.row * charSize.height

3. Get terminal container position
   └─ Find terminal element by ID
   └─ Get bounding rect: rect = terminal.getBoundingClientRect()

4. Check if cursor is on bottom row
   ├─ If cursor.row === rows - 1 (bottom row)
   │  └─ Position input ABOVE cursor: top = rect.top + y - charSize.height
   └─ Else
      └─ Position input BELOW cursor: top = rect.top + y + charSize.height

5. Apply position
   └─ Set imeInput.style.left = rect.left + x (px)
   └─ Set imeInput.style.top = calculated top value (px)
```

**Implementation Steps**:

1. **Create updateIMEPosition Function**
   - Create new function after `setupIMEHandlers()`
   - Signature: `function updateIMEPosition(): void`
   - Early return if `!imeInput || !terminalState` (defensive check)

2. **Get Cursor and Terminal Information**
   - Call `terminalState.getCursor()` to get current cursor position
   - Call `terminalState.getRows()` to get terminal row count
   - Get terminal container element: `document.getElementById("terminal")`
   - Early return if terminal element not found
   - Get container's bounding rect: `rect = terminal.getBoundingClientRect()`
   - Get computed styles for accurate padding:
     ```typescript
     const styles = getComputedStyle(terminal);
     const paddingLeft = parseFloat(styles.paddingLeft) || 0;
     const paddingTop = parseFloat(styles.paddingTop) || 0;
     ```
   - Get scroll offset (if available):
     ```typescript
     const scrollOffset = terminalState.getScrollOffset?.() ?? 0;
     ```

3. **Calculate Pixel Position**
   - Calculate x-coordinate: `cursor.col * charSize.width + paddingLeft`
   - Calculate y-coordinate: `cursor.row * charSize.height + paddingTop - scrollOffset`
   - Container position is obtained from `rect.left` and `rect.top`

4. **Determine Vertical Position (Handle Bottom Row)**
   - Calculate y with padding and scroll: `const y = cursor.row * charSize.height + paddingTop - scrollOffset;`
   - Check if `cursor.row === rows - 1`
   - If true: position above cursor → `rect.top + y - charSize.height`
   - If false: position below cursor → `rect.top + y + charSize.height`
   - Rationale: Prevents candidate window from appearing off-screen at bottom

5. **Apply Position to Input Element**
   - Calculate x with padding: `const x = cursor.col * charSize.width + paddingLeft;`
   - Set `imeInput.style.left = ${rect.left + x}px`
   - Set `imeInput.style.top = ${calculatedTop}px`
   - Position is absolute (already set to `position: fixed`)

6. **Add Call Sites**
   - In `setupNewTerminalHandlers()`, after `terminalRenderer.scheduleRender(terminalState)`
     - Call `updateIMEPosition()` to sync position after terminal updates
   - In resize observer callback (inside `observeContainerResize`), after `terminalRenderer.forceRender(terminalState)`
     - Call `updateIMEPosition()` to sync position after terminal resize

**Dependencies**:
- Requires: Phase 1 (hidden input element must exist)
- Requires: terminalState to be initialized and updated
- Blocks: None (position sync is an enhancement, not blocking)

**Testing Approach**:

*Manual Testing*:
- [ ] Type Japanese at various cursor positions - candidate window appears near cursor
- [ ] Type Japanese when cursor is on bottom row - candidate window appears ABOVE cursor
- [ ] Resize terminal window - candidate window position updates correctly
- [ ] Move cursor with arrow keys, then type Japanese - candidate window follows cursor

**Acceptance Criteria**:
- [ ] Candidate window appears below cursor by default
- [ ] Candidate window appears above cursor when on bottom row
- [ ] Position updates correctly after terminal state changes
- [ ] Position updates correctly after terminal resize
- [ ] No visual flicker or lag when position updates

**Estimated Effort**: 中 (3-4 hours)

**Risks and Mitigation**:
- **Risk**: charSize might not be accurate for all fonts
  - **Mitigation**: Use existing `measureCharacterSize()` which already handles font metrics
- **Risk**: OS IME might not respect input element position on all platforms
  - **Mitigation**: Best-effort approach, document platform-specific behavior

---

### Phase 4: Coexistence with Existing Keyboard Handler

**Goal**: Ensure IME input and existing keyboard shortcuts (Ctrl+C, arrows, etc.) coexist without interference.

**Files to Create**:
None (all modifications to existing files)

**Files to Modify**:
- `src/main.ts`:
  - Modify `handleKeyDown()` to skip IME input characters while allowing special keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| isSpecialKey | Determine if key event is a special key | KeyboardEvent exists | Boolean indicating if key should be handled by keydown |
| Modified handleKeyDown | Skip regular keys during IME focus, allow special keys | ptyClient exists | Special keys processed, regular keys handled by IME |

**Processing Flow**:
```
1. Key event occurs
   └─ handleKeyDown() receives event

2. Existing checks
   └─ Check if ptyClient exists
   └─ Check shouldHandleKey(event)
   └─ Check event.isComposing (already exists, skip if true)

3. NEW: Check if IME input has focus
   ├─ If document.activeElement === imeInput
   │  └─ Check if key is special key (Ctrl+, Arrow, Escape, Tab)
   │     ├─ If special key → continue processing (allow special keys)
   │     └─ If not special key → return (let IME handler deal with it)
   └─ Else → continue processing (normal keyboard handling)

4. Process key event
   └─ Convert to bytes via keyEventToBytes()
   └─ Send to PTY
```

**Implementation Steps**:

1. **Create isSpecialKey Helper Function**
   - Create new function before `handleKeyDown()`
   - Signature: `function isSpecialKey(event: KeyboardEvent): boolean`
   - Implementation:
     ```typescript
     function isSpecialKey(event: KeyboardEvent): boolean {
       // Ctrl/Alt/Meta combinations (always special)
       if (event.ctrlKey || event.altKey || event.metaKey) {
         return true;
       }

       // Navigation keys
       if (event.key.startsWith("Arrow") ||
           event.key === "Home" || event.key === "End" ||
           event.key === "PageUp" || event.key === "PageDown") {
         return true;
       }

       // Editing keys
       if (event.key === "Backspace" || event.key === "Delete") {
         return true;
       }

       // Function keys
       if (event.key.startsWith("F") && /^F\d+$/.test(event.key)) {
         return true;
       }

       // Other special keys
       if (event.key === "Escape" || event.key === "Tab" || event.key === "Insert") {
         return true;
       }

       return false;
     }
     ```
   - Rationale: These keys should bypass IME and go directly to PTY

2. **Modify handleKeyDown Function**
   - After existing `event.isComposing` check (around line 332)
   - Add new check:
     ```
     if (document.activeElement === imeInput) {
       if (!isSpecialKey(event)) {
         return; // Let IME handler process
       }
       // Special keys fall through and are processed normally
     }
     ```
   - Rationale: Prevents double-processing of regular characters while allowing special keys

**Dependencies**:
- Requires: Phase 1 (imeInput must exist)
- Requires: Phase 2 (IME handlers must be set up)
- Blocks: None (final integration step)

**Testing Approach**:

*Manual Testing*:
- [ ] Type regular English characters - sent to PTY (no IME interference)
- [ ] Type Japanese characters - processed by IME handlers
- [ ] Press Ctrl+C during IME composition - interrupt signal sent (special key works)
- [ ] Press Arrow keys during IME composition - cursor moves (special keys work)
- [ ] Press Escape during IME composition - cancels composition (if applicable)
- [ ] Verify no double-character input for regular typing

**Acceptance Criteria**:
- [ ] Special keys (Ctrl+C, arrows, Escape, Tab) work during IME focus
- [ ] Regular character keys are handled by IME when IME has focus
- [ ] No double-processing of any characters
- [ ] Existing keyboard functionality remains unchanged when IME is not active

**Estimated Effort**: 小 (2-3 hours)

**Risks and Mitigation**:
- **Risk**: Some special key combinations might conflict with IME
  - **Mitigation**: Test extensively with common IME shortcuts, adjust isSpecialKey() as needed

---

## Complete File Structure

```
src/
├── main.ts                      # MODIFIED: All IME functionality added here
│                                # - Global variable: imeInput
│                                # - initTerminal(): Create hidden input, focus management
│                                # - setupIMEHandlers(): IME event handling
│                                # - updateIMEPosition(): Cursor position sync
│                                # - isSpecialKey(): Special key detection
│                                # - handleKeyDown(): Modified for IME coexistence
│                                # - cleanup(): Remove hidden input
├── pty/
│   ├── client.ts                # NO CHANGES - existing write() API used
│   └── keyboard.ts              # NO CHANGES - existing keyEventToBytes() used
├── terminal/
│   ├── state.ts                 # NO CHANGES - existing getCursor(), getRows() used
│   └── renderer.ts              # NO CHANGES - existing rendering used
└── types/
    └── terminal.ts              # NO CHANGES
```

**File Responsibilities**:
- **src/main.ts**: Entry point, terminal initialization, IME integration, event handling
- **src/pty/client.ts**: PTY communication (unchanged, used for write operations)
- **src/terminal/state.ts**: Terminal state management (unchanged, used for cursor position)

**Key Modification Areas in main.ts**:
1. **Global State** (top of file): Add `imeInput` variable
2. **initTerminal()** (initialization section): Create hidden input, focus management, call setupIMEHandlers
3. **setupIMEHandlers()** (new function): IME event listeners, text encoding, PTY write
4. **updateIMEPosition()** (new function): Calculate and apply input element position
5. **isSpecialKey()** (new function): Detect special keys for bypass logic
6. **handleKeyDown()** (existing function): Add IME focus check with special key bypass
7. **cleanup()** (cleanup section): Remove hidden input element

## Testing Strategy

### Unit Testing

**Approach**:
Since this feature primarily involves DOM event handling and browser IME integration, automated unit tests are not applicable. Manual testing is the primary validation method as specified in requirements.

**Test Coverage Goals**:
Manual test coverage: 100% of user scenarios and edge cases

**Key Test Areas**:
1. **Hidden Input Element** (Phase 1)
   - Creation and styling verification
   - Focus management
   - Cleanup verification

2. **IME Event Handling** (Phase 2)
   - Japanese text confirmation and PTY write
   - UTF-8 encoding correctness
   - Enter key handling (text + newline)
   - Empty confirmation handling
   - Error handling (PTY write failure)

3. **Cursor Position Sync** (Phase 3)
   - Candidate window position (normal rows)
   - Candidate window position (bottom row)
   - Position updates on terminal changes
   - Position updates on resize

4. **Keyboard Coexistence** (Phase 4)
   - Special keys during IME (Ctrl+C, arrows)
   - Regular keys without IME
   - No double-processing

### Manual Testing Checklist

Based on SPEC.md test scenarios:

**Basic Functionality**:
- [ ] **TS1**: Type "nihongo" → Space → Enter → "日本語" appears in terminal
- [ ] **TS2**: Type "nihongo" → F7 → Confirm → "ニホンゴ" appears (katakana)
- [ ] **TS3**: Candidate window appears below cursor at various positions
- [ ] **TS4**: Input 100+ characters of Japanese text without lag
- [ ] **TS5**: Press Enter after conversion → text + newline sent
- [ ] **TS6**: Start composition → lose focus → regain focus → continue composition
- [ ] **TS7**: Press Ctrl+C during composition → interrupt signal sent
- [ ] **TS8**: Activate IME → press Enter without typing → only newline sent

**Edge Cases**:
- [ ] **EC1**: PTY session not started → input ignored, no crash
- [ ] **EC2**: Very rapid typing (stress test) → all characters captured
- [ ] **EC3**: Switch English/Japanese rapidly → no lost characters
- [ ] **EC4**: Terminal resize during composition → position updates correctly
- [ ] **EC5**: Multiple sequential confirmations → each handled independently

**Platform Testing**:
- [ ] Linux with iBus → all scenarios pass
- [ ] Linux with Fcitx → all scenarios pass
- [ ] Windows with MS-IME → all scenarios pass
- [ ] Windows with Google Japanese Input → all scenarios pass
- [ ] macOS (best effort) → basic scenarios pass

**Performance Testing**:
- [ ] **PT1**: Measure input latency (event to PTY write) < 50ms (average of 100 inputs)
- [ ] **PT2**: Input 500 characters → memory increase < 10MB, no visible lag

**Security Testing**:
- [ ] **ST1**: After confirmation, `input.value` is cleared (inspect with DevTools)
- [ ] **ST2**: Text is encoded as UTF-8, never interpreted as HTML

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| None | - | Uses browser native APIs | - |

**Note**: This feature uses only browser-native APIs (DOM, Composition Events, TextEncoder) and existing project dependencies.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Hidden Input Creation → No dependencies
2. Phase 2: IME Event Handlers → Depends on Phase 1
3. Phase 3: Cursor Position Sync → Depends on Phase 1, can run parallel to Phase 2
4. Phase 4: Keyboard Coexistence → Depends on Phase 1 & 2

**Component Dependencies**:
- `setupIMEHandlers()` depends on hidden input element existing
- `updateIMEPosition()` depends on `terminalState` and `charSize` being initialized
- `handleKeyDown()` modification depends on `imeInput` and `isSpecialKey()` existing

## Risk Assessment

### Technical Risks

1. **Platform-Specific IME Behavior**
   - **Risk**: Different IMEs (iBus, Fcitx, MS-IME, macOS) may fire events in different orders
   - **Likelihood**: Medium
   - **Impact**: High (incorrect text capture)
   - **Mitigation**:
     - Implement both `input` and `compositionend` handlers for redundancy
     - Use `input.value` instead of `event.data` for reliability
     - Test on all major platforms and IMEs
     - Document known platform-specific quirks

2. **Candidate Window Positioning Accuracy**
   - **Risk**: OS IME may not respect input element position on some platforms (especially macOS)
   - **Likelihood**: Medium
   - **Impact**: Low (usability affected but not functionality)
   - **Mitigation**:
     - Best-effort approach with row-level accuracy
     - macOS marked as "best effort" in requirements
     - Use existing `charSize` calculation for consistency

3. **Performance with Long Text Input**
   - **Risk**: 100+ character input might cause lag or memory issues
   - **Likelihood**: Low
   - **Impact**: Medium (poor UX)
   - **Mitigation**:
     - Clear `input.value` immediately after processing
     - Use efficient UTF-8 encoding (TextEncoder is optimized)
     - Test with 500+ character stress test
     - Profile memory usage during testing

### Implementation Risks

1. **Interference with Existing Keyboard Handling**
   - **Risk**: IME focus might break existing special key shortcuts
   - **Likelihood**: Medium
   - **Impact**: High (loss of terminal functionality)
   - **Mitigation**:
     - Implement `isSpecialKey()` bypass logic in Phase 4
     - Preserve existing `event.isComposing` check
     - Comprehensive manual testing of all keyboard shortcuts

2. **Race Conditions in Event Handling**
   - **Risk**: `input` and `compositionend` events might fire out of order
   - **Likelihood**: Low
   - **Impact**: Medium (duplicate or missing characters)
   - **Mitigation**:
     - Both handlers clear `input.value` after processing
     - Use `enterPressed` flag to track state across events
     - Test rapid input sequences

## Performance Considerations

1. **Input Processing Latency**
   - Use async/await for PTY write but don't await unnecessarily
   - `TextEncoder.encode()` is synchronous and fast
   - Target: < 50ms from `input` event to `ptyClient.write()` completion
   - Measure: Instrument with `console.time()` during testing

2. **Memory Management**
   - Clear `input.value` immediately after processing (prevents lingering data)
   - Hidden input element adds < 1KB memory overhead
   - No caching or state accumulation (stateless design)

3. **Rendering Performance**
   - `updateIMEPosition()` only updates 2 CSS properties (left, top)
   - Position updates triggered only on terminal state changes, not every frame
   - No impact on existing terminal rendering pipeline

4. **Event Handler Efficiency**
   - Early returns in event handlers minimize processing
   - No complex computations in hot path (just encoding and PTY write)
   - Error handling uses try-catch without throwing (non-blocking)

## Security Considerations

1. **Input Value Clearing**
   - `input.value` is cleared immediately after sending to PTY
   - Prevents sensitive data (passwords, tokens) from lingering in DOM
   - Cleared in `finally` block to ensure cleanup even on error

2. **XSS Prevention**
   - Text is encoded directly to UTF-8 bytes via `TextEncoder`
   - Never interpreted as HTML or JavaScript
   - Sent directly to PTY as binary data
   - No use of `innerHTML`, `eval`, or other risky APIs

3. **Focus Hijacking Prevention**
   - Hidden input has `pointer-events: none` to prevent accidental user interaction
   - `z-index: -1` keeps it behind all other elements
   - Only focused via explicit click on terminal area

4. **Data Validation**
   - No special validation needed - all UTF-8 text is valid terminal input
   - Empty strings are handled gracefully (early return, no error)

## Open Questions

None - all clarifications were obtained during requirements gathering phase.

## Future Enhancements

Items deferred to later phases or releases:

### Not in Current Spec:
- Multi-language IME support (Chinese, Korean) - requires validation with other IME systems
- Custom IME candidate window rendering - would require complex platform-specific code
- IME mode indicator UI - visual feedback for IME state
- Configurable IME behavior (preferences for candidate position, etc.)

## Success Metrics

### Functional Completeness
- [ ] All functional requirements (FR1-FR7) implemented
- [ ] All manual test scenarios pass
- [ ] Error handling works correctly (PTY write failures logged, no crash)

### Quality Metrics
- [ ] Manual test coverage: 100% of scenarios
- [ ] No critical bugs in manual testing
- [ ] Code follows TypeScript best practices
- [ ] Type checking passes (`bun run typecheck`)

### Performance Metrics
- [ ] Input latency < 50ms (measure with instrumentation)
- [ ] 500+ character input without lag
- [ ] Memory overhead < 1KB

### Platform Support
- [ ] Linux (iBus, Fcitx) - 100% pass rate
- [ ] Windows (MS-IME, Google Japanese Input) - 100% pass rate
- [ ] macOS - Best effort (basic scenarios work)

### User Experience
- [ ] Candidate window appears near cursor
- [ ] No visual artifacts or flicker
- [ ] Existing keyboard shortcuts work (Ctrl+C, arrows, etc.)
- [ ] Clear error messages in console if issues occur

## References

- **Requirements Document**: `doc/tasks/ime-input-support/要件定義書.md`
- **Technical Specification**: `doc/tasks/ime-input-support/SPEC.md`
- **Tabby Terminal IME Implementation**: https://github.com/Eugeny/tabby (hidden textarea approach)
- **MDN Composition Events**: https://developer.mozilla.org/en-US/docs/Web/API/CompositionEvent
- **MDN InputEvent**: https://developer.mozilla.org/en-US/docs/Web/API/InputEvent
- **TextEncoder API**: https://developer.mozilla.org/en-US/docs/Web/API/TextEncoder
- **Current Keyboard Handler**: `src/pty/keyboard.ts`
- **PTY Client**: `src/pty/client.ts`
- **Terminal State**: `src/terminal/state.ts`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Verify all phases align with SPEC.md requirements
   - Confirm testing approach (manual testing only)
   - Address any questions about implementation approach

2. **Environment Setup**
   - Ensure Tauri development environment is ready
   - Verify `bun test` and `bun run typecheck` work
   - Setup test environments (Linux, Windows, macOS if available)

3. **Begin Implementation**
   - Start with Phase 1 (hidden input creation)
   - Test each phase independently before moving to next
   - Commit incrementally with clear messages

4. **Testing and Validation**
   - Follow manual testing checklist for each phase
   - Test on multiple platforms and IMEs
   - Measure performance metrics
   - Document any platform-specific issues

5. **Code Review and Documentation**
   - Review code changes
   - Update documentation if needed
   - Prepare for release
