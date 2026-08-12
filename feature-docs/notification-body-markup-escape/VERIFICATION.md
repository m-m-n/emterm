# Verification Document: notification-body-markup-escape

## Overview

**Feature**: notification-body-markup-escape / **SPEC.md**: `feature-docs/notification-body-markup-escape/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/notification-body-markup-escape/IMPLEMENTATION.md`

## Build Verification

- Command (main / GUI デフォルトフィーチャー): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli / `--no-default-features`): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: いずれも exit code 0、エラーなし

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: カバレッジ測定ツールは未導入のため数値目標は設定しない。新規の純関数ヘルパー 2 つ（エスケープ変換 / ケイパビリティ結果解釈）は全分岐をユニットテストで覆う
- 注意: テストは `--lib` にある（`--bin emterm` は 0 件）。`tabs.rs` の replay テストに既知の非決定性があり、無関係な失敗が疑われる場合は同じ承認済みコマンドを再実行して判断する

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | タグインジェクションのタイトル `<a href="https://attacker.invalid">Sign in</a>`（body-markup 確認済み） | 解釈可能なマークアップを含まないエスケープ済みリテラルテキストのみの本文 | Unit |
| TS2 | 既に `&amp;` を含む入力 | `&amp;amp;` になる（`&` 先行順序の固定） | Unit |
| TS3 | 100 文字超・境界付近にメタ文字を持つタイトル | 先にトランケート、後にエスケープ。実体参照が分断されない（結果が 100 文字超は正当） | Unit（境界値） |
| TS4 | `"body-markup"` 不在、または取得失敗 | 本文は未エスケープのまま（`&amp;` が可視化されない） | Unit |
| TS5 | タブアクティビティ経路とエージェント経路の本文がシンク側エスケープ判定を通る | 両経路とも確認済み時にエスケープされる（単一チョークポイント） | Integration（合成ユニット） |
| TS6 | 既存の `sanitize_title` / `notification_body` テスト | 期待値変更なしで通る（`src-tauri/src/notifications.rs` は無変更） | Regression |
| TS7 | `--lib` スイート全体 | 全テスト通過 | Regression |
| TS8 | CLI のみビルドの検証: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0（フィーチャーゲートが壊れていない） | Build check |
| TS9 | Windows 通知経路の無変更確認 | ケイパビリティ判定・エスケープのコードが Unix ゲート配下にのみ存在し、Windows 送出フローが字面上も無変更 | Manual（コード検査） |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: プロジェクトに承認済みの lint コマンドは無い（check コマンドで代替）

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | マークアップメタ文字（`&` → `&amp;`、`<` → `&lt;`、`>` → `&gt;`）をエスケープするコード経路が存在する | TS1 / TS2 |
| SC2 | エスケープは 100 文字トランケートの後に実行され、実体参照が分断されない | TS3 |
| SC3 | エスケープは `get_capabilities()` が `"body-markup"` を確認した場合のみ適用され、非対応サーバーで `&amp;` が生表示されない | TS4 |
| SC4 | タグ・実体参照を含むタイトルが本文でリテラルテキストのみになることを固定するユニットテストが存在する | TS1 |
| SC5 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る | TS7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2（ユニットテスト） |
| FR2 | task0001 | TS3（境界値ユニットテスト） |
| FR3 | task0001 | TS4（ユニットテスト） |
| FR4 | task0001 | TS5（両経路の合成テスト） |
| FR5 | task0001 | TS9（コード検査: Unix ゲート配下のみに変更が存在） |
| NFR1 | task0001 | TS6, TS7（既存テストの無変更通過） |
| NFR2 | task0001 | TS1, TS5（チョークポイントの実証。実装位置は IMPLEMENTATION.md D1 参照） |
| NFR3 | task0001 | TS8（`--no-default-features` check）+ TS9 |

## E2E Testing

既存の E2E 基盤・E2E 実行コマンドは検出されていない（SPEC のとおり該当なし）。

## Manual Testing (E2E Not Possible)

- [ ] TS9: Windows 通知経路の無変更確認 — `src-tauri/src/callbacks.rs` の差分を検査し、ケイパビリティ判定・エスケープのコードが `#[cfg(unix)]` 相当のゲート配下にのみ存在し、Windows 送出フローが無変更であること（Windows クロスビルドは承認済みコマンドに含まれないため、検査で代替する）
- [ ] （任意）body-markup 対応の通知サーバー（GNOME Shell / Plasma / dunst の markup=full）上で、タグ入りタイトルの通知本文がリテラルテキストとして表示されることの実機確認

## Performance / Security Verification

- セキュリティ（NFR2）: OSC 0/2 由来タイトルが notify_rust の `.body()` に到達する全経路（タブアクティビティ / エージェント / OSC 9 / link_hover 由来）が単一チョークポイント `NotifyRustSink::send` で覆われる — TS5 + レビューでの経路検査
- パフォーマンス: 数値要件なし。送出ごとの `get_capabilities()` 問い合わせは既存スロットルで頻度が抑えられる（IMPLEMENTATION.md D2）

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2（main / cli） | 2 | 0 | 0 |
| Unit / Integration | TS1〜TS5 | 5 | 0 | 0 |
| Regression | TS6, TS7 | 2 | 0 | 0 |
| Feature gate | TS8 | 1 | 0 | 0 |
| Code inspection | TS9 | 0 | 0 | 1 |
| Format | 1 | 1 | 0 | 0 |
