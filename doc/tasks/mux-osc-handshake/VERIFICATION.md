# Verification: Mux Protocol Redesign

## Build Verification

```bash
# CLI-only build (remote server use case)
cargo build --release --no-default-features --manifest-path src-tauri/Cargo.toml

# GUI build
cargo check --manifest-path src-tauri/Cargo.toml

# TypeScript typecheck
bun run typecheck

# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

## Phase 1: Handshake Removal

### Automated
- [ ] Build succeeds (CLI-only + GUI)
- [ ] Existing mux tests pass
- [ ] `handshake_emterm()` function no longer exists in cli.rs
- [ ] OSC query/ACK handler removed from osc-handler.ts
- [ ] Bridge timeout test: mock daemon that never responds → bridge exits after 5s

### Manual
- [ ] `emterm mux` starts instantly inside eMterm (no delay)
- [ ] `emterm mux` on SSH server starts without freeze
- [ ] `emterm mux` in non-eMterm terminal: bridge exits cleanly after timeout
- [ ] Nesting prevention: `EMTERM_MUX=1 emterm mux` → error

## Phase 2: SwitchWindow Handler

### Automated
- [ ] SwitchWindow message triggers active_window_id update in session
- [ ] StatusUpdate sent after SwitchWindow
- [ ] Unit test: switch window → verify session.active_window_id changed

### Manual
- [ ] prefix+n cycles through windows on daemon side

## Phase 3: GUI Tab Integration

### Automated
- [ ] TypeScript typecheck passes
- [ ] Tab creation/deletion events fire correctly

### Manual
- [ ] Entering mux mode creates tab(s) for existing windows
- [ ] prefix+c creates new window → new tab appears
- [ ] Clicking tab switches displayed content instantly
- [ ] Tab close → window destroyed, panes killed
- [ ] Tab names show window names from StatusUpdate
- [ ] Activity indicator on tab when background window has output

## Phase 4: All-Window Output Routing

### Manual
- [ ] Start 2 windows, run `yes` in window 2, switch to window 1
- [ ] Switch back to window 2 → output is current (no delay/catch-up)
- [ ] Run long command in background window → no visible performance impact on active window

## Phase 5: Reattach Multi-Window

### Manual
- [ ] Create 3 windows with different content
- [ ] Detach (prefix+d or close eMterm)
- [ ] Reattach (`emterm mux` or `emterm mux attach`)
- [ ] All 3 windows restored with correct content
- [ ] Tab bar shows all 3 windows
- [ ] Active window is same as before detach

## SPEC.md Requirements Traceability

| Requirement | Phase | Verification |
|-------------|-------|-------------|
| FR1: No-Check Startup | 1 | Manual: instant start |
| FR2: Daemon-Side Grid | — | Already exists (shadow parser) |
| FR3: Raw Bytes Forwarding | — | Already exists |
| FR4: All-Window Streaming | 4 | Manual: background output |
| FR5: Window GUI Tab Mapping | 3 | Manual: tabs ↔ windows |
| FR6: Window Lifecycle Messages | 2, 3 | Automated + Manual |
| FR7: Window Switch Behavior | 3 | Manual: instant switch |
| FR8: Reattach Screen Restoration | 5 | Manual: multi-window reattach |
| FR9: Bridge Timeout | 1 | Automated: 5s timeout test |
| FR10: Nesting Prevention | 1 | Manual: EMTERM_MUX check |
| NFR1: No blocking on startup | 1 | Manual: instant start |
| NFR2: Memory efficiency | — | Shadow parser already allocated |
| NFR3: Bandwidth | 4 | Manual: idle window minimal traffic |
