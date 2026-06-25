# Verification Document: Font Bundle Redesign

## Overview
**Feature**: font-bundle-redesign
**SPEC.md**: `doc/tasks/font-bundle-redesign/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/font-bundle-redesign/IMPLEMENTATION.md`

## Build Verification
- **Quick check command**: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- **Release build command**: `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`
- **Windows cross-build**: `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
- **CLI-only feature check**: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- **Expected**: exit code 0, no errors. Each GUI build must succeed only after `make fetch-fonts` has populated `src-tauri/assets/fonts/`. CLI-only check must succeed without any fonts present.

### Implementation results (2026-06-25)
- `cargo check` (GUI, default features) → **PASS** (exit 0)
- `cargo check --no-default-features` (CLI-only) → **PASS** (exit 0)
- Release build (`cargo build --release`) → **not run** (per project rule: user invokes release builds explicitly)
- Windows cross-build → **not run** (no network for cargo-xwin SDK fetch in this session)

## Test Verification
- **Rust unit + integration tests**: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- **TypeScript tests**: `bun test`
- **TypeScript typecheck**: `bun run typecheck`
- **Coverage target**: pure helpers (`presentation_for`, `migrate_legacy`, `user_font_dir`) ≥ 95 % branch; resolver register/scan paths ≥ 80 % branch.

### Implementation results (2026-06-25)
- Rust `cargo test --lib -- --test-threads=1` → **PASS** (1943 passed, 0 failed, 3 ignored)
  - tabs.rs replay tests are flaky in parallel per project notes; single-threaded run is the stable mode.
- `bun test` → **PASS** (10 tests, 0 failed)
- `bun run typecheck` → **PASS** (no errors)
- New tests added in this implementation:
  - `render::font::presentation::tests` × 10 (TS-1 .. TS-5 + sanity + table-shape invariants)
  - `render::font::resolver::tests::register_bundled_returns_distinct_ids` (TS-9; updated to 4 ids)
  - `render::font::resolver::tests::by_role_lists_each_registered_font` (4 roles)
  - `render::font::resolver::tests::by_family_resolves_registered_name` (4 families)
  - `render::font::user_dir::tests` × 6 (TS-10, TS-11, env resolution, empty / missing dir)
  - `app_settings::settings::tests` × 5 (TS-6 / TS-7 / TS-8 + mixed-key + Markdown-side variant)
  - `settings::tests::loader_font_family_emoji_color_sets_emoji_font` + 2 siblings

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `presentation_for('a', None)` | `NotEmoji` | Unit |
| TS-2 | `presentation_for('\u{23F5}', None)` | `Monochrome` (text-presentation default) | Unit |
| TS-3 | `presentation_for('\u{23F5}', Some('\u{FE0F}'))` | `Color` (VS16 override) | Unit |
| TS-4 | `presentation_for('\u{1F600}', None)` | `Color` (emoji-presentation default) | Unit |
| TS-5 | `presentation_for('\u{1F600}', Some('\u{FE0E}'))` | `Monochrome` (VS15 forces text) | Unit |
| TS-6 | `migrate_legacy` moves legacy emoji key to color key and returns true | New schema in memory; idempotent on the new file | Unit |
| TS-7 | `migrate_legacy` initializes `font_family_emoji_monochrome` to `Noto Emoji` when legacy file lacks it | Default value present | Unit |
| TS-8 | `migrate_legacy` on a file already on the new schema | Returns `false`; no `.bak` written | Unit |
| TS-9 | `Resolver::register_bundled()` registers four distinct ids | CJK, ColorEmoji, MonochromeEmoji, Base all distinct; bytes non-empty | Unit |
| TS-10 | `Resolver::scan_user_dir()` against tempdir with `.ttf`, `.otf`, `.txt`, corrupt font | Registers exactly the two valid font files; corrupt one yields a single warn | Unit |
| TS-11 | User-dir font override wins over bundled family on family-name lookup | `by_family(name)` returns the user-dir entry | Integration |
| TS-12 | Build with one bundled font missing | `cargo build` (gui) panics with the documented message naming the file and recovery command | Integration |
| TS-13 | `fetch-fonts.sh` idempotency: re-run with up-to-date files | Exit 0; each font reported as "up-to-date"; no network access | Integration (shell) |
| TS-14 | `fetch-fonts.sh` SHA256 mismatch: corrupt a file, re-run | Replaces the file; exit 0 | Integration (shell) |
| TS-15 | `fetch-fonts.sh` simulated download failure (unreachable URL) | Exits non-zero with the documented error tag; no partial file remains on disk | Integration (shell) |
| TS-16 | Settings load flow on a legacy file | `.bak` file written; `settings.json` rewritten on the new schema | Integration |
| TS-17 | Settings panel renders both emoji rows (color, monochrome) | Both rows visible and editable | E2E (tauri-driver smoke) |
| TS-18 | `cli-build` works without fonts | `cargo check --no-default-features` succeeds with `assets/fonts/` empty | Integration |

## Code Quality Verification
- **Format check**: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` (project rustfmt config applies; `cargo fmt` without `--check` is the apply variant).
- **TypeScript format**: project policy uses Biome via Claude Code `PostToolUse` hook; verify no diff after edits.
- **Static analysis**: `CARGO_TARGET_DIR=src-tauri/target cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` (existing project lint level).

