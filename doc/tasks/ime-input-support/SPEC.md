# Feature: IME Input Support for Japanese Text

## Overview

This feature enables Japanese Input Method Editor (IME) support in the eMterm terminal emulator. Currently, eMterm handles keyboard events directly via `keydown` listeners, which prevents IME composition from being properly captured. This implementation uses a dual-mode approach: **EditContext API** (Chromium/WebView2) as the primary method, with a **hidden textarea fallback** (WebKit) to receive IME events and properly forward composed text to the PTY.

Additionally, this implementation includes:
- **Composition View**: Visual overlay showing composition text in real-time
- **SKK IME Support**: Special handling for Emacs-style IME with marker detection

## Objectives

- Enable Japanese text input using system IME (hiragana, katakana, kanji conversion)
- Position IME candidate windows at the terminal cursor location
- Maintain typing latency under 50ms for responsive user experience
- Support long-form Japanese text input (100+ characters)
- Ensure compatibility with existing keyboard handling for special keys
- Preserve IME state across focus changes

## User Stories

### US1: Basic Japanese Text Input
As a Japanese developer, I want to type Japanese text in the terminal using my system IME, so that I can write commit messages and documentation in Japanese.

**Acceptance Criteria:**
- [ ] Can activate IME and type hiragana characters
- [ ] Can convert hiragana to kanji using space/enter keys
- [ ] Converted text is sent to the PTY and displayed in the terminal
- [ ] Typing latency is under 50ms

### US2: IME Candidate Window Positioning
As a terminal user, I want the IME candidate window to appear near my cursor, so that I can easily see and select conversion candidates without visual confusion.

**Acceptance Criteria:**
- [ ] Candidate window appears below the cursor line by default
- [ ] When cursor is on the bottom line, candidate window appears above the cursor
- [ ] Position updates correctly as cursor moves

### US3: Long-Form Japanese Input
As a user writing lengthy Japanese text, I want to input 100+ characters smoothly, so that I can compose long commit messages or documentation without performance degradation.

**Acceptance Criteria:**
- [ ] Can input 100+ characters without lag
- [ ] Memory usage remains stable
- [ ] No visible performance degradation

### US4: Preserve IME State Across Focus Changes
As a multitasking user, I want my IME conversion state preserved when I switch windows, so that I don't lose my input when I temporarily switch focus.

**Acceptance Criteria:**
- [ ] IME composition state is preserved when focus is lost
- [ ] Composition can continue when focus returns
- [ ] No text is lost during focus changes

### US5: Enter Key Confirms Text
As a terminal user, I want pressing Enter after IME conversion to confirm the text, so that I can input Japanese commands efficiently.

**Acceptance Criteria:**
- [ ] Enter key confirms IME composition and sends text to PTY
- [ ] Standard IME handles Enter internally (no separate CR sending required)
- [ ] No double newline occurs after confirmation

## Technical Requirements

### Functional Requirements
- **FR1:** Use EditContext API (Chromium/WebView2) as primary IME input method, with hidden textarea as fallback (WebKit)
- **FR2:** Listen to `input` and `compositionend` events to capture confirmed text
- **FR3:** Encode confirmed text as UTF-8 bytes and send to PTY via `ptyClient.write()`
- **FR4:** Focus the hidden textarea element when terminal area is clicked (fallback mode)
- **FR5:** Display Composition View overlay at cursor position showing composition text in real-time
- **FR6:** Handle composition confirmation correctly (no separate CR sending to prevent double newlines)
- **FR7:** Coexist with existing `keydown` handler for special keys (Ctrl+C, arrows, etc.)
- **FR8:** Detect and handle SKK IME markers (▽, ▼, 【】) for Emacs-style IME support

### Non-Functional Requirements
- **NFR1 - Performance:** Typing latency must be under 50ms from input event to PTY write
- **NFR2 - Performance:** Support 100+ character input without performance degradation
- **NFR3 - Security:** Clear `input.value` immediately after sending to prevent data leakage
- **NFR4 - Compatibility:** Linux: Fcitx recommended (other IMEs are unsupported)
- **NFR5 - Compatibility:** Windows: MS-IME, Google Japanese Input
- **NFR6 - Compatibility:** macOS: Best-effort support
- **NFR7 - Reliability:** IME failures should not crash the application or affect existing keyboard input
- **NFR8 - Usability:** Hidden textarea must be invisible; Composition View provides visual feedback

