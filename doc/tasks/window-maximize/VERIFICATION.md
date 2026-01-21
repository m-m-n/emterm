# Verification Document: Window Maximize on Startup

## Overview
**Feature**: Window Maximize on Startup
**SPEC.md**: `doc/tasks/window-maximize/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/window-maximize/IMPLEMENTATION.md`

## Implementation Status

**Date:** 2026-01-21
**Status:** Implementation Complete
**All Tests:** PASS (no new tests required - configuration-only change)

### Implementation Summary
Added `maximized: true` to the Tauri window configuration to start the application with a maximized window.

### Phase Summary
- [x] Phase 1: Configuration Update - Added `"maximized": true` to `src-tauri/tauri.conf.json`

### Build Results
```bash
$ bun run build
Bundled 255 modules in 71ms
Build successful

$ cargo build --manifest-path src-tauri/Cargo.toml
Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.84s
Build successful

$ bun run typecheck
Success
```

---

## Build Verification

### Build Command
```bash
bun tauri build
```

### Expected Result
- Exit code: 0
- No error messages related to configuration

## Configuration Verification

### Configuration Check
Verify `src-tauri/tauri.conf.json` contains the correct window configuration:

```bash
grep -A 10 '"windows"' src-tauri/tauri.conf.json
```

### Expected Configuration
```json
{
  "app": {
    "windows": [
      {
        "title": "eMterm",
        "width": 800,
        "height": 600,
        "resizable": true,
        "fullscreen": false,
        "maximized": true
      }
    ]
  }
}
```

### Verification Points
| Property | Expected Value | Purpose |
|----------|----------------|---------|
| maximized | true | Start window maximized |
| resizable | true | Allow user to resize |
| fullscreen | false | Not fullscreen mode |

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | Application starts with maximized window | Launch app, observe window state |
| SC-2 | User can restore and resize window | Click restore, drag edges |
| SC-3 | User can re-maximize window | Click maximize button |
| SC-4 | No regression in existing functionality | Run existing test suite |

### Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| FR1: Window must open in maximized state | Launch and observe |
| FR2: Window must remain resizable | Drag window edges |
| FR3: Fullscreen mode not used | Verify fullscreen: false in config |

### User Story Acceptance Criteria

**US1: Maximized Window on Startup**
- [ ] Window opens in maximized state on application launch
- [ ] Terminal content fills the maximized window

**US2: Window Resize**
- [ ] User can restore (un-maximize) the window
- [ ] User can resize the window by dragging edges
- [ ] User can re-maximize the window

## Manual Testing Checklist

### Basic Functionality
- [ ] Launch application with `bun tauri dev`
- [ ] Verify window opens in maximized state
- [ ] Verify terminal is visible and functional

### Window Operations
- [ ] Click restore button (or double-click title bar)
- [ ] Verify window becomes approximately 800x600
- [ ] Drag window edges to resize
- [ ] Verify resize works in all directions
- [ ] Click maximize button
- [ ] Verify window maximizes again

### Additional Tests
- [ ] Minimize window and restore (window returns to maximized state)
- [ ] Close and reopen application
- [ ] Verify window is maximized on each launch

### Platform Tests
- [ ] Linux: Verify all above tests pass
- [ ] (Future) Windows: Verify all above tests pass
- [ ] (Future) macOS: Verify all above tests pass

## Non-Functional Verification

### Performance (NFR1)
- [ ] Application startup time is not noticeably affected

### Compatibility (NFR2)
- [ ] Works on Linux (primary target)

## Regression Testing

### Existing Test Suite
```bash
# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# TypeScript tests
bun test

# Type check
bun run typecheck
```

### Expected Result
- All existing tests pass
- No new errors or warnings

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 1 | Yes | - |
| Configuration | 3 | Yes | - |
| SPEC Compliance | 4 | - | Yes |
| User Stories | 5 | - | Yes |
| Manual Testing | 15 | - | Yes |
| Regression | 3 | Yes | - |

**Total**: 7 automated checks, 24 manual verification items