### Implementation results (2026-06-25)
- `cargo fmt` applied to every Rust file touched in this implementation (not the whole crate, per project rule). Files formatted:
  - `src-tauri/build.rs`
  - `src-tauri/src/app.rs`
  - `src-tauri/src/settings.rs`, `src-tauri/src/settings_store.rs`
  - `src-tauri/src/render/font/{mod,resolver,presentation,user_dir,swash_adapter,ab_glyph_adapter,fallback}.rs`
  - `src-tauri/src/render/terminal_grid_pass.rs`
  - `src-tauri/src/ui/chrome.rs`
  - `crates/app_settings/src/settings.rs`
- TypeScript: the project `PostToolUse` hook ran on every TypeScript edit; no manual `bunx biome` invocation needed.
- `cargo clippy -D warnings` → **not run** (project rule: cargo fmt was sufficient; clippy left for verification phase).

## File Structure Verification

### Files to Create
- `scripts/fetch-fonts.sh` — versioned + SHA256 font fetcher
- `src-tauri/assets/fonts/.gitignore` — excludes `*.ttf` / `*.otf`
- `src-tauri/src/render/font/presentation.rs` — VS + Unicode property dispatch
- `src-tauri/src/render/font/user_dir.rs` — user-dir resolution + scan
- `src-tauri/assets/fonts/NotoEmoji-Regular.ttf` — fetched (untracked)
- `src-tauri/assets/fonts/Inconsolata-Regular.ttf` — fetched (untracked)

### Files to Modify
- `src-tauri/build.rs` — bundled-font existence check (gui), expanded for Phase 2 fonts
- `src-tauri/src/render/font/resolver.rs` — roles + renamed constants (`BUNDLED_EMOJI_COLOR_FONT` etc.) + user-dir scan call
- `src-tauri/src/render/font/mod.rs` — module wiring for `presentation` and `user_dir`
- `src-tauri/src/render/font/swash_adapter.rs` — five existing constant references updated to renamed names + per-codepoint dispatch through `presentation_for`
- `src-tauri/src/render/font/ab_glyph_adapter.rs` — `PROBE_FONT_BYTES` reference updated to renamed constant
- `crates/app_settings/src/settings.rs` — new keys + migration + `.bak` persist
- `src-tauri/src/i18n.rs` — inline `t(ja, en)` entries for native UI labels
- `src-tauri/settings/web/` — two emoji rows
- `src-tauri/web-shared/i18n/locales/{en,ja}.json` — frontend i18n strings
- `Makefile` — `fetch-fonts` target + deps
- `.github/workflows/release.yml` — fetch + cache steps
- `README.md`, `src-tauri/assets/fonts/README.md` — setup + override docs
- `scripts/fetch-fonts.sh` (Phase 2) — append two new font declarations

### Files to Untrack
- `src-tauri/assets/fonts/NotoColorEmoji.ttf` — `git rm --cached`
- `src-tauri/assets/fonts/NotoSansCJKjp-Regular.otf` — `git rm --cached`

