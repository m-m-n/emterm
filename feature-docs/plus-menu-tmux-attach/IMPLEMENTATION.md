# Implementation Plan: Plus-Menu tmux Attach

## Overview

Add live tmux socket rows to the new-tab chooser and spawn an attaching tab on selection. Single-task feature; this document records only the decisions that constrain the whole feature.

## Technology Stack

- **Language**: Rust (existing `emterm` binary, `gui` feature)
- **Key libraries**: standard library only — Unix domain socket connect probe via the standard library's Unix networking; no new dependencies (license: MIT, unaffected)

## Layer Structure

- Discovery (new module `src-tauri/src/tmux_sockets.rs`): pure enumeration logic, no UI knowledge. Unix-only (`#[cfg(unix)]`), declared under the `gui` feature in `lib.rs`.
- UI (existing `profile_selector` chooser): renders rows, decodes selection.
- App (existing `app.rs`): calls discovery on chooser open, dispatches the attach spawn via the existing `SpawnOverrides` path.

Dependency direction: App → Discovery, App → UI. UI never calls Discovery directly (the socket list is handed to the chooser state by App).

## Shared Components

Single task — no cross-task contracts.

## Conventions

- Error policy: discovery never fails the chooser — missing directory or unreadable entries degrade to an empty/partial list; failures worth noting use the existing logging module at warn level or below per project rules.
- Naming: follow existing chooser vocabulary (`Choice`, `row_to_choice`, `open_with_global`).

## Cross-task Design Decisions

### Attach is a plain PTY spawn, not a mux integration

The tmux attach tab is an ordinary tab whose PTY runs the tmux client (via `SpawnOverrides` with an explicit executable and argv). It does not touch eMterm's built-in mux subsystem. Rationale: SPEC A5; keeps the feature isolated from `src-tauri/src/mux/`.

### Profile-empty fast path changes meaning

Today, zero profiles → "+" spawns a tab immediately without opening the chooser. With this feature, the chooser must open when tmux sockets exist even if profiles are empty (SPEC edge case). The fast path applies only when profiles AND live tmux sockets are both absent.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Connect probe blocks the UI thread on a pathological socket | Low | Medium | Non-blocking connect / short timeout; probe count is bounded by socket-dir entries |
| Chooser row-index drift (Global / profiles / tmux ordering) | Medium | Medium | Selection decode goes through one function (`row_to_choice`) with unit tests over the combined ordering |
| Windows / CLI builds break via unixonly APIs | Low | High | `#[cfg(unix)]` gating; both builds are part of verification |

## Open Questions

- None.