## Implementation Approach

### Architecture

**System Architecture (Dual-Mode):**
```
┌─────────────────────────────────────┐
│      User (OS IME Active)           │
├─────────────────────────────────────┤
│  ┌─────────────┐  ┌───────────────┐ │
│  │ EditContext │  │ Hidden        │ │  ← Primary (Chromium) / Fallback (WebKit)
│  │ API         │  │ Textarea      │ │
│  └─────────────┘  └───────────────┘ │
├─────────────────────────────────────┤
│   Composition View (Overlay)        │  ← Visual feedback at cursor
├─────────────────────────────────────┤
│   IME Event Handlers (TypeScript)   │  ← Processes input/compositionend
├─────────────────────────────────────┤
│   PTY Client (ptyClient.write)      │  ← Sends UTF-8 bytes
├─────────────────────────────────────┤
│   Tauri Backend (Rust)              │
├─────────────────────────────────────┤
│         Shell Process               │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
┌──────────────────┐
│  Terminal DOM    │──click──▶ Focus Manager
│   Container      │
└──────────────────┘

┌──────────────────┐    ┌──────────────────┐
│ EditContext API  │ OR │ Hidden Textarea  │  (auto-selected based on platform)
│ (Chromium)       │    │ (WebKit fallback)│
└──────────────────┘    └──────────────────┘
         │                       │
         └───────────┬───────────┘
                     ▼
         ┌──────────────────┐
         │ IME Event Handler│──▶ PTY Client
         └──────────────────┘
                     │
                     ▼
         ┌──────────────────┐
         │ Composition View │  (overlay at cursor position)
         └──────────────────┘

┌──────────────────┐
│ Cursor Position  │──update─▶ Position Updater ──▶ Composition View Position
│   Tracker        │
└──────────────────┘

┌──────────────────┐
│ Existing Keydown │──special keys──▶ PTY Client
│    Handler       │
└──────────────────┘
```

### Data Flow

**Normal IME Input Flow:**
```
User types "nihongo" → OS IME starts composition
                     → Composition View shows "にほんご" at cursor
                     → User presses Space
                     → IME shows candidates "日本語", "にほんご", etc.
                     → User selects "日本語" and presses Enter
                     → compositionend event fires
                     → input.value contains "日本語"
                     → IME Event Handler reads value
                     → TextEncoder.encode("日本語") → UTF-8 bytes
                     → ptyClient.write(bytes)
                     → input.value cleared, Composition View hidden
                     → Shell receives "日本語"
                     → Terminal displays "日本語"
```

**Enter Key Confirmation Flow:**
```
User confirms with Enter → compositionend fires with confirmed text
                        → input event fires (isComposing === false)
                        → Send confirmed text only (no separate CR)
                        → input.value cleared
                        → Standard IME handles Enter internally
```

**SKK IME Flow:**
```
User types "Nihongo" → SKK shows "▽にほんご" (未確定)
                     → Composition View displays "▽にほんご"
                     → hasSKKMarker() detects ▽ → keep composing
                     → User confirms → SKK shows "日本語"
                     → No SKK markers → send to PTY
```

### API Design

This feature does not introduce new public APIs. It uses existing APIs:

#### PTY Client API (Existing)

**Method: `ptyClient.write()`**

**Request:**
```typescript
async write(data: Uint8Array | string): Promise<void>
```

**Usage:**
```typescript
const confirmedText = "日本語";
const bytes = new TextEncoder().encode(confirmedText);
await ptyClient.write(bytes);
```

**Error Handling:**
```typescript
try {
  await ptyClient.write(bytes);
} catch (error) {
  console.error("Failed to write to PTY:", error);
  // Continue execution - do not crash
}
```

### Database Schema

Not applicable - this feature does not persist data.

### Dependencies

**Internal Dependencies:**
- `PtyClient` (src/pty/client.ts): Used to send text to PTY
- `TerminalState` (src/terminal/state.ts): Used to get cursor position
- Existing `handleKeyDown` in src/main.ts: Must coexist with IME handlers

**External Dependencies:**
- Browser/WebView IME API: Native composition events
- TextEncoder API: UTF-8 encoding

### File Structure

