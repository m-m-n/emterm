# Implementation Plan: Font Bundle Redesign

## Overview
Bundle font binaries are removed from git and fetched via a SHA256-pinned shell script at build time. Two new fonts (Noto Emoji monochrome, Inconsolata) join the bundle, the emoji configuration splits into color/monochrome, and a four-tier font resolution chain (settings → user dir → system → bundle) is introduced.

## Objectives
- Decouple repository size and history hygiene from the font release cycle while keeping release builds byte-reproducible.
- Render text-presentation code points (e.g. U+23F5 `⏵`) correctly on Windows by adding a monochrome emoji fallback to the bundle.
- Preserve existing user settings through an automatic migration to the new color/monochrome keys.

## Prerequisites

### Development Environment
- Rust toolchain pinned by the project (see `rust-toolchain.toml`).
- Bun (TypeScript bundler for child WebView assets — unchanged by this feature).
- `curl` and `sha256sum` available on the host (Linux dev, CI runner, and Git for Windows ship these).

### Dependencies
- Existing `fontdb`, `swash`, `zeno` integration in `src-tauri/src/render/font/`.
- Existing `crates/app_settings` schema and migration helpers (`deserialize_null_*`).
- Existing GitHub Actions release workflow.

## Architecture Overview

### Technology Stack
- **Language**: Rust 2024 edition (crate `emterm` and workspace crates) + Bun-bundled TypeScript (only for the settings panel UI strings).
- **Framework**: native winit + wgpu + swash for the terminal, wry for child WebViews. No framework changes in this feature.
- **Key Libraries**: `fontdb` (system + user-dir scanning), `swash` (rasterization), `serde` (settings migration), POSIX shell + `curl` + `sha256sum` (build-time fetch).

### Design Approach
The redesign keeps the existing single-resolver model and layers new responsibilities into it:
1. **Build-time provisioning** is moved out of git into a versioned script that satisfies `include_bytes!` requirements before compilation.
2. **Font roles** gain a binary split between color and monochrome emoji so per-glyph presentation can dispatch.
3. **Font resolution** introduces an additional user-dir probe ahead of the existing system + bundle layers.
4. **Settings migration** runs at deserialization, persists once, and stays idempotent on subsequent loads.

### Component Interaction
```
build.rs ── checks files exist ──> include_bytes! ──> resolver bundle constants
   ↑                                                       │
fetch-fonts.sh (CI / dev)                                  ▼
   │                                                  ResolverChain
   └─> fonts/ (gitignored)                  (settings > user > system > bundle)
                                                            │
                                                            ▼
                                              presentation dispatch (VS / Unicode)
                                                            │
                                                            ▼
                                                   swash rasterizer
```

Settings flow:
```
settings.json ─> serde deserialize ─> migrate_legacy() ─> persist (.bak) ─> AppSettings
```

## Implementation Phases

### Phase 1: Font fetch infrastructure

**Goal**: Remove existing font binaries from git, introduce SHA256-pinned fetch tooling, and make `make build` fail fast with actionable guidance when fonts are absent.

**Files to Create**:
- `scripts/fetch-fonts.sh` — Versioned + SHA256-checked downloader for bundled fonts.
- `src-tauri/assets/fonts/.gitignore` — Excludes `*.ttf` / `*.otf` while keeping `LICENSE`, `README.md`, and itself.

**Files to Modify**:
- `src-tauri/build.rs` — Pre-compile existence check for required font files (gated on `gui` feature).
- `Makefile` — Add `fetch-fonts` target and wire it as a prerequisite of `setup`, `dev`, `build`, `dpkg`. `cli-build` and `cli-dpkg` deliberately skip the dependency.
- `.github/workflows/release.yml` — Insert a fetch step and an `actions/cache` step keyed on the script's hash before any build step that needs bundled fonts.
- `README.md` — Add setup paragraph pointing developers at `make fetch-fonts`.
- `src-tauri/assets/fonts/README.md` — Explain that the directory is now populated by the fetch script and how to regenerate it.

