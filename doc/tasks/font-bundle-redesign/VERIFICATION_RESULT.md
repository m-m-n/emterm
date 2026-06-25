# 検証結果: font-bundle-redesign

- 検証日時: 2026-06-25
- 対象機能: フォントバンドル再設計 (バイナリ脱git化 + Noto Emoji/Inconsolata 追加 + プレゼンテーション分割 + ユーザーディレクトリ override)
- 関連: `SPEC.md` / `IMPLEMENTATION.md` / `VERIFICATION.md` / `tasks.yaml`
- ビルド/テスト/フォーマット/静的解析は sdd.5-check で確認済み (全PASS)。本ドキュメントでは再実行しない。

## 概要

sdd.yaml に基づき phase-1 〜 phase-3 の全タスクが `status: completed` (1件 `deferred`) と記録されている。実装ファイルは要件どおり配置済み、SPEC.md の FR1〜FR10 と NFR1〜NFR8 はおおむねカバーされているが、以下の3点は未確定:

1. `scripts/fetch-fonts.sh` の URL/SHA256 placeholder (`TODO(URL)` × 4, `TODO(SHA256)` × 2)
2. `migrate_legacy()` の loader 側からの呼び出し (caller wiring) と `.bak` 書き出し
3. `git rm --cached` による既存バイナリの untrack

これらは tasks.yaml/IMPLEMENTATION.md でも「user/後続作業」として明示的に保留されている既知項目。

## ファイル構造検証

### Files to Create (要件 vs 実体)

| パス | 要件 | 実体 | 結果 |
|------|------|------|------|
| `scripts/fetch-fonts.sh` | 作成 | 存在 (6193B, exec 可) | ✅ |
| `src-tauri/assets/fonts/.gitignore` | 作成 | 存在 (`*.ttf`/`*.otf` exclude, `LICENSE`/`README.md` 維持) | ✅ |
| `src-tauri/src/render/font/presentation.rs` | 作成 | 存在 (26.6 KiB, `presentation_for` + テスト10件) | ✅ |
| `src-tauri/src/render/font/user_dir.rs` | 作成 | 存在 (11.0 KiB, `user_font_dir`+`scan_user_dir`+テスト) | ✅ |
| `src-tauri/assets/fonts/NotoEmoji-Regular.ttf` | fetch (untracked) | 存在 (10.7 MiB, placeholder=NotoColorEmoji コピー) | ⚠️ プレースホルダ |
| `src-tauri/assets/fonts/Inconsolata-Regular.ttf` | fetch (untracked) | 存在 (15.7 MiB, placeholder=CJK コピー) | ⚠️ プレースホルダ |

`NotoEmoji-Regular.ttf` と `Inconsolata-Regular.ttf` は SHA256 placeholder 状態。tasks.yaml の `phase-2-fetch-and-buildrs` notes に「本物のフォントが届くまで既存バンドルのコピーを `include_bytes!` 解決のために配置」と明記。

### Files to Modify (要件 vs git status)

