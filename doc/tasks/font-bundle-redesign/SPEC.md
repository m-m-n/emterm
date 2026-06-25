# Feature: Font Bundle Redesign

## Overview

Remove bundled font binaries from git, fetch them via a versioned + SHA256-pinned script during build, add Noto Emoji (monochrome) and Inconsolata to the bundle set, split the emoji font configuration into color/monochrome, and introduce a four-tier font resolution chain (settings > user dir > system > bundle). The redesign solves the Windows tofu-glyph problem for `⏵` (U+23F5) and decouples the font release cycle from the emterm release cycle.

## Objectives

- Eliminate ~27 MB of font binaries from the git repository while preserving reproducible release builds.
- Render text-presentation code points (e.g. `⏵` U+23F5) correctly on Windows by adding Noto Emoji (monochrome) to the bundle and selecting it for text-presentation contexts.
- Allow users to override individual fonts (especially color emoji) without waiting for an emterm release.
- Preserve all existing user settings via automatic key migration.

## User Stories

### US1: Developer first-time setup
As an emterm developer, I want `make build` to succeed immediately after a fresh clone, so that I do not need to manually fetch fonts.

**Acceptance Criteria:**
- [ ] `make setup` or `make fetch-fonts` populates `src-tauri/assets/fonts/` with all four bundled fonts.
- [ ] `make build` triggered without prior fetch first downloads fonts, then builds.
- [ ] Re-running `make fetch-fonts` with up-to-date files is a no-op (idempotent).

### US2: Windows end-user sees `⏵`
As a Windows emterm user running Claude Code, I want `⏵` (auto mode indicator) to render correctly, so that I am not staring at tofu.

**Acceptance Criteria:**
- [ ] Release build on Windows displays U+23F5 using the bundled Noto Emoji (monochrome) fallback.
- [ ] VS16-attached emoji code points still render via Noto Color Emoji.

### US3: End-user font override
As an emterm user, I want to drop a font file into a known directory and have emterm pick it up over the bundled copy, so that I can use the latest Noto Color Emoji without waiting for an emterm release.

**Acceptance Criteria:**
- [ ] Placing `NotoColorEmoji.ttf` under the user-dir path causes emterm to use it instead of the bundled copy on next launch.
- [ ] Removing the override falls back to the bundled copy.

### US4: Existing settings preservation
As an existing emterm user, I want my `font_family_emoji` value to survive the upgrade, so that I do not need to reconfigure.

**Acceptance Criteria:**
- [ ] Legacy `font_family_emoji` value is migrated to `font_family_emoji_color`.
- [ ] Legacy `markdown_emoji_font_family` value is migrated to `markdown_emoji_font_family_color`.
- [ ] `*_monochrome` keys initialize to their defaults.
- [ ] `settings.json` is rewritten with the new schema after migration.

### US5: Build reproducibility
As a release engineer, I want every CI build to produce byte-identical font assets, so that supply-chain auditing remains feasible.

**Acceptance Criteria:**
- [ ] Each fetched font is verified against a pinned SHA256 hash.
- [ ] Hash mismatch aborts the build with a clear error.

## Technical Requirements

### Functional Requirements

- **FR1:** A `scripts/fetch-fonts.sh` shell script downloads each font from a fixed HTTPS URL pinned to an upstream Git tag (a GitHub Releases asset URL or a `raw.githubusercontent.com` tag ref) and verifies SHA256 before placing the file under `src-tauri/assets/fonts/`. Idempotent: skip files whose SHA256 already matches.
- **FR2:** Remove `NotoColorEmoji.ttf` and `NotoSansCJKjp-Regular.otf` from git history; add `src-tauri/assets/fonts/.gitignore` excluding `*.ttf` and `*.otf` while retaining `LICENSE` and `README.md`.
- **FR3:** Add Noto Emoji (monochrome) and Inconsolata to the bundled font set. Both are embedded via `include_bytes!` under the `gui` feature gate only.
- **FR4:** Split the emoji font configuration into color and monochrome keys:
    - `font_family_emoji_color` (default: `Noto Color Emoji`)
    - `font_family_emoji_monochrome` (default: `Noto Emoji`)
    - `markdown_emoji_font_family_color` (default: `Noto Color Emoji`)
    - `markdown_emoji_font_family_monochrome` (default: `Noto Emoji`)
