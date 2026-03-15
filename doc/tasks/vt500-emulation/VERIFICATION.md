# VERIFICATION: VT500 Emulation Level Migration

## Automated Tests

### V1: WASM Unit Tests

```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && cargo test csi_device"
```

- [ ] `test_da1` passes with new value `\x1b[?65;1;4;22c`
- [ ] `test_da2` passes with new value `\x1b[>65;1;0c`
- [ ] All other device tests pass unchanged

### V2: Full Test Suite

```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && cargo test"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

- [ ] All WASM tests pass
- [ ] All TypeScript tests pass
- [ ] TypeScript typecheck passes

## Manual Verification

### V3: vim Search Wrap (NFR1)

1. Open a file in vim inside eMterm
2. Search forward (`/pattern`) and press `n` until wrap → message "下まで検索したので上に戻ります" should stay
3. Search backward (`?pattern`) and press `N` until wrap → message "上まで検索したので下に戻ります" should stay
4. Both messages should persist until next keypress

### V4: Application Compatibility (NFR1)

- [ ] tmux starts and operates normally
- [ ] vim opens and basic editing works
- [ ] less scrolling works correctly
- [ ] Shell prompt and commands render correctly