```
src/
├── main.ts                       # Main modifications here
│   ├── initTerminal()           # Create textarea/EditContext, Composition View
│   ├── setupIMEHandlers()       # IME event setup (textarea fallback)
│   ├── setupEditContextIME()    # EditContext API setup (Chromium primary)
│   ├── updateIMEPosition()      # Position sync for textarea
│   ├── updateCompositionView()  # Update Composition View overlay
│   ├── hasSKKMarker()           # Detect SKK IME markers
│   ├── isSpecialKey()           # Detect special keys for bypass
│   └── handleKeyDown()          # Skip during composition
└── pty/
    ├── client.ts                # No changes needed
    └── keyboard.ts              # No changes needed
```

## Implementation Details

### 1. Dual-Mode IME Input (EditContext + Textarea Fallback)

**Location:** `src/main.ts` - in `initTerminal()` function

**Code Structure:**
```typescript
// Global state
let imeInput: HTMLTextAreaElement | null = null;

function initTerminal(): void {
  const terminal = document.getElementById("terminal");
  if (!terminal) return;

  // Create Composition View overlay
  const compositionView = document.createElement("div");
  compositionView.id = "composition-view";
  compositionView.style.cssText = `
    position: fixed;
    display: none;
    background: #333;
    color: #fff;
    padding: 2px 6px;
    border-radius: 3px;
    font-family: monospace;
    z-index: 1000;
    pointer-events: none;
  `;
  document.body.appendChild(compositionView);

  // Try EditContext API first (Chromium/WebView2)
  if ("EditContext" in window) {
    console.log("[IME] EditContext API available, using it");
    setupEditContextIME(terminal, compositionView);
  } else {
    console.log("[IME] EditContext API not available, using textarea fallback");
    // Create hidden textarea for IME (fallback for WebKit)
    imeInput = document.createElement("textarea");
    imeInput.autocomplete = "off";
    imeInput.style.cssText = `
      position: fixed;
      left: -9999px;
      top: 0;
      width: 10px;
      height: 10px;
      opacity: 0;
      border: none;
      padding: 0;
      margin: 0;
      outline: none;
      overflow: hidden;
      resize: none;
    `;
    document.body.appendChild(imeInput);
    setupIMEHandlers(imeInput, compositionView);
  }
}
```

**Rationale:**
- **EditContext API**: Modern Chromium API for superior IME integration (primary)
- **Hidden Textarea**: Fallback for WebKit-based environments
- **Off-screen positioning** (`left: -9999px`): Avoids visual artifacts while remaining focusable
- **Composition View**: Provides visual feedback at cursor position regardless of IME mode

### 2. Focus Management

**Location:** `src/main.ts` - modify existing click handler

**Code Structure:**
```typescript
function initTerminal(): void {
  // ... existing code ...

  // Focus handling - focus hidden input instead of terminal
  terminal.addEventListener("click", () => {
    if (imeInput) {
      imeInput.focus();
    }
  });

  // Initial focus
  if (imeInput) {
    imeInput.focus();
  }
}
```

**Rationale:**
- Clicking terminal focuses hidden input, enabling IME
- Hidden input maintains focus for continuous IME usage

### 3. IME Event Handlers

**Location:** `src/main.ts` - function `setupIMEHandlers()`

**Code Structure:**
```typescript
function setupIMEHandlers(input: HTMLTextAreaElement, view: HTMLDivElement): void {
  let isComposing = false;
  let lastSentValue = "";
  let lastSentTimestamp = 0;

  // Handle compositionstart
  input.addEventListener("compositionstart", () => {
    isComposing = true;
  });

  // Handle input event (primary handler)
  input.addEventListener("input", async (event: Event) => {
    const inputEvent = event as InputEvent;
    const value = input.value;

    // During composition, show in Composition View
    if (inputEvent.isComposing || isComposing || hasSKKMarker(value)) {
      updateCompositionView(view, value);
      return;
    }

    // Not composing - send confirmed text to PTY
    if (!value) return;

    // Duplicate detection - skip if same value sent within 100ms
    const now = Date.now();
    if (value === lastSentValue && now - lastSentTimestamp < 100) {
      input.value = "";
      updateCompositionView(view, "");
      return;
    }

    try {
      const bytes = new TextEncoder().encode(value);
      if (ptyClient) {
        await ptyClient.write(bytes);
        // Note: No separate CR sending - standard IME handles Enter internally
        lastSentValue = value;
        lastSentTimestamp = now;
      }
    } catch (error) {
      console.error("Failed to write IME input to PTY:", error);
    } finally {
      input.value = "";
      updateCompositionView(view, "");
    }
  });

  // Handle compositionend (fallback)
  input.addEventListener("compositionend", async () => {
    isComposing = false;
    const value = input.value;
    if (!value || hasSKKMarker(value)) {
      updateCompositionView(view, value);
      return;
    }
    // Similar duplicate detection and PTY write...
  });
}

// SKK IME marker detection
function hasSKKMarker(text: string): boolean {
  return /[▽▼【】]/.test(text);
}
```