- **FR5:** Implement presentation-aware emoji selection in the renderer:
    - `<codepoint> + U+FE0F (VS16)` → color emoji font
    - `<codepoint> + U+FE0E (VS15)` → monochrome emoji font
    - Bare codepoint with Unicode `Emoji_Presentation=Yes` → color
    - Bare codepoint with `Emoji=Yes` but `Emoji_Presentation=No` (text-default, e.g. U+23F5) → monochrome
    - When the chosen side has no glyph, fall back to the opposite side before declaring a tofu glyph.
- **FR6:** Font resolution chain (highest priority first):
    1. Settings-supplied font path (absolute path explicitly configured)
    2. User directory: `~/.local/share/net.laser5.app.emterm/fonts/` (Linux) or `%APPDATA%\net.laser5.app.emterm\fonts\` (Windows)
    3. System fonts via fontdb
    4. Bundled fonts
- **FR7:** Auto-migrate legacy settings keys at deserialization:
    - `font_family_emoji` → `font_family_emoji_color`
    - `markdown_emoji_font_family` → `markdown_emoji_font_family_color`
    - Initialize `*_monochrome` keys to their defaults.
    - Persist the migrated schema back to `settings.json`.
- **FR8:** `build.rs` (under `gui` feature) verifies that each bundled font file exists before allowing the build to proceed. On absence, panic with a message instructing the user to run `make fetch-fonts`.
- **FR9:** Makefile targets `setup`, `dev`, `build`, `dpkg`, `cli-dpkg` (where they need bundled fonts) depend on a new `fetch-fonts` target; `cli-build` does not.
- **FR10:** GitHub Actions workflows that build release artifacts invoke `bash scripts/fetch-fonts.sh` before the build step, with `actions/cache` keyed on the script's SHA256 sum.

### Non-Functional Requirements

- **NFR1 - Build Reproducibility:** SHA256 verification guarantees that the embedded font bytes are byte-identical to the bytes served by the pinned upstream tag.
- **NFR2 - Repository Size:** Net repository size reduction of ~30 MB (current font binaries removed, plus future avoidance of binary diffs on updates).
- **NFR3 - Diagnosability:** Missing-font failures during build must name the missing file and reference `make fetch-fonts` / `bash scripts/fetch-fonts.sh`.
- **NFR4 - Backward Compatibility:** Existing `settings.json` files load without user intervention; auto-migration preserves prior values for the color side.
- **NFR5 - Offline Development:** After a successful fetch, subsequent builds (including `cargo build --release`) require no network access. Re-running `fetch-fonts.sh` while offline succeeds when all files are already present and SHA256-matched.
- **NFR6 - Startup Performance:** Font resolver scan remains within the existing NFR (< 500 ms with `EMTERM_FONT_PERF=1` observable). The added user-directory probe must contribute < 50 ms in the typical case (empty directory or 1–4 files).
- **NFR7 - Binary Size:** GUI binary size increase from adding Noto Emoji + Inconsolata ≤ 2 MB.
- **NFR8 - Security:** Only HTTPS download URLs. SHA256 checks are mandatory; no `--insecure` / `-k` curl flags.

## Implementation Approach

### Architecture

**System layers (font handling):**
```
┌─────────────────────────────────────────────────┐
│  Settings (AppSettings / serde)                 │
│   - font_family_emoji_{color,monochrome}        │
│   - migrate legacy keys                          │
├─────────────────────────────────────────────────┤
│  Renderer (term_core grid + swash adapter)       │
│   - per-codepoint VS15/VS16 presentation logic   │
├─────────────────────────────────────────────────┤
│  Font Resolver (render/font/resolver.rs)         │
│   - chain: settings > user dir > system > bundle │
│   - FontRole::ColorEmoji / MonochromeEmoji       │
├─────────────────────────────────────────────────┤
│  Bundled bytes (include_bytes!)                  │
│   - NotoSansCJKjp, NotoColorEmoji, NotoEmoji,    │
│     Inconsolata (under #[cfg(feature="gui")])    │
├─────────────────────────────────────────────────┤
│  Build-time (build.rs + scripts/fetch-fonts.sh)  │
│   - fetch (HTTPS + SHA256) → assets/fonts/       │
│   - build.rs existence check                     │
└─────────────────────────────────────────────────┘
```

### Data Flow

**Fetch flow (developer / CI):**
```
make fetch-fonts
  → scripts/fetch-fonts.sh
     for each font in (NotoColorEmoji, NotoSansCJKjp, NotoEmoji, Inconsolata):
        if local SHA256 != expected:
           curl --fail -sSL <url> -o <tmp>
           verify SHA256(<tmp>) == expected
           mv <tmp> src-tauri/assets/fonts/<file>
        else skip
```

**Resolution flow (runtime):**
```
Glyph request (codepoint, vs?, role)
  → role := presentation_role(codepoint, vs)        # ColorEmoji / MonochromeEmoji / Base / Cjk
  → for font in chain(role):
       if font.has_glyph(codepoint): return font.rasterize(codepoint)
  → fall back to opposite emoji side, then tofu
```

### File Structure

```
src-tauri/
├── assets/
│   └── fonts/
│       ├── .gitignore              # NEW: excludes *.ttf / *.otf
│       ├── LICENSE                 # (git-tracked)
│       ├── README.md               # UPDATED: setup instructions
│       ├── NotoColorEmoji.ttf      # gitignored, fetched
│       ├── NotoSansCJKjp-Regular.otf  # gitignored, fetched
│       ├── NotoEmoji-Regular.ttf   # NEW, gitignored, fetched
│       └── Inconsolata-Regular.ttf # NEW, gitignored, fetched
├── build.rs                        # UPDATED: assert font files exist (gui)
├── Cargo.toml                      # UPDATED: bundle constants
└── src/
    └── render/
        └── font/
            ├── resolver.rs         # UPDATED: ColorEmoji + MonochromeEmoji roles,
            │                       # user-dir probe, new bundle constants
            └── presentation.rs     # NEW: VS15/VS16 + Unicode property dispatch

crates/
└── app_settings/
    └── src/
        └── settings.rs             # UPDATED: new keys + migration

scripts/
└── fetch-fonts.sh                  # NEW

.github/
└── workflows/
    └── (release / build)           # UPDATED: fetch-fonts step + cache

Makefile                            # UPDATED: fetch-fonts target + deps
```

### Settings Schema

```rust
// crates/app_settings/src/settings.rs (excerpt, illustrative)

#[derive(Serialize, Deserialize)]
pub struct AppSettings {
    // ... existing fields ...

    #[serde(default = "default_font_family_emoji_color",
            deserialize_with = "deserialize_null_emoji_color")]
    pub font_family_emoji_color: String,

    #[serde(default = "default_font_family_emoji_monochrome",
            deserialize_with = "deserialize_null_emoji_monochrome")]
    pub font_family_emoji_monochrome: String,

    #[serde(default = "default_markdown_emoji_color")]
    pub markdown_emoji_font_family_color: String,

    #[serde(default = "default_markdown_emoji_monochrome")]
    pub markdown_emoji_font_family_monochrome: String,

    // legacy keys consumed only during migration; not serialized back
    #[serde(default, skip_serializing)]
    pub(crate) legacy_font_family_emoji: Option<String>,

    #[serde(default, skip_serializing)]
    pub(crate) legacy_markdown_emoji_font_family: Option<String>,
}

