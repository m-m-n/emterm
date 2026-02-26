# Feature: SKK IME Freeze Fix

## Overview

Fix a bug where using SKK (fcitx5-skk) to input Japanese causes the composition view to freeze and Japanese input to become non-functional. The root cause is the `hasSKKMarker()` method in `ImeHandler` which incorrectly detects SKK conversion markers (▽/▼) in the input text, preventing composition from completing normally.

## Objectives

- Remove the SKK-specific marker detection logic (`hasSKKMarker()`)
- Rely solely on standard composition events (compositionstart/compositionend) for IME state management
- Ensure Japanese input via SKK works without freezing

## User Stories

### US1: SKK Japanese Input
As a terminal user using fcitx5-skk, I want to input Japanese text normally, so that I can use SKK without the composition view freezing.

**Acceptance Criteria:**
- [ ] Japanese input via SKK completes without the composition view freezing
- [ ] Composition view appears during input and disappears after conversion
- [ ] Converted text is correctly sent to PTY
- [ ] English/ASCII input remains unaffected

## Technical Requirements

### Functional Requirements
- **FR1:** Remove `hasSKKMarker()` method and all references to it
- **FR2:** The `input` event handler must not use SKK marker detection to determine composition state; rely on `inputEvent.isComposing` and the local `isComposing` flag only
- **FR3:** The `compositionend` handler must not skip sending text to PTY based on SKK marker presence

### Non-Functional Requirements
- **NFR1 - Compatibility:** Standard IME (fcitx5-mozc, ibus, etc.) must continue to work correctly
- **NFR2 - No Regression:** EditContext API path (Chromium/WebView2) is unaffected (it does not use `hasSKKMarker()`)

## Implementation Approach

### Changes

**File:** `src/terminal-app/handlers/ime.ts`

1. **Remove `hasSKKMarker()` method** (lines 469-477)

2. **Remove SKK marker check in `input` event handler** (line 647):
   ```typescript
   // Before:
   if (inputEvent.isComposing || isComposing || this.hasSKKMarker(value))
   // After:
   if (inputEvent.isComposing || isComposing)
   ```

3. **Remove SKK marker check in `compositionend` handler** (lines 708-715):
   ```typescript
   // Remove this block entirely:
   if (this.hasSKKMarker(value)) {
       this.updateCompositionView(value);
       return;
   }
   ```

4. **Remove SKK marker debug logging** in `input` event handler (line 639):
   ```typescript
   // Remove: hasSKKMarker: this.hasSKKMarker(value),
   ```

### Root Cause Analysis

The `hasSKKMarker()` method checks if the textarea value contains ▽, ▼, or 【】. When SKK enters conversion mode:

1. fcitx5-skk places ▽ or ▼ prefix in the composition text
2. `hasSKKMarker()` detects these markers
3. In the `input` handler: the marker check forces the code path into "composing" mode, preventing text from being sent to PTY
4. In the `compositionend` handler: the marker check skips sending the final text, leaving composition view visible
5. Result: composition view stays frozen, and subsequent Japanese input fails (English still works because it bypasses composition)

Standard composition events (`compositionstart`, `compositionend`, `isComposing`) already correctly track IME state for SKK. The `hasSKKMarker()` logic is redundant and harmful.

## Test Scenarios

### Unit Tests
- [ ] Verify `input` event with `isComposing=false` sends text to PTY
- [ ] Verify `input` event with `isComposing=true` shows composition view
- [ ] Verify `compositionend` sends final text to PTY and clears composition view

### Manual Tests
- [ ] fcitx5-skk: Enter conversion mode (▽), select candidate (▼), confirm — text appears in terminal
- [ ] fcitx5-skk: Cancel conversion — composition view clears
- [ ] Standard IME (mozc etc.): Japanese input works as before
- [ ] Direct ASCII input works as before

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

## Success Criteria

- [ ] SKK Japanese input completes without freezing
- [ ] Composition view appears/disappears correctly during SKK conversion
- [ ] Standard IME input is not regressed
- [ ] All existing tests pass