**Event Flow:**
1. User types and converts Japanese text
2. During composition: Show text in Composition View overlay
3. SKK markers detected: Keep composing (don't send yet)
4. On confirmation: `compositionend` fires → get text from `input.value`
5. `input` event fires with `isComposing === false` → send to PTY
6. **No separate CR sending**: Standard IME handles Enter internally to prevent double newlines

**Rationale:**
- Use `input.value` instead of `event.data` for reliability
- Composition View provides visual feedback at cursor position
- SKK marker detection for Emacs-style IME support
- Duplicate detection prevents double-sending within 100ms
- Clear `input.value` immediately for security

### 4. Cursor Position Synchronization & Composition View

**Location:** `src/main.ts` - functions `updateIMEPosition()` and `updateCompositionView()`

**Code Structure:**
```typescript
// Update hidden textarea position (for IME candidate window)
function updateIMEPosition(): void {
  if (!imeInput || !terminalState) return;

  const cursorCol = terminalState.cursorCol;
  const cursorRow = terminalState.cursorRow;
  const rows = terminalState.rows;

  const terminal = document.getElementById("terminal");
  if (!terminal) return;

  const rect = terminal.getBoundingClientRect();
  const styles = getComputedStyle(terminal);
  const paddingLeft = parseFloat(styles.paddingLeft) || 0;
  const paddingTop = parseFloat(styles.paddingTop) || 0;

  const x = cursorCol * charSize.width + paddingLeft;
  const y = cursorRow * charSize.height + paddingTop;

  // Position above cursor if on bottom row, below otherwise
  let top: number;
  if (cursorRow === rows - 1) {
    top = rect.top + y - charSize.height;
  } else {
    top = rect.top + y + charSize.height;
  }

  imeInput.style.left = `${rect.left + x}px`;
  imeInput.style.top = `${top}px`;
}

// Update Composition View overlay
function updateCompositionView(view: HTMLDivElement, text: string): void {
  if (!text) {
    view.style.display = "none";
    return;
  }

  view.textContent = text;
  view.style.display = "block";

  // Position at cursor (similar calculation as updateIMEPosition)
  // ... position logic ...
}
```

**Call Sites:**
- After terminal state updates (in `setupNewTerminalHandlers`)
- On terminal resize (in resize observer callback)
- During composition (Composition View updates in real-time)

**Rationale:**
- **updateIMEPosition()**: Positions hidden textarea for OS IME candidate window
- **updateCompositionView()**: Shows composition text overlay for visual feedback
- Handles edge case of bottom row by positioning above
- Uses terminal container's bounding rect with padding for accuracy

### 5. Coexistence with Existing Keydown Handler

**Location:** `src/main.ts` - modify `handleKeyDown()`

**Current Code:**
```typescript
async function handleKeyDown(event: KeyboardEvent): Promise<void> {
  if (!ptyClient || !shouldHandleKey(event)) {
    return;
  }

  // Skip if IME composition is in progress
  if (event.isComposing) {
    return;
  }

  // ... existing key handling ...
}
```

**Required Modification:**
```typescript
async function handleKeyDown(event: KeyboardEvent): Promise<void> {
  if (!ptyClient || !shouldHandleKey(event)) {
    return;
  }

  // Skip if IME composition is in progress
  if (event.isComposing) {
    return;
  }

  // Skip if hidden input has focus (IME might be active)
  if (document.activeElement === imeInput) {
    // Let IME handler deal with it, unless it's a special key
    if (!isSpecialKey(event)) {
      return;
    }
  }

  // ... existing key handling ...
}

function isSpecialKey(event: KeyboardEvent): boolean {
  // Special keys that should always be handled by keydown
  return event.ctrlKey || event.key.startsWith("Arrow") ||
         event.key === "Escape" || event.key === "Tab";
}
```

**Rationale:**
- Prevents double-processing of regular characters
- Allows special keys (Ctrl+C, arrows) to be handled by keydown even during IME focus
- Maintains backward compatibility with existing functionality

### 6. Cleanup

**Location:** `src/main.ts` - modify `cleanup()` function

**Code:**
```typescript
function cleanup(): void {
  // ... existing cleanup ...

  // Remove IME input element
  if (imeInput && imeInput.parentNode) {
    imeInput.parentNode.removeChild(imeInput);
    imeInput = null;
  }
}
```

## Test Scenarios

### Unit Tests

Since this feature primarily involves DOM event handling, unit tests are not applicable. Manual testing is the primary validation method.

### Integration Tests

Not applicable - E2E tests are not required per requirements.

### Manual Test Scenarios

#### Test 1: Basic Japanese Input
**Steps:**
1. Start eMterm
2. Click terminal area
3. Activate Japanese IME (switch to Japanese input mode)
4. Type "nihongo" (にほんご)
5. Press Space to convert
6. Press Enter to confirm

**Expected Result:**
- "日本語" appears in the terminal
- Text is sent to shell correctly

#### Test 2: Katakana Conversion
**Steps:**
1. Type "nihongo" (にほんご)
2. Press F7 to convert to katakana
3. Confirm

**Expected Result:**
- "ニホンゴ" appears in terminal

#### Test 3: Candidate Window Positioning
**Steps:**
1. Type Japanese text at various cursor positions
2. Observe candidate window location

**Expected Result:**
- Candidate window appears below cursor normally
- Candidate window appears above cursor when on bottom line

#### Test 4: Long Text Input (100+ Characters)
**Steps:**
1. Type and convert 100+ characters of Japanese text
2. Monitor performance

**Expected Result:**
- No lag or delay
- All characters sent correctly

#### Test 5: Enter Key Confirmation
**Steps:**
1. Type "nihongo" and convert to "日本語"
2. Press Enter to confirm

**Expected Result:**
- "日本語" is confirmed and sent to PTY
- No double newline occurs (standard IME handles Enter internally)
- Shell receives the text correctly

#### Test 6: Focus Loss and Recovery
**Steps:**
1. Start Japanese input (during composition)
2. Click outside eMterm window (lose focus)
3. Click back to eMterm (regain focus)
4. Continue composition

**Expected Result:**
- Composition state is preserved
- Can continue typing and converting

#### Test 7: Special Keys During IME
**Steps:**
1. Start Japanese composition
2. Press Ctrl+C

**Expected Result:**
- Ctrl+C sends interrupt signal (existing behavior maintained)

#### Test 8: Empty Confirmation
**Steps:**
1. Activate IME
2. Press Enter without typing anything

**Expected Result:**
- No error
- Just a newline is sent

#### Test 9: Composition View Display
**Steps:**
1. Type "nihongo" with IME active
2. Observe Composition View overlay

**Expected Result:**
- Composition View appears at cursor position showing "にほんご"
- Text updates in real-time as you type
- View disappears after confirmation

#### Test 10: SKK IME Support (if using SKK)
**Steps:**
1. Type with SKK IME active
2. Observe markers (▽, ▼)

**Expected Result:**
- SKK markers are detected correctly
- Text is not sent until markers are removed
- Final text is sent after confirmation

### Edge Cases

- [ ] Edge case 1: PTY session not started → Input is ignored, no crash
- [ ] Edge case 2: Very rapid typing (stress test) → All characters captured
- [ ] Edge case 3: Switching between English and Japanese rapidly → No lost characters
- [ ] Edge case 4: Terminal resize during composition → Position updates correctly
- [ ] Edge case 5: Multiple sequential confirmations → Each handled independently

### Performance Tests

#### Performance Test 1: Input Latency
**Method:**
1. Instrument `ptyClient.write()` with timestamp logging
2. Type Japanese characters and measure time from `input` event to `write()` completion
3. Repeat 100 times and calculate average

**Acceptance Criteria:** Average latency < 50ms

#### Performance Test 2: Long Text Performance
**Method:**
1. Input 500 characters of Japanese text
2. Monitor memory usage
3. Monitor frame rate

**Acceptance Criteria:**
- Memory increase < 10MB
- No visible lag

## Security Considerations

- **Input Value Clearing:** `input.value` is cleared immediately after sending to prevent sensitive data from remaining in memory
- **XSS Prevention:** Text is encoded as UTF-8 bytes directly via `TextEncoder`, never interpreted as HTML
- **Focus Hijacking Prevention:** Hidden input has `pointer-events: none` to prevent accidental focus
- **Data Validation:** No special validation needed - all UTF-8 text is valid terminal input

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| N/A | PTY write failure | N/A | (Console log only) |
| N/A | Input element creation failure | N/A | (Console log only) |

### Error Flow

```
IME Confirmation → Text Encoding → PTY Write
                                      ↓ (error)
                                   Log Error
                                      ↓
                                   Continue
                                   (no crash)
```

**Error Handling Policy:**
- All errors are logged to console
- No user-facing error messages
- Application continues to function
- Existing keyboard input remains functional

## Performance Optimization

### Performance Goals
- Response time: < 50ms for IME input processing
- Throughput: Support 10+ characters/second sustained input
- Memory overhead: < 2KB for IME elements (textarea + Composition View)

### Optimization Strategies
- Use `textarea` for better IME compatibility (vs `input`)
- EditContext API as primary method (lower overhead on Chromium)
- Clear `input.value` immediately after processing to free memory
- Avoid unnecessary DOM operations during input handling
- Position updates only when cursor moves (not on every render)
- Duplicate detection (100ms window) prevents redundant PTY writes

### Caching Strategy
Not applicable - no caching needed for this feature.

## Success Criteria

- [ ] All functional requirements (FR1-FR8) are implemented and tested
- [ ] All manual test scenarios pass on Linux (Fcitx) and Windows (MS-IME)
- [ ] Performance meets specified goals (< 50ms latency)
- [ ] Security requirements are satisfied (immediate value clearing)
- [ ] Code review is completed
- [ ] No regression in existing keyboard handling functionality
- [ ] EditContext API works on Chromium/WebView2
- [ ] Textarea fallback works on WebKit-based environments
- [ ] Composition View displays correctly during composition
- [ ] SKK IME markers are detected and handled properly

## Open Questions

None - all clarifications were obtained during requirements gathering.

## Implementation Phases

This feature was implemented in four phases:

### Phase 1: Hidden Textarea Element Creation and Focus Management
- Create invisible textarea for IME input
- Focus management when terminal is clicked
- Cleanup on disposal

### Phase 2: IME Event Handlers and PTY Integration
- `input` and `compositionend` event handlers
- UTF-8 encoding and PTY write
- Duplicate detection mechanism
- SKK IME marker detection

### Phase 3: Cursor Position Sync and Composition View
- `updateIMEPosition()` for textarea positioning
- Composition View overlay for visual feedback
- Position updates on cursor move and resize

### Phase 4: Keyboard Coexistence and EditContext API
- `isSpecialKey()` for bypass logic
- `handleKeyDown()` modifications
- EditContext API integration (Chromium primary mode)

## Platform Support

| Platform | IME | Support Level |
|----------|-----|---------------|
| Linux | Fcitx | ✅ Recommended |
| Linux | iBus | ⚠️ Unsupported |
| Linux | Other | ⚠️ Unsupported |
| Windows | MS-IME | ✅ Supported |
| Windows | Google Japanese Input | ✅ Supported |
| macOS | Default IME | ⚠️ Best-effort |

## References

- Requirements Document: `doc/tasks/ime-input-support/要件定義書.md`
- Implementation Plan: `doc/tasks/ime-input-support/IMPLEMENTATION.md`
- Tabby Terminal IME Implementation: https://github.com/Eugeny/tabby
- MDN Composition Events: https://developer.mozilla.org/en-US/docs/Web/API/CompositionEvent
- MDN InputEvent: https://developer.mozilla.org/en-US/docs/Web/API/InputEvent
- EditContext API: https://developer.mozilla.org/en-US/docs/Web/API/EditContext
- Current Keyboard Handler: `src/pty/keyboard.ts`
- PTY Client: `src/pty/client.ts`