fn default_font_family_emoji_color() -> String { "Noto Color Emoji".into() }
fn default_font_family_emoji_monochrome() -> String { "Noto Emoji".into() }
fn default_markdown_emoji_color() -> String { "Noto Color Emoji".into() }
fn default_markdown_emoji_monochrome() -> String { "Noto Emoji".into() }
```

Migration logic runs after deserialization:
```rust
impl AppSettings {
    pub fn migrate_legacy(&mut self) -> bool {
        let mut migrated = false;
        if let Some(v) = self.legacy_font_family_emoji.take() {
            if self.font_family_emoji_color == default_font_family_emoji_color() {
                self.font_family_emoji_color = v;
            }
            migrated = true;
        }
        if let Some(v) = self.legacy_markdown_emoji_font_family.take() {
            if self.markdown_emoji_font_family_color == default_markdown_emoji_color() {
                self.markdown_emoji_font_family_color = v;
            }
            migrated = true;
        }
        migrated
    }
}
```

Caller persists when `migrate_legacy()` returns `true`.

### Font Resolver Updates

```rust
// src-tauri/src/render/font/resolver.rs (excerpt)

pub const BUNDLED_CJK_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansCJKjp-Regular.otf");
pub const BUNDLED_EMOJI_COLOR_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoColorEmoji.ttf");
pub const BUNDLED_EMOJI_MONO_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoEmoji-Regular.ttf");
pub const BUNDLED_BASE_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/Inconsolata-Regular.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontRole {
    Base,
    Cjk,
    ColorEmoji,       // RENAMED from Emoji
    MonochromeEmoji,  // NEW
    Secondary,
    User,
}
```

`Resolver::register_bundled()` returns four ids:
```rust
pub fn register_bundled(&mut self)
    -> (FontId /*cjk*/, FontId /*color*/, FontId /*mono*/, FontId /*base*/);