| パス | 要件 | 変更 | 結果 |
|------|------|------|------|
| `src-tauri/build.rs` | bundled-font 存在チェック | `check_bundled_fonts()` が4件全部を列挙 + 期待されるエラーメッセージ | ✅ |
| `Makefile` | `fetch-fonts` target + 依存 | `setup`/`dev`/`build`/`win-build`/`dpkg` が `fetch-fonts` 依存、`cli-build`/`cli-dpkg` は非依存 | ✅ |
| `.github/workflows/release.yml` | fetch + cache step | `build-linux` (L100-111) と `build-windows` (L200-209) 両ジョブに挿入済み、キーは `hashFiles('scripts/fetch-fonts.sh')` | ✅ |
| `README.md` | setup + override 説明 | L135 から fetch-fonts 説明、L141-148 で user override path 記載 | ✅ |
| `src-tauri/assets/fonts/README.md` | fetch + override 説明 | 全面書き換え (fetch / offline / override / placeholder 注記) | ✅ |
| `crates/app_settings/src/settings.rs` | 新 key + migration | 4 新 key + `apply_migrations()` + `migrate_legacy()` alias + tests 5 件 | ✅ |
| `src-tauri/src/render/font/resolver.rs` | role 分割 + 定数改名 | `BUNDLED_EMOJI_COLOR_FONT`/`BUNDLED_EMOJI_MONO_FONT`/`BUNDLED_BASE_FONT`/`BUNDLED_CJK_FONT`、`FontRole::{ColorEmoji,MonochromeEmoji}` | ✅ |
| `src-tauri/src/render/font/mod.rs` | `presentation`/`user_dir` 露出 | wiring 実施 | ✅ |
| `src-tauri/src/render/font/swash_adapter.rs` | 定数改名 + per-codepoint 分岐 | 5 参照更新 + L191-192 で ColorEmoji + MonochromeEmoji を chain に挿入 | ✅ (注: per-codepoint `presentation_for` 直接呼出しは `FallbackChain::from_resolver` 経由で間接実現、tasks.yaml で deferred) |
| `src-tauri/src/render/font/ab_glyph_adapter.rs` | `PROBE_FONT_BYTES` 改名 | L159 で `BUNDLED_EMOJI_COLOR_FONT` 参照 | ✅ |
| `src-tauri/src/render/font/fallback.rs` | chain に MonochromeEmoji 挿入 | L85 で ColorEmoji と Secondary の間に位置 | ✅ |
| `src-tauri/src/render/terminal_grid_pass.rs` | `register_bundled` caller 更新 | 4-id destructure に変更済み | ✅ |
| `src-tauri/src/app.rs` | resolver build 順 | L679 `scan_user_dir()` を `register_bundled()` の前に呼び first-wins で user 優先 | ✅ |
| `src-tauri/src/ui/chrome.rs` | `BUNDLED_EMOJI_FONT` → COLOR | L95, L108 で改名済み | ✅ |
| `src-tauri/src/settings.rs` | 新 key + loader | `emoji_font` + `emoji_font_monochrome` + loader tests (`loader_font_family_emoji_color_sets_emoji_font` 他2件) | ✅ |
| `src-tauri/src/settings_store.rs` | round-trip test 更新 | new color key 経由 | ✅ |
| `src-tauri/web-shared/settings/types.ts` | 新 key + FontCategory | 反映 | ✅ |
| `src-tauri/web-shared/settings/settings-applier.ts` | color 新 key 優先 + legacy fallback | 反映 | ✅ |
| `src-tauri/web-shared/settings/font-picker.ts` | カテゴリマップ追加 | 反映 | ✅ |
| `src-tauri/web-shared/settings/sections/terminal-appearance-section.ts` | emoji 行を 2 行に分割 | 反映 | ✅ |
| `src-tauri/web-shared/settings/sections/markdown-viewer-section.ts` | 同上 | 反映 | ✅ |
| `src-tauri/web-shared/i18n/locales/{en,ja}.json` | 新ラベル | `emojiFontFamilyColor`/`emojiFontFamilyMonochrome` 等を追加 | ✅ |
| `src-tauri/src/i18n.rs` | native UI ラベル | **未変更** (native UI は font picker ラベルを露出しない設計のため必要なし、tasks.yaml で明記) | ⚠️ 不要 |

### Files to Untrack (要件 vs git ls-files)

| パス | 要件 | 現状 | 結果 |
|------|------|------|------|
| `src-tauri/assets/fonts/NotoColorEmoji.ttf` | `git rm --cached` | **まだ tracked** (`git ls-files` で出る) | ⏳ ユーザー操作待ち |
| `src-tauri/assets/fonts/NotoSansCJKjp-Regular.otf` | `git rm --cached` | **まだ tracked** | ⏳ ユーザー操作待ち |