**Files to Untrack**:
- `src-tauri/assets/fonts/NotoColorEmoji.ttf`
- `src-tauri/assets/fonts/NotoSansCJKjp-Regular.otf`

Both are removed from the index via `git rm --cached` in a single commit. Their on-disk copies remain (already-built developer machines stay functional).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `fetch-fonts.sh` | Download bundled fonts from pinned URLs, verify SHA256, place under `src-tauri/assets/fonts/` | HTTPS reachable OR target files already present and hash-matched | All declared font files exist with the pinned SHA256, OR the script exits non-zero with an actionable error |
| `build.rs` font-check | Fail compile when any required font is missing | `gui` feature enabled | Either every declared font exists OR `cargo build` aborts with a message naming the missing file and the recovery command |
| `.gitignore` (fonts) | Keep font binaries out of git | None | `git status` does not surface font binaries; `git ls-files` returns only `LICENSE`, `README.md`, `.gitignore` |
| Makefile prerequisites | Guarantee fonts exist before any GUI build target runs | Make invoked at project root | Targets needing bundled fonts run `fetch-fonts` first; CLI-only targets remain network-free |

**Processing Flow** (script-side):
1. For each declared font (name, URL, expected SHA256):
   - If file exists and its SHA256 equals the expected value → mark up-to-date, continue.
   - Otherwise → download to a temp path, verify SHA256, atomically move to the final path.
2. If any download fails or hash mismatches → emit a clear error to stderr and exit non-zero before leaving behind partial files.

**Implementation Steps** (max 7):
1. **Draft `fetch-fonts.sh` skeleton** — POSIX-friendly `bash` with `set -euo pipefail`, helper `fetch_one(name, url, sha256)`, idempotent skip-on-match logic.
2. **Pin the two existing fonts** — Resolve the upstream GitHub Releases tags for Noto Color Emoji and Noto Sans CJK JP that match the bytes currently committed (verify by comparing SHA256 against the local copies), embed URL + SHA256 in the script.
3. **Add gitignore + untrack** — Create `src-tauri/assets/fonts/.gitignore`, `git rm --cached` the two font files, leave them on disk.
4. **Wire `build.rs` failsafe** — Inside the `gui` feature branch, assert each required font path exists; on failure, panic with a single message naming the file and the recovery command.
5. **Wire Makefile** — Add the new target, make it a prerequisite of the relevant GUI targets, keep CLI targets untouched.
6. **Update CI workflows** — Insert fetch + cache steps in every workflow job that builds the GUI binary; CLI-only jobs stay unchanged.
7. **Document** — Update top-level README and `assets/fonts/README.md`; mention offline behavior and the recovery command.

**Dependencies**: Blocks Phase 2 (which extends the script with new fonts and the bundle with new constants).

**Testing Approach**:
- Unit: not applicable (no compiled code added).
- Integration: script-level idempotency check (re-run reports up-to-date); deliberately corrupt a file and confirm re-run repairs it.
- E2E: existing build pipeline runs end-to-end after fetch in CI.
- Manual: developer fresh clone → `make setup && make build` succeeds.

**Acceptance Criteria**:
- [ ] `git ls-files src-tauri/assets/fonts/` does not include any `*.ttf` or `*.otf`.
- [ ] Running `make fetch-fonts` against a populated directory exits 0 and reports each font as up-to-date.
- [ ] Deleting one font and running `cargo build` (gui) fails with the documented missing-font panic message.
- [ ] Deleting one font and running `make build` succeeds (fetch runs automatically).
- [ ] All existing CI workflows still pass.

**Estimated Effort**: small.

---

### Phase 2: Bundle expansion + color/monochrome split

**Goal**: Add Noto Emoji (monochrome) and Inconsolata to the bundle, split the emoji configuration into color/monochrome, and route per-glyph presentation through a new dispatch helper.