```

The user-directory probe runs before the system scan:
```rust
pub fn scan_user_dir(&mut self) {
    let dir = match user_font_dir() { Some(d) => d, None => return };
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
            if matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf") {
                // register under FontRole::User; family name discovered via fontdb
            }
        }
    }
}
```

### Presentation Dispatch

```rust
// src-tauri/src/render/font/presentation.rs (NEW)

pub enum EmojiPresentation { Color, Monochrome, NotEmoji }

pub fn presentation_for(codepoint: char, variation_selector: Option<char>) -> EmojiPresentation {
    match variation_selector {
        Some('\u{FE0F}') => EmojiPresentation::Color,
        Some('\u{FE0E}') => EmojiPresentation::Monochrome,
        _ => {
            if has_emoji_presentation_default(codepoint) {
                EmojiPresentation::Color
            } else if is_emoji(codepoint) {
                EmojiPresentation::Monochrome
            } else {
                EmojiPresentation::NotEmoji
            }
        }
    }
}
```

`has_emoji_presentation_default` and `is_emoji` read Unicode emoji-data tables (already present via existing emoji-related crates such as `unicode-segmentation` or a hand-rolled inline table; spec leaves the data source open for the implementation plan).

### `scripts/fetch-fonts.sh`

Shell language: POSIX-ish `bash` (already used by `scripts/build-dpkg.sh`).

Skeleton (illustrative):
```bash
#!/usr/bin/env bash
set -euo pipefail

DEST_DIR="${DEST_DIR:-src-tauri/assets/fonts}"
mkdir -p "$DEST_DIR"

fetch_one() {
    local name="$1" url="$2" expected_sha="$3"
    local dest="$DEST_DIR/$name"

    if [ -f "$dest" ]; then
        local actual; actual=$(sha256sum "$dest" | awk '{print $1}')
        if [ "$actual" = "$expected_sha" ]; then
            printf 'fetch-fonts: %s up-to-date\n' "$name"
            return 0
        fi
    fi

    printf 'fetch-fonts: downloading %s\n' "$name"
    local tmp; tmp=$(mktemp)
    curl --fail --silent --show-error --location -o "$tmp" "$url"
    local actual; actual=$(sha256sum "$tmp" | awk '{print $1}')
    if [ "$actual" != "$expected_sha" ]; then
        printf 'error: SHA256 mismatch for %s (expected %s, got %s)\n' \
            "$name" "$expected_sha" "$actual" >&2
        rm -f "$tmp"
        exit 1
    fi
    mv "$tmp" "$dest"
}