IMPLEMENTATION.md と tasks.yaml で「実行者は user」と明記済み。実装者側で対応する手順は完了。

## FR / NFR カバレッジ

### Functional Requirements

| ID | 要約 | 実装場所 | Status |
|----|------|----------|--------|
| FR1 | SHA256 pin + idempotent fetch script | `scripts/fetch-fonts.sh` (本検証で実行確認: 既存4ファイルが全て "up-to-date"/"present" 報告で network 不使用) | ⚠️ 2件 placeholder |
| FR2 | gitignore + バイナリを git 外す | `.gitignore` 配置済み / 既存2バイナリは未 untrack | ⚠️ untrack 未実施 |
| FR3 | Noto Emoji + Inconsolata をバンドル追加 | `resolver.rs` の `BUNDLED_EMOJI_MONO_FONT`/`BUNDLED_BASE_FONT`、`include_bytes!` 4本 | ✅ |
| FR4 | color/monochrome 設定 key 分割 | `app_settings::AppSettings` に4新 key + Settings 反映 + WebView i18n | ✅ |
| FR5 | VS15/VS16 + Unicode property dispatch | `presentation.rs::presentation_for` (FE0F/FE0E + `Emoji_Presentation` テーブル) + `FallbackChain` に MonochromeEmoji 挿入 | ✅ (per-codepoint 呼び出しは fallback chain 経由) |
| FR6 | 解決チェーン settings > user dir > system > bundle | `app.rs` で `scan_user_dir()` → `register_bundled()` の順、`Resolver::by_family` first-wins | ✅ |
| FR7 | legacy key auto-migration + persist | `AppSettings::migrate_legacy()` 実装、tests 5 件 / **loader 側の呼び出しは未配線**、`.bak` は corrupt 用のみ | ⚠️ TBD (caller wiring) |
| FR8 | build.rs 存在チェック + actionable panic | `check_bundled_fonts()` で4本検証 + `make fetch-fonts` 案内 | ✅ |
| FR9 | Makefile target 配線 | `setup`/`dev`/`build`/`win-build`/`dpkg` 依存 / `cli-build`/`cli-dpkg` 非依存 | ✅ |
| FR10 | CI workflow fetch + cache | `release.yml` 2 ジョブで fetch + `actions/cache@v4` keyed by `hashFiles('scripts/fetch-fonts.sh')` | ✅ |

### Non-Functional Requirements

| ID | 要約 | 実装/評価 | Status |
|----|------|------|--------|
| NFR1 | SHA256 で byte-identical 保証 | script は mismatch 時 abort、4 entry いずれも `sha256sum` 検証パス | ✅ (placeholder 2件は pin 待ち) |
| NFR2 | repo size −30MB | `.gitignore` 完備だが `git rm --cached` 未実行のためまだ未達成 | ⏳ user 操作待ち |
| NFR3 | missing-font エラーの実行可能性 | `build_rs.font_missing` メッセージに recovery command 含む | ✅ |
| NFR4 | 既存 settings 互換 | `migrate_legacy()` で legacy key を color 側へ移送 (in-memory) | ⚠️ persist 未配線 |
| NFR5 | offline でも builds 成功 | 4 ファイル on-disk + SHA256 match で network 不使用 (本検証で確認) | ✅ |
| NFR6 | startup < 500ms | release 実バイナリ計測のみ可能 | 🛑 計測保留 |
| NFR7 | binary size +2MB 以内 | release ビルド要 | 🛑 計測保留 |
| NFR8 | HTTPS only / no `--insecure` | `download()` 関数で URL を `https://*` パターンで強制、curl/wget 共に `-k` 不使用 | ✅ |

## テストシナリオ結果 (TS-1..TS-18)

sdd.5-check により全 Rust テスト (1943 件) と TypeScript テスト (10 件) は PASS 済み。以下は VERIFICATION.md 上の TS が実際にテストとしてコード化されているかの確認。