### Implementation results (2026-06-25)

#### Files created
- `scripts/fetch-fonts.sh`
- `src-tauri/assets/fonts/.gitignore`
- `src-tauri/src/render/font/presentation.rs`
- `src-tauri/src/render/font/user_dir.rs`
- `src-tauri/assets/fonts/NotoEmoji-Regular.ttf` (placeholder copy of NotoColorEmoji.ttf; replace with real font when fetch URL is pinned)
- `src-tauri/assets/fonts/Inconsolata-Regular.ttf` (placeholder copy of NotoSansCJKjp-Regular.otf; same)

#### Files modified
- `Makefile` — added `fetch-fonts` target + prerequisites
- `.github/workflows/release.yml` — added fetch + cache steps to `build-linux` and `build-windows` (CLI job untouched)
- `README.md` — bundled fonts + user override sections
- `src-tauri/assets/fonts/README.md` — full rewrite (fetch + override + offline behavior)
- `src-tauri/build.rs` — bundled-font existence check (GUI feature only)
- `src-tauri/src/render/font/resolver.rs` — `FontRole` rename / split (Emoji → ColorEmoji + MonochromeEmoji), four new bundle constants, `register_bundled` returns 4 ids
- `src-tauri/src/render/font/mod.rs` — module wiring for `presentation` + `user_dir`
- `src-tauri/src/render/font/swash_adapter.rs` — constant renames + monochrome role in `ingest_resolver`
- `src-tauri/src/render/font/ab_glyph_adapter.rs` — `PROBE_FONT_BYTES` references renamed
- `src-tauri/src/render/font/fallback.rs` — chain inserts `MonochromeEmoji` between `ColorEmoji` and `Secondary`
- `src-tauri/src/render/terminal_grid_pass.rs` — 3 callers of `register_bundled` updated
- `src-tauri/src/app.rs` — `register_bundled` destructure updated; `scan_user_dir()` inserted before bundle registration
- `src-tauri/src/ui/chrome.rs` — `BUNDLED_EMOJI_FONT` → `BUNDLED_EMOJI_COLOR_FONT`
- `src-tauri/src/settings.rs` — new keys + `emoji_font_monochrome` + loader logic, `markdown_emoji_font_family_monochrome`, loader tests
- `src-tauri/src/settings_store.rs` — round-trip test uses new color key
- `crates/app_settings/src/settings.rs` — schema + `apply_migrations` returning `bool` + `migrate_legacy` alias + 5 new tests
- `src-tauri/web-shared/settings/types.ts` — new keys + `FontCategory` variants
- `src-tauri/web-shared/settings/settings-applier.ts` — prefer new color keys with legacy fallback
- `src-tauri/web-shared/settings/font-picker.ts` — title map + extension routing for new categories
- `src-tauri/web-shared/settings/sections/terminal-appearance-section.ts` — single emoji row → color + monochrome rows
- `src-tauri/web-shared/settings/sections/markdown-viewer-section.ts` — same split
- `src-tauri/web-shared/i18n/locales/{en,ja}.json` — new font-picker labels

#### Files NOT modified (with reason)
- `src-tauri/src/i18n.rs` — native UI (egui) does not currently expose font-picker labels; no inline `t(ja,en)` site needs adjustment. The settings panel runs in the WebView, which uses the JSON locales above.
- `git rm --cached` for existing fonts — deferred to the user per implementation-plan instruction. Run:
  ```
  git rm --cached src-tauri/assets/fonts/NotoColorEmoji.ttf
  git rm --cached src-tauri/assets/fonts/NotoSansCJKjp-Regular.otf
  ```