**Files to Create**:
- `src-tauri/src/render/font/presentation.rs` — Pure-Rust helper exposing a `presentation_for(codepoint, variation_selector)` function and the inline emoji-property tables it consults.
- `src-tauri/assets/fonts/NotoEmoji-Regular.ttf` — Fetched by the script (gitignored on disk).
- `src-tauri/assets/fonts/Inconsolata-Regular.ttf` — Fetched by the script (gitignored on disk).

**Files to Modify**:
- `scripts/fetch-fonts.sh` — Add two new declarations (URL + SHA256).
- `src-tauri/src/render/font/resolver.rs` — Add new bundle constants, rename `FontRole::Emoji` to `FontRole::ColorEmoji`, add `FontRole::MonochromeEmoji`, extend `register_bundled()` to register the four roles. Rename `BUNDLED_EMOJI_FONT` to `BUNDLED_EMOJI_COLOR_FONT` and add `BUNDLED_EMOJI_MONO_FONT` / `BUNDLED_BASE_FONT`.
- `src-tauri/src/render/font/mod.rs` — Wire the new `presentation` module, expose its dispatch helper to callers.
- `src-tauri/src/render/font/swash_adapter.rs` — Update the five existing references to `BUNDLED_EMOJI_FONT` / `BUNDLED_CJK_FONT` to use the renamed constants and consult `presentation_for(...)` for per-codepoint dispatch (color vs monochrome) with opposite-side fallback before tofu.
- `src-tauri/src/render/font/ab_glyph_adapter.rs` — Update `PROBE_FONT_BYTES` reference to the renamed `BUNDLED_EMOJI_COLOR_FONT` constant.
- `crates/app_settings/src/settings.rs` — Introduce new keys, mark the legacy keys as deserialization-only sinks (via `serde(default)` aliases), add `migrate_legacy()` and persist-on-migrate behavior, update validation, update `Default`.
- `src-tauri/src/i18n.rs` — Add inline `t(ja, en)` entries for the two new emoji font labels (native egui UI strings; no separate JSON locale file is used backend-side per the project's i18n design).
- `src-tauri/settings/web/` (TypeScript settings panel) — Add two new font-picker rows (color, monochrome) under the existing emoji section, remove the single-row form.
- `src-tauri/web-shared/i18n/locales/{en,ja}.json` — UI strings for the new rows (frontend i18n, used by the settings WebView).
- `src-tauri/build.rs` — Extend the existence check to cover the two new fonts.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `presentation_for(cp, vs)` | Decide whether a code point should be drawn with the color or monochrome emoji font | `cp` is a Unicode scalar; `vs` is `None`, VS15, or VS16 | Returns `Color`, `Monochrome`, or `NotEmoji`; deterministic and pure |
| Emoji-property table | Provide the `Emoji` and `Emoji_Presentation` property bits per code point | Compiled-in static table (no I/O) | Constant-time / log-n lookup |
| `FontRole::ColorEmoji` / `MonochromeEmoji` | Identify which bundled / system entry is which side of the emoji split | Resolver registry initialized | Two distinct ids per side, with bundled fallbacks |
| `AppSettings::migrate_legacy()` | Move legacy `font_family_emoji` / `markdown_emoji_font_family` values into the new color keys | Called once after deserialization | Returns `true` if any migration happened; caller persists `.bak` + new schema |
| Bundle constants | Provide bytes for CJK, color emoji, mono emoji, base monospace | `gui` feature enabled, files present on disk at compile time | Four `&'static [u8]` slices available to the resolver |

**Processing Flow** (renderer dispatch for one code point):
1. Compute `role = presentation_for(cp, vs)`.
   - `Color` → emoji-color chain.
   - `Monochrome` → emoji-mono chain.
   - `NotEmoji` → existing Base / CJK chain.
2. For each entry in the chosen chain (settings-supplied > user dir > system > bundle):
   - If the entry has a glyph for `cp` → rasterize and return.
3. If the emoji side returns no glyph, attempt the opposite emoji side once before declaring tofu.

**Processing Flow** (settings load):
1. Deserialize settings via serde.
2. Call `migrate_legacy()`.
   - If `true` → copy current file to `settings.json.bak`, then rewrite the file with the new schema.
   - If `false` → no-op (idempotent on subsequent launches).

**Implementation Steps** (max 7):
1. **Build the Unicode property tables** — Generate compile-time sorted arrays of code-point ranges for `Emoji=Yes` and `Emoji_Presentation=Yes` from the upstream Unicode data. Tables live inline in `presentation.rs` so no new crate dependency is introduced.
2. **Implement `presentation_for`** — VS16 forces color, VS15 forces monochrome, otherwise look up `Emoji_Presentation` then `Emoji`. Unit-tested per code-point class.
3. **Extend the resolver + adapter touch-points** — Add the four bundled byte slices, rename existing constants (`BUNDLED_EMOJI_FONT` → `BUNDLED_EMOJI_COLOR_FONT`), split the role enum, update `register_bundled()` to register and return all four ids, update the five references in `swash_adapter.rs` and the one in `ab_glyph_adapter.rs`, keep family-name uniqueness (`(bundled)` suffix as today).
4. **Route the swash adapter through the dispatch** — At each per-codepoint fallback decision, consult `presentation_for(...)` to pick the emoji role; preserve the opposite-side fallback before tofu.
5. **Update settings schema + migration** — Add the four new fields with serde defaults, accept the legacy keys as deserialization-only aliases, implement `migrate_legacy()`, wire `.bak` + persist at the loader site, refresh validation. Update inline `t(ja, en)` entries in `src-tauri/src/i18n.rs` for native UI labels and `src-tauri/web-shared/i18n/locales/{en,ja}.json` for the settings WebView.
6. **Update the settings panel UI** — Replace the single emoji-font row with two rows; reuse the existing font-picker component; ensure the picker filters/labels work for both color and monochrome lists.
7. **Update fetch + build wiring** — Add the two new entries to `fetch-fonts.sh` and extend `build.rs`'s existence check.

**Dependencies**: Requires Phase 1 (fetch + ignore + failsafe). Blocks Phase 3 only for the user-dir chain extension (Phase 3 still slots above the bundle layer this phase establishes).

**Testing Approach**:
- Unit: `presentation_for` across VS16, VS15, `Emoji_Presentation=Yes`, `Emoji=Yes ∧ Emoji_Presentation=No` (incl. U+23F5), ASCII (NotEmoji).
- Unit: `AppSettings::migrate_legacy()` idempotency, new-schema files left untouched, legacy-only files migrated, mixed files have new keys win.
- Integration: settings loader call site persists `.bak` and rewrites `settings.json` on first migration.
- Integration: resolver returns four distinct ids and bundle bytes are non-empty.
- E2E: existing UI smoke test covers settings panel rendering of the two new rows.
- Manual: launch the Windows release build; paste a line containing `⏵`; verify monochrome glyph appears.

**Acceptance Criteria**:
- [ ] `presentation_for('\u{23F5}', None) == Monochrome`.
- [ ] `presentation_for('\u{23F5}', Some('\u{FE0F}')) == Color`.
- [ ] `presentation_for('\u{1F600}', None) == Color`.
- [ ] `presentation_for('\u{1F600}', Some('\u{FE0E}')) == Monochrome`.
- [ ] `Resolver::register_bundled()` returns four distinct ids; each carries non-empty bytes.
- [ ] Loading a legacy `settings.json` produces a `.bak` and a new-schema file; reloading the new file does not produce a second `.bak`.
- [ ] Windows release build renders `⏵` (manual confirmation).

**Estimated Effort**: medium.

---

### Phase 3: User directory override

**Goal**: Honor a user-supplied font directory ahead of the system + bundle layers so fonts can be overridden without re-releasing the app.

**Files to Create**:
- `src-tauri/src/render/font/user_dir.rs` — Computes the user font directory path and exposes a scan helper that registers eligible `*.ttf` / `*.otf` files into the resolver under `FontRole::User`.

**Files to Modify**:
- `src-tauri/src/render/font/resolver.rs` — Insert a `scan_user_dir()` call into the resolver build sequence ahead of `scan_system_fonts()`, document the chain order.
- `src-tauri/src/render/font/mod.rs` — Expose the new module.
- `README.md` and / or `src-tauri/assets/fonts/README.md` — Explain how to drop overrides into the user directory.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `user_font_dir()` | Return platform-specific user font path | Linux or Windows host | Returns the resolved path or `None` if neither platform var is reachable |
| `Resolver::scan_user_dir()` | Register `*.ttf` / `*.otf` files in the directory as `FontRole::User` | Resolver mutable, path resolvable | Registered families are queryable; non-font / unreadable files are logged once and skipped |
| Resolution chain | Ensure `User` role wins over `System` and `bundled` family-name lookups | `scan_user_dir()` ran before `scan_system_fonts()` | A user override for a family present in both wins |

**Processing Flow**:
1. Resolver build sequence:
   - Register bundled fonts (Phase 2).
   - Call `scan_user_dir()` (this phase).
   - Call `scan_system_fonts()` (existing).
2. `scan_user_dir()` enumerates one directory level (no recursion).
   - For each entry whose extension is `.ttf` or `.otf` → read bytes, register under `FontRole::User` with the family name reported by fontdb.
   - For other entries → ignore.
   - If `read_dir` errors → log a single warn line and return.
3. Family-name lookup walks `User` before `System` before `Base/CJK/ColorEmoji/MonochromeEmoji` bundle entries.

**Implementation Steps** (max 7):
1. **Resolve platform paths** — Linux: `XDG_DATA_HOME/net.laser5.app.emterm/fonts/` with fallback `~/.local/share/...`; Windows: `%APPDATA%/net.laser5.app.emterm/fonts/`. Share path resolution with the existing settings-path helper.
2. **Implement `scan_user_dir()`** — Single-level enumeration, extension filter, eager byte read, registration via the existing `register_bytes` API.
3. **Insert the scan into the resolver build flow** — Run after bundled registration, before system scan, so user-dir entries appear ahead of system in lookup order.
4. **Add precedence checks** — Family-name lookup must consult the `User` role first.
5. **Document** — README note pointing users at the override path and noting per-platform locations.

**Dependencies**: Requires Phase 2 (resolver split into color/mono); standalone otherwise.

**Testing Approach**:
- Unit: `user_font_dir()` resolves to expected Linux and Windows paths under fixture env vars.
- Unit: `Resolver::scan_user_dir()` against a tempdir with one valid `.ttf`, one `.otf`, one `.txt`, and one corrupt font; resolver registers only the two real fonts.
- Integration: place a known font with a family name that overlaps a bundled one; family lookup returns the user copy.
- Integration: empty directory and missing directory both result in zero registrations and no warn-storm.
- E2E: skip (no UI surface).
- Manual: drop a font on Linux and Windows; restart emterm; observe the override in the terminal.

**Acceptance Criteria**:
- [ ] User-dir scan registers only `.ttf` / `.otf`.
- [ ] User-dir entries beat system + bundle on family-name lookup.
- [ ] Missing or empty user dir produces no warn lines.
- [ ] Corrupt font in user dir produces exactly one warn line and is skipped.

**Estimated Effort**: small.

---

## Complete File Structure

```
emterm/
├── Makefile                                     # MOD: fetch-fonts target + deps
├── README.md                                    # MOD: setup paragraph
├── scripts/
│   └── fetch-fonts.sh                           # NEW
├── .github/
│   └── workflows/
│       └── (release / build configs)            # MOD: fetch + cache steps
├── src-tauri/
│   ├── build.rs                                 # MOD: bundled-font existence check
│   ├── assets/
│   │   └── fonts/
│   │       ├── .gitignore                       # NEW
│   │       ├── LICENSE                          # (unchanged)
│   │       ├── README.md                        # MOD: explains fetch + override
│   │       ├── NotoColorEmoji.ttf               # untracked (fetched)
│   │       ├── NotoSansCJKjp-Regular.otf        # untracked (fetched)
│   │       ├── NotoEmoji-Regular.ttf            # NEW, untracked (fetched)
│   │       └── Inconsolata-Regular.ttf          # NEW, untracked (fetched)
│   ├── settings/
│   │   └── web/                                 # MOD: two emoji rows
│   ├── web-shared/
│   │   └── i18n/locales/                        # MOD: frontend UI strings for new rows
│   │       ├── en.json
│   │       └── ja.json
│   └── src/
│       ├── i18n.rs                              # MOD: inline t(ja, en) for native UI labels
│       └── render/
│           └── font/
│               ├── mod.rs                       # MOD: re-exports presentation + user_dir
│               ├── resolver.rs                  # MOD: roles + renamed constants + user-dir scan
│               ├── swash_adapter.rs             # MOD: constant renames + per-codepoint dispatch
│               ├── ab_glyph_adapter.rs          # MOD: PROBE_FONT_BYTES renamed constant
│               ├── presentation.rs              # NEW
│               └── user_dir.rs                  # NEW
├── crates/
│   └── app_settings/
│       └── src/
│           └── settings.rs                      # MOD: new keys + migrate_legacy
└── doc/
    └── tasks/
        └── font-bundle-redesign/                # this directory
```

## Testing Strategy
- **Unit**: Pure functions (`presentation_for`, `migrate_legacy`, `user_font_dir`) reach ≥ 95 % branch coverage. Resolver register/scan paths covered with synthetic byte buffers.
- **Integration**: Settings load → migrate → persist cycle exercised in a temp dir. Resolver chain order asserted via family-name lookup.
- **E2E**: Existing tauri-driver smoke suite extended with one assertion that the settings panel renders the two emoji rows.
- **Manual**: Windows release build for `⏵` glyph confirmation; Linux + Windows user-dir override smoke.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `fontdb` | existing (workspace) | System + user-dir font enumeration |
| `swash` | existing (workspace) | Glyph rasterization |
| `serde` | existing (workspace) | Settings deserialization + migration |
| `curl` + `sha256sum` | host | Build-time font fetch + integrity check |
| `actions/cache` | v4 | CI cache keyed on fetch-fonts.sh hash |

No new Cargo dependencies are introduced.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Upstream GitHub Release tag goes missing | low | high | Pin via tag (permanent on GitHub) and document a self-host backup in `assets/fonts/README.md` |
| SHA256 of currently-committed binaries does not match any upstream release artifact | medium | medium | If matches fail, fetch the upstream asset, treat that as the new baseline, document the choice in the script header |
| Migration corrupts user settings on edge files (BOM, partial JSON) | low | high | `.bak` written before rewrite; validation runs after migration; failure path keeps original file untouched |
| User-dir scan slows startup beyond the 500 ms budget | low | medium | Single-level scan, no recursion; instrument under `EMTERM_FONT_PERF=1` |
| Inline emoji property table goes stale with Unicode releases | medium | low | Document the source revision in `presentation.rs` header; budget a refresh cadence aligned with Unicode major versions |
| Settings panel UI string keys collide with existing ones | low | low | Namespace new keys explicitly (e.g. `font.emoji.color`, `font.emoji.monochrome`) |

## Open Questions
All Open Questions were resolved to assumptions in `sdd.yaml`:
- OQ1 (FR1): pinned tags + SHA256 will be selected during Phase 1 step 2; values committed to `fetch-fonts.sh`.
- OQ2 (FR5): inline emoji property table (no new crate dependency); source Unicode revision documented in `presentation.rs`.
- OQ3 (FR7): `.bak` file is written on first migration.

## Success Metrics
- [ ] Repository size reduction ≥ 25 MB measured via `git count-objects -vH` against a fresh clone after merge.
- [ ] All Phase 1–3 acceptance criteria pass.
- [ ] No regression in existing terminal font tests.
- [ ] Resolver startup time stays under 500 ms with `EMTERM_FONT_PERF=1`.
- [ ] GUI binary size grows by no more than 2 MB.