# Pinned font sources (tag + SHA256 placeholders filled in IMPLEMENTATION.md)
fetch_one "NotoColorEmoji.ttf"      "<URL>" "<SHA256>"
fetch_one "NotoSansCJKjp-Regular.otf" "<URL>" "<SHA256>"
fetch_one "NotoEmoji-Regular.ttf"   "<URL>" "<SHA256>"
fetch_one "Inconsolata-Regular.ttf" "<URL>" "<SHA256>"
```

### `build.rs` Update

```rust
// src-tauri/build.rs (excerpt, illustrative)
#[cfg(feature = "gui")]
fn check_bundled_fonts() {
    let required = [
        "assets/fonts/NotoColorEmoji.ttf",
        "assets/fonts/NotoSansCJKjp-Regular.otf",
        "assets/fonts/NotoEmoji-Regular.ttf",
        "assets/fonts/Inconsolata-Regular.ttf",
    ];
    for path in required {
        if !std::path::Path::new(path).exists() {
            panic!(
                "bundled font missing at {path}\n  \
                 Run `make fetch-fonts` or `bash scripts/fetch-fonts.sh` to download bundled fonts."
            );
        }
        println!("cargo:rerun-if-changed={path}");
    }
}
```

### Makefile Integration

```makefile
FONT_FILES = \
    src-tauri/assets/fonts/NotoColorEmoji.ttf \
    src-tauri/assets/fonts/NotoSansCJKjp-Regular.otf \
    src-tauri/assets/fonts/NotoEmoji-Regular.ttf \
    src-tauri/assets/fonts/Inconsolata-Regular.ttf

.PHONY: fetch-fonts
fetch-fonts:
	bash scripts/fetch-fonts.sh

setup: fetch-fonts
	rustup target add x86_64-pc-windows-msvc
	cargo install cargo-xwin

dev: fetch-fonts
	bun run build:viewer
	bun run build:settings
	cargo run --manifest-path src-tauri/Cargo.toml

build: fetch-fonts
	CARGO_TARGET_DIR=src-tauri/target-host \
	    cargo build --release --manifest-path src-tauri/Cargo.toml
```

`cli-build` and `cli-dpkg` deliberately do not depend on `fetch-fonts`.

### GitHub Actions Integration

```yaml
- name: Fetch bundled fonts
  run: bash scripts/fetch-fonts.sh

