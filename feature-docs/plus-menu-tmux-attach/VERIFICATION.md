# Verification Document: Plus-Menu tmux Attach

## Overview
**Feature**: plus-menu-tmux-attach / **SPEC.md**: `feature-docs/plus-menu-tmux-attach/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/plus-menu-tmux-attach/IMPLEMENTATION.md`

## Build Verification
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (windows): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Note: `tabs.rs` replay tests are known to be flaky in parallel runs; on failure there, re-check against the base tree (pre-existing flakiness is not a feature failure)

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Discovery over live socket + stale socket + regular file | Only the live socket returned (name + absolute path) | Unit |
| TS-2 | Discovery with missing socket directory | Empty list, no error | Unit |
| TS-3 | Selection decode over Global → profiles → tmux ordering | Every row index maps to the correct choice; M=0 matches today's behavior | Unit |
| TS-4 | Attach override construction | Executable `tmux`, argv exactly `-S {socket_path} attach` | Unit |
| TS-5 | Profile-empty fast path | Sockets present → chooser opens; no sockets → immediate spawn preserved | Unit |
| TS-6 | CLI-only and Windows cross builds | Both check commands exit 0 (Unix-only code fully gated) | Build |

## Code Quality Verification
- Format: none configured (project has no enforced format command)

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | TS-1..TS-5 pass |
| SC-2 | Windows and CLI-only builds keep compiling | Build Verification commands |
| SC-3 | Live attach works end to end | Manual M-1 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | TS-3, TS-5, M-1 |
| FR3 | task0001 | TS-4, M-1 |
| NFR1 | task0001 | TS-1 design (no subprocess, bounded probes); M-1 perceived latency |
| NFR2 | task0001 | TS-6 (Build Verification cli + windows) |
| NFR3 | task0001 | TS-3 (M=0 case), `--lib` suite green |

## Manual Testing (E2E Not Possible)
- [ ] M-1: Start `tmux -L test` with a running session. Open the + menu → a `tmux: test` row appears. Click it → a new tab opens attached to the session. Detach/exit → tab behaves like a normal ended shell tab.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Unit scenarios | 5 | 5 | 0 | 0 |
| Manual | 1 | 0 | 0 | 1 |