| ID | シナリオ | テスト関数 | 結果 |
|----|----------|-----------|------|
| TS-1 | `presentation_for('a', None)` → NotEmoji | `presentation.rs::presentation_for_ascii_letter_is_not_emoji` | ✅ コード化 + PASS |
| TS-2 | `presentation_for('\u{23F5}', None)` → Monochrome | `presentation_for_text_default_emoji_is_monochrome` | ✅ コード化 + PASS |
| TS-3 | `presentation_for('\u{23F5}', VS16)` → Color | `presentation_for_vs16_forces_color` | ✅ コード化 + PASS |
| TS-4 | `presentation_for('\u{1F600}', None)` → Color | `presentation_for_emoji_default_is_color` | ✅ コード化 + PASS |
| TS-5 | `presentation_for('\u{1F600}', VS15)` → Monochrome | `presentation_for_vs15_forces_monochrome` | ✅ コード化 + PASS |
| TS-6 | `migrate_legacy` で emoji → color へ移動 | `app_settings::migrate_legacy_moves_emoji_key_to_color` | ✅ コード化 + PASS |
| TS-7 | `migrate_legacy` で monochrome default 初期化 | `migrate_legacy_initializes_monochrome_default` | ✅ コード化 + PASS |
| TS-8 | new-schema file で idempotent (false 返却) | `migrate_legacy_idempotent_on_new_schema` | ✅ コード化 + PASS |
| TS-9 | `register_bundled()` が 4 distinct id を返す | `resolver::tests::register_bundled_returns_distinct_ids` | ✅ コード化 + PASS |
| TS-10 | `scan_user_dir` が `.ttf`/`.otf` のみ登録、corrupt は warn 1 件 | `user_dir::tests::scan_dir_into_filters_by_extension_and_skips_corrupt` | ✅ コード化 + PASS |
| TS-11 | user-dir font が bundle に勝つ | `user_dir::tests::user_dir_entry_wins_family_lookup_over_bundle` | ✅ コード化 + PASS |
| TS-12 | bundled font 1 個欠損で `cargo build` panic | コード化なし。`build.rs::check_bundled_fonts` のメッセージは確認済み (要件「`build_rs.font_missing` で recovery command 案内」を満たすことを目視) | ⏳ 手動 (本検証では削除しない) |
| TS-13 | `fetch-fonts.sh` idempotent | 本検証で 1 回実行 → 4 件すべて "up-to-date"/"present" を報告、network access なし | ✅ 実行確認 |
| TS-14 | SHA256 mismatch 検出 + 置換 | スクリプト L122-128 で `fetch_fonts.sha_mismatch` 出力 + tmp 削除のロジック確認 (手動再現は本検証では未実施) | ⏳ 手動 |
| TS-15 | 到達不能 URL でエラー | スクリプト L113-118 で `fetch_fonts.download_failed`、tmp 削除を確認 | ⏳ 手動 |
| TS-16 | legacy settings load → `.bak` 書き出し + 新 schema rewrite | **未配線**。`migrate_legacy` は呼び出されておらず、`settings.json.bak` は corrupt-file 経路でしか生まれない (`settings_store.rs` L44-60) | ❌ TBD (caller wiring) |
| TS-17 | settings panel に emoji 2 行表示 | E2E (tauri-driver) — 本セッションでは実行しない | ⏳ 手動 / E2E |
| TS-18 | `cli-build` (fonts 不在で OK) | sdd.5-check で `cargo check --no-default-features` PASS、`Makefile` の `cli-build`/`cli-dpkg` は `fetch-fonts` 非依存 | ✅ |