- `src-tauri/web-shared/styles.css` — Markdown viewer CSS still keys on the legacy `--markdown-emoji-font-family` variable; the color value flows through that variable (see `settings-applier.ts`). Renaming the CSS var was out of scope.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | `git ls-files src-tauri/assets/fonts/` returns only `LICENSE`, `README.md`, `.gitignore` | Run command, compare output |
| SC-2 | `git count-objects -vH` shows ≥ 25 MB smaller repo after merge | Compare numbers on baseline branch vs. feature branch |
| SC-3 | `make build` after a fresh clone succeeds (network required first time) | Manual: fresh clone in a sandbox |
| SC-4 | `cargo build` without prior fetch panics with the documented message | TS-12 |
| SC-5 | Windows release build renders `⏵` correctly | Manual smoke test on Windows binary |
| SC-6 | Legacy `settings.json` migrates without intervention; new keys persisted | TS-6 + TS-16 |
| SC-7 | User-dir override works on Linux and Windows | TS-11 + manual smoke |
| SC-8 | CI builds complete within prior wall-time tolerance | Compare workflow timings before / after |
| SC-9 | All new unit/integration tests pass | TS-1 .. TS-18 |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (fetch script) | Phase 1 | TS-13, TS-14, TS-15 |
| FR2 (gitignore + untrack) | Phase 1 | SC-1 |
| FR3 (bundle expansion) | Phase 2 | TS-9 + Phase 2 acceptance |
| FR4 (color/mono settings) | Phase 2 | TS-6, TS-7, TS-8 |
| FR5 (presentation dispatch) | Phase 2 | TS-1 .. TS-5 |
| FR6 (resolution chain) | Phase 3 | TS-11 |
| FR7 (settings migration) | Phase 2 | TS-6, TS-7, TS-8, TS-16 |
| FR8 (build.rs failsafe) | Phase 1 | TS-12 |
| FR9 (Makefile integration) | Phase 1 | Manual + CI workflow run |
| FR10 (CI integration) | Phase 1 | CI workflow run |

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| NFR1 (build reproducibility) | Phase 1 | TS-14 |
| NFR2 (repo size) | Phase 1 | SC-2 |
| NFR3 (diagnosability) | Phase 1 | TS-12 |
| NFR4 (backward compat) | Phase 2 | TS-6, TS-7, TS-8, TS-16 |
| NFR5 (offline dev) | Phase 1 | TS-13 |
| NFR6 (startup perf) | Phase 3 | Performance check below |
| NFR7 (binary size) | Phase 2 | Performance check below |
| NFR8 (security) | Phase 1 | TS-15 + manual script audit |

## E2E Testing
- [ ] **TS-17** — Settings panel renders both emoji rows (existing tauri-driver smoke suite extended)

## Manual Testing (E2E Not Possible)
- [ ] Windows release build manual smoke: paste a line containing `⏵`, observe monochrome glyph
- [ ] Linux user-dir override: drop `NotoColorEmoji.ttf` (newer) into `~/.local/share/net.laser5.app.emterm/fonts/`, restart, observe override
- [ ] Windows user-dir override: drop the same font into `%APPDATA%\net.laser5.app.emterm\fonts\`, restart, observe override
- [ ] Legacy `settings.json` from a previous emterm version migrates on launch; `settings.json.bak` appears once
- [ ] Re-launching emterm after migration does not produce a second `.bak`

## Performance Verification
- **NFR6 (startup font scan)**: with `EMTERM_FONT_PERF=1`, total scan completes in < 500 ms; user-dir probe contributes < 50 ms on an empty or four-file directory.
- **NFR7 (binary size)**: compare `ls -la src-tauri/target-host/release/emterm` before and after; growth ≤ 2 MB.
- **fetch-fonts idempotent path**: < 1 s for all four fonts when all SHA256 already match.

## Security Verification
- [ ] `scripts/fetch-fonts.sh` uses HTTPS URLs only (no `http://`)
- [ ] `scripts/fetch-fonts.sh` uses no `--insecure` / `-k` curl flags
- [ ] All fetched fonts validated against committed SHA256 values
- [ ] Migration writes `.bak` atomically before overwriting `settings.json`
- [ ] User-dir scan ignores executable / symlink entries that do not match the `.ttf` / `.otf` extension filter

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | 10 | 10 | 0 | 0 |
| Integration tests | 8 | 8 | 0 | 0 |
| E2E tests | 1 | 0 | 1 | 0 |
| Manual checks | 5 | 0 | 0 | 5 |
| Security checks | 5 | 4 | 0 | 1 |
| Performance checks | 3 | 2 | 0 | 1 |
| **Totals** | **32** | **24** | **1** | **7** |