- name: Cache bundled fonts
  uses: actions/cache@v4
  with:
    path: src-tauri/assets/fonts/*.ttf src-tauri/assets/fonts/*.otf
    key: fonts-${{ hashFiles('scripts/fetch-fonts.sh') }}

- name: Build
  run: make build
```

### Dependencies

**Internal:**
- `crates/app_settings` — settings schema + migration entry point.
- `src-tauri/src/render/font/*` — bundle constants, role enum, resolver, presentation.

**External:**
- `curl` (system, used by fetch-fonts.sh; `wget` accepted as alternative if implementation chooses).
- `sha256sum` (POSIX-installable; ships in coreutils on Linux; available via Git for Windows).
- Existing `fontdb`, `swash`, `zeno` crates (no version bump required by this feature).

## Test Scenarios

### Unit Tests
- [ ] `presentation_for('a', None)` → `NotEmoji`
- [ ] `presentation_for('\u{23F5}', None)` → `Monochrome` (text-presentation default)
- [ ] `presentation_for('\u{23F5}', Some('\u{FE0F}'))` → `Color` (VS16 overrides)
- [ ] `presentation_for('\u{1F600}', None)` → `Color` (emoji-presentation default)
- [ ] `presentation_for('\u{1F600}', Some('\u{FE0E}'))` → `Monochrome` (VS15 forces text)
- [ ] `AppSettings::migrate_legacy()` moves `font_family_emoji` → `font_family_emoji_color` and returns `true`
- [ ] `AppSettings::migrate_legacy()` initializes `font_family_emoji_monochrome` to `Noto Emoji` when absent
- [ ] `AppSettings::migrate_legacy()` returns `false` for a settings file already on the new schema (idempotent)
- [ ] `Resolver::register_bundled()` registers four distinct ids covering CJK, ColorEmoji, MonochromeEmoji, Base
- [ ] `Resolver::scan_user_dir()` registers `*.ttf` / `*.otf` from the path and skips other extensions

### Integration Tests
- [ ] User-dir font override: place a known font in the user dir → resolver returns it before bundle for the same family name
- [ ] Build with one font deleted: cargo build fails with the expected panic message naming the missing file
- [ ] `fetch-fonts.sh` is idempotent: second run reports "up-to-date" for every font
- [ ] `fetch-fonts.sh` detects SHA256 mismatch: corrupt a file, re-run → script reports error and replaces the file

### E2E Tests
**Existing E2E tests**: tauri-driver based; see `test/README.md`
**Run command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "..."`
- [ ] Existing E2E tests pass without regression
- [ ] Manual: launch Windows release build, paste `⏵` → rendered using bundled Noto Emoji (monochrome)

### Edge Cases
- [ ] User dir does not exist: resolver skips silently (no warning spam)
- [ ] User dir exists but is empty: resolver skips silently
- [ ] User dir contains a non-font file (e.g. README.txt): resolver ignores it
- [ ] User dir contains a corrupted TTF: resolver logs warn and continues
- [ ] settings.json has only legacy keys: migration runs and rewrites file
- [ ] settings.json has both legacy and new keys (mixed): new keys win, legacy keys are dropped
- [ ] fetch-fonts.sh runs without `curl` and `wget`: clear error
- [ ] fetch-fonts.sh runs without `sha256sum`: clear error
- [ ] `make cli-build` works without fetch-fonts (no font files needed)

### Performance Tests
- [ ] Resolver startup with empty user dir: total scan < 500 ms
- [ ] Resolver startup with 4 user-dir fonts: < 600 ms

## Security Considerations

- **HTTPS only:** fetch-fonts.sh uses curl with `--location` over HTTPS; no `--insecure` flag.
- **SHA256 integrity:** every fetched file is checked against a hash committed in the repository. Mismatch aborts the build.
- **No arbitrary execution:** the script does not execute downloaded content; files are only placed under `src-tauri/assets/fonts/`.
- **Input validation:** Settings migration treats legacy values as opaque strings (font family names already pass through serde without execution).

## Error Handling

### Error Codes

| Code (logged tag) | Description | Surface |
|------|-------------|--------|
| `fetch_fonts.download_failed` | curl/wget non-zero exit | stderr from `fetch-fonts.sh` |
| `fetch_fonts.sha_mismatch` | downloaded SHA256 differs from pinned | stderr |
| `fetch_fonts.tool_missing` | neither curl nor wget present | stderr |
| `build_rs.font_missing` | bundled file absent at compile time | cargo build error |
| `font.user_dir.read_failed` | I/O error reading user font dir | `emterm.log` warn |
| `font.user_dir.invalid_file` | font parsing failed | `emterm.log` warn |
| `settings.migrate.failed_persist` | could not rewrite settings.json after migration | `emterm.log` warn |

### Error Flow
```
Build-time error
  → fetch-fonts.sh exits non-zero
  → make / GitHub Actions fails
  → developer / CI reads stderr instructions

Runtime error (user dir)
  → resolver logs warn (with emterm.log path noted)
  → resolver continues with remaining chain entries
```

## Performance Optimization

### Performance Goals
- Resolver scan: < 500 ms (existing NFR1)
- User-dir scan overhead: < 50 ms typical
- fetch-fonts.sh idempotent path: < 1 s for all four fonts

### Optimization Strategies
- SHA256 short-circuit: avoid re-downloading already-present, hash-matched files.
- GitHub Actions cache: SHA256-of-script keyed cache skips network entirely on cache hit.
- User-dir scan caps at one level deep (no recursive descent).

### Caching Strategy
- CI: `actions/cache` keyed on `hashFiles('scripts/fetch-fonts.sh')`
- Local dev: implicit via filesystem (idempotency check)

## Success Criteria

- [ ] `git ls-files src-tauri/assets/fonts/` returns only `LICENSE`, `README.md`, and `.gitignore`
- [ ] `git count-objects -vH` shows a smaller repo than baseline by at least 25 MB
- [ ] `make build` after a fresh clone succeeds (network required for first build)
- [ ] `cargo build` without prior `fetch-fonts` panics with the documented error message
- [ ] Windows release build renders `⏵` correctly (manual smoke test)
- [ ] Legacy `settings.json` migrates without user intervention; new keys persisted
- [ ] User-dir override works for at least one font on Linux and Windows
- [ ] CI builds run within prior wall-time tolerance (fetch + cache amortized)
- [ ] All new unit/integration tests pass

## Open Questions

> 未解決の要件は sdd.yaml で `status: tbd` として管理する。`/em-sdd:sdd.2-create-plan` の実行前に解決すること。

- [x] **OQ1 (FR1):** Resolved. Each font is pinned to an upstream Git tag:
    - `NotoColorEmoji.ttf` — `googlefonts/noto-emoji` @ `v2.051`
    - `NotoSansCJKjp-Regular.otf` — `googlefonts/noto-cjk` @ `Sans2.004`
    - `NotoEmoji-Regular.ttf` — `googlefonts/noto-emoji` @ `v2.034`
    - `Inconsolata-Regular.ttf` — `googlefonts/Inconsolata` @ `v3.000`
    SHA256 values live in `scripts/fetch-fonts.sh`.
- [ ] **OQ2 (FR5):** Source of truth for `Emoji` / `Emoji_Presentation` Unicode property tables (inline static table vs `unicode-emoji` crate vs custom build script). Will be resolved during the implementation plan based on dependency footprint.
- [ ] **OQ3 (FR7):** Whether the migrated settings file should keep a one-time backup (`settings.json.bak`) for easy rollback. Will be resolved during the implementation plan.

## Implementation Phases

### Phase 1: Font fetch infrastructure
**Goals:** Remove fonts from git, introduce `fetch-fonts.sh` and `build.rs` failsafe, wire Makefile and CI.
**Deliverables:**
- `scripts/fetch-fonts.sh` with hash-pinned downloads for two existing fonts
- `src-tauri/assets/fonts/.gitignore`
- `git rm --cached` for existing font files (history rewrite NOT required; new commit removes them)
- `build.rs` existence check
- Makefile `fetch-fonts` target + dependencies
- GitHub Actions workflow updates
- README updates for the new setup step

### Phase 2: Bundle expansion + color/monochrome split
**Goals:** Add Noto Emoji (monochrome) and Inconsolata, split emoji configuration, implement presentation-aware dispatch.
**Deliverables:**
- New font entries in `fetch-fonts.sh`
- New `include_bytes!` constants in `resolver.rs`
- `FontRole::ColorEmoji` / `MonochromeEmoji` enum split (rename of existing `Emoji`)
- `presentation.rs` with VS15/VS16 and Unicode property dispatch
- Settings schema additions + auto-migration
- Settings UI updates (color / monochrome rows in the font section)

### Phase 3: User directory override
**Goals:** Honor a user-supplied font directory ahead of system + bundle.
**Deliverables:**
- `user_font_dir()` helper covering Linux and Windows app-data paths
- `Resolver::scan_user_dir()` integration
- Tests covering empty / populated / invalid user dirs
- README note pointing users at the override path

## References

- 要件定義書: `doc/tasks/font-bundle-redesign/要件定義書.md`
- 設計レポート: `tmp/font-bundle-redesign-2026-06-24.md`
- 既存 font 関連 SDD: `doc/tasks/font-swash-migration/SPEC.md`
- 既存 default font SDD: `doc/tasks/default-font-adjustment/SPEC.md`
- OFL: https://scripts.sil.org/OFL
- Unicode Emoji Data: https://unicode.org/Public/emoji/
- googlefonts/noto-emoji: https://github.com/googlefonts/noto-emoji
- notofonts/noto-cjk: https://github.com/notofonts/noto-cjk
- googlefonts/Inconsolata: https://github.com/googlefonts/Inconsolata