その他コード化されている追加テスト:
- `migrate_legacy_keeps_new_key_when_both_present`, `migrate_legacy_markdown_emoji_key_to_color` (mixed-key と markdown 側分岐)
- `presentation_for_digit_is_monochrome`, `presentation_for_japanese_letter_is_not_emoji`, `presentation_for_other_vs_falls_through`, テーブル形状 invariants
- `user_dir::tests::user_font_dir_prefers_xdg_data_home`, `user_font_dir_falls_back_to_home_local_share`, `scan_dir_into_missing_dir_is_silent_noop`, `scan_dir_into_empty_dir_is_zero_registrations`
- `resolver::tests::by_role_lists_each_registered_font`, `by_family_resolves_registered_name`
- `settings::tests::loader_font_family_emoji_sets_emoji_font` + `_color` + `_monochrome` + `loader_new_color_key_overrides_legacy`

## セキュリティ確認

| 項目 | 検証コマンド | 結果 |
|------|-------------|------|
| HTTPS only | `grep -nE 'https?://' scripts/fetch-fonts.sh` | `http://` は 0 件、`https://` のみ (実 URL 2 件 + L56 のパターンマッチ)。`download()` で `https://*` 以外は `fetch_fonts.insecure_url` で reject | ✅ |
| `--insecure` / `-k` フラグ不使用 | `grep -nE -- '--insecure\|-k' scripts/fetch-fonts.sh` | curl 行は `--fail --silent --show-error --location` のみ、`-k` 無し | ✅ |
| SHA256 強制 | スクリプト L121-128 で `actual != expected_sha` 時 abort、placeholder 時も L91 で no-local-copy なら exit 1 | ✅ |
| `.bak` atomic write | corrupt-file 経路のみ (`settings_store.rs` L47-60)、migration 起因の `.bak` は未配線 | ⚠️ migration 経路は未実装 |
| user-dir スキャン: executable/symlink 除外 | `user_dir.rs` の `scan_dir_into` で拡張子 `.ttf`/`.otf` のみ通過、それ以外無視 | ✅ |

## 性能確認

| 項目 | 計測方法 | 状態 |
|------|----------|------|
| NFR6 (startup font scan < 500ms, user-dir < 50ms) | `EMTERM_FONT_PERF=1` 付き GUI release 起動で計測 | 🛑 release build 未実行のため保留 |
| NFR7 (binary size +2MB 以内) | `ls -la src-tauri/target-host/release/emterm` 前後比較 | 🛑 release build 未実行のため保留 |
| fetch-fonts idempotent path < 1s | 本検証で実測: 4 ファイル即時 "up-to-date"/"present" (体感 < 1s) | ✅ |

## 残 TODO / placeholder 一覧

`scripts/fetch-fonts.sh` 内:

| 行 | プレースホルダ | 必要なアクション |
|----|---------------|----------------|
| L144 | `TODO(URL)` NotoColorEmoji.ttf | 既存バイナリの SHA256 (L145 `ede3ac…306c`) と一致する GitHub Releases asset の URL を pin |
| L148 | `TODO(URL)` NotoSansCJKjp-Regular.otf | 既存バイナリの SHA256 (L149 `68a3fc…75b5`) と一致する GitHub Releases asset の URL を pin |
| L156 | `TODO(URL)` NotoEmoji-Regular.ttf | `googlefonts/noto-emoji` の release tag を選定し URL を入れる |
| L157 | `TODO(SHA256)` NotoEmoji-Regular.ttf | 上記 tag の asset SHA256 を pin (現在は placeholder 受け入れモードで動作) |
| L160 | `TODO(URL)` Inconsolata-Regular.ttf | `googlefonts/Inconsolata` の release tag を選定し URL を入れる |
| L161 | `TODO(SHA256)` Inconsolata-Regular.ttf | 上記 tag の asset SHA256 を pin |

その他:

| 場所 | TODO | アクション |
|------|------|-----------|
| `crates/app_settings/src/settings.rs::migrate_legacy` 呼び出し | 未配線 | loader 側で `migrate_legacy()` を呼び、true なら `settings.json.bak` を atomic 書き出し → 新 schema 書き戻し (FR7 / TS-16) |
| `src-tauri/assets/fonts/NotoColorEmoji.ttf` `NotoSansCJKjp-Regular.otf` | 未 untrack | `git rm --cached <path>` を user が実行 (`.gitignore` は既に有効) |
| `src-tauri/assets/fonts/NotoEmoji-Regular.ttf` (10.7 MiB) | placeholder (NotoColorEmoji コピー) | 上記 fetch script の URL/SHA256 pin 後に再 fetch すると本物に差し替わる |
| `src-tauri/assets/fonts/Inconsolata-Regular.ttf` (15.7 MiB) | placeholder (CJK コピー) | 同上 |

## ユーザーが手動で行うべき項目

### 即時必要 (FR/SC 達成に必須)

1. **`git rm --cached src-tauri/assets/fonts/NotoColorEmoji.ttf`** および **`NotoSansCJKjp-Regular.otf`** を実行し、本 feature ブランチでコミット → SC-1 / SC-2 / NFR2 を満たす
2. **upstream tag + SHA256 を pin** して `scripts/fetch-fonts.sh` の 6 件の `TODO(URL)` / 2 件の `TODO(SHA256)` を埋める
3. **`migrate_legacy()` を loader 側から呼び出す配線** を追加し、true なら `settings.json` を `.bak` にバックアップしてから新 schema で書き戻す (FR7 / SC-6 / TS-16)

### スモークテスト (E2E 不可)

4. **Windows release build スモーク**: `⏵` (U+23F5) を貼り付け、Noto Emoji (monochrome) で描画されることを確認 (SC-5 / US2)
5. **Linux user-dir override スモーク**: `~/.local/share/net.laser5.app.emterm/fonts/` に新しい `NotoColorEmoji.ttf` を置いて起動、override が効くことを目視 (SC-7)
6. **Windows user-dir override スモーク**: `%APPDATA%\net.laser5.app.emterm\fonts\` で同上
7. **legacy `settings.json` migration**: 旧 schema の settings.json を置いて起動 → `settings.json.bak` が1度だけ生成され、再起動で2度目が出ないこと (TS-16 / US4) *(配線実装後)*
8. **`cargo build` panic 確認** (TS-12): `src-tauri/assets/fonts/*.ttf` を1個退避して `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` 実行 → `build_rs.font_missing: bundled font missing at assets/fonts/...` panic + `make fetch-fonts` 案内が出ることを確認
9. **TS-17 E2E**: tauri-driver で settings panel を起動し、color/monochrome の2行が両方表示・編集可能であることを確認
10. **TS-14 / TS-15**: 既存 font を bit 反転で破壊して fetch-fonts 再実行 → `sha_mismatch` で abort、tmp 残骸なし。URL を `https://invalid.example.invalid/x` に変えて `download_failed` で abort

### 性能計測

11. **NFR6**: release build を作成し `EMTERM_FONT_PERF=1` で起動、scan 時間 < 500ms / user-dir < 50ms を確認
12. **NFR7**: release binary サイズを baseline と比較し +2MB 以内を確認

## 総合判定

⚠️ **Conditional PASS** — 実装本体 (Rust/TS コード、テスト、ビルド配線、CI、ドキュメント) は要件どおり完了し sdd.5-check で全テスト PASS 済み。ただし以下3点が「ユーザー側の確定作業待ち」のため、SPEC.md の Success Criteria (SC-1, SC-2, SC-5, SC-6, SC-7) を満たすには手動アクションが必要:

- `git rm --cached` 未実施 → SC-1 / SC-2 未達
- `scripts/fetch-fonts.sh` の URL/SHA256 placeholder → 本物の Noto Emoji / Inconsolata が手元に来ていない (placeholder copy が代入)
- `migrate_legacy()` の loader caller wiring + `.bak` 書き出し → SC-6 / TS-16 未達

これらはいずれも IMPLEMENTATION.md / tasks.yaml で「保留」として明示記録されており、実装側のスコープは全うされている。残作業をユーザーが完了させれば、Phase-1〜3 全体の Acceptance Criteria を満たす。
