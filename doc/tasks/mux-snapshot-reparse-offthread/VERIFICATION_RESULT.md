# 🔍 検証結果レポート: mux-snapshot-reparse-offthread

**検証日時**: 2026-06-18 20:22 JST
**対象機能**: mux 切替時 snapshot 再パースコストの計測と判断
**VERIFICATION.md**: `doc/tasks/mux-snapshot-reparse-offthread/VERIFICATION.md`
**ブランチ**: refactor/promote-native-poc

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド | ✅ | `cargo check` / CLI-only / full release(lto) すべて exit 0 |
| テスト実行 | ✅ | default suite 1796 passed / 0 failed / 1 ignored、term_core 643 passed / 0 failed |
| コードフォーマット | ✅ | `cargo fmt --check` 差分なし（src-tauri / term_core） |
| 静的解析 | ✅ | 本 feature 由来の警告なし（sgr.rs の既存 collapsible_match のみ） |
| ファイル構造 | ✅ | 変更ファイル・doc 一式すべて存在 |
| SPEC.md 適合性 | ✅ | FR1 / FR3 / NFR1-3 達成、FR2 判断確定（下記） |

**総合評価**: ✅ すべて合格

---

## ✅ SPEC.md 適合性（FR / NFR）

| 要件 | 状態 | 検証 |
|------|------|------|
| FR1 計測ハーネス | ✅ | `terminal_core.rs` に決定的合成 scrollback ビルダ＋`#[ignore]` 計測ハーネス＋空入力ガード。`test_synthetic_scrollback_is_deterministic` / `test_reparse_empty_input_no_panic` green、`measure_reparse_cost_2mib` は `#[ignore]` でデフォルト除外（TS-1/2/5） |
| FR2 go/no-go 判断 | ✅ | release 計測を §4 しきい値にマッピングし判断を記録（下記「FR2 判断」、TS-1/7） |
| FR3 ロックスコープ guard-rail | ✅ | `handle_request_pane_snapshot` で scrollback ガードを明示スコープブロック化（`handlers.rs:473`）＋不変条件コメント。byte-identity 回帰テスト `snapshot_bytes_unchanged_after_lock_scope_guardrail` green（TS-3/4） |
| NFR1 決定性/隔離 | ✅ | 計測は固定合成入力・`term_core` 直接呼び出し（`pump_all` 非経由）（TS-2/5） |
| NFR2 無回帰/CI 衛生 | ✅ | FR3 byte-identical、計測はデフォルト suite 非実行、CLI-only green（TS-4/5/6） |
| NFR3 移植性 | ✅ | term_core + src-tauri、Linux/Windows・CLI-only ビルド維持。full release(lto) も成功（TS-6） |

---

## 🎯 FR2 判断: **GO（案a を実装）**

### 計測（release ビルド・`[profile.release] lto = true`・実機）

`cargo test -p term_core --release -- --ignored --nocapture`（target-host）:

| サイズ | バイト数 | 所要 | スループット |
|--------|----------|------|--------------|
| 256 KiB | 262,144 | 30.080 ms | 8.3 MiB/s |
| 1 MiB | 1,048,576 | 116.590 ms | 8.6 MiB/s |
| **2 MiB** | 2,097,152 | **232.502 ms** | 8.6 MiB/s |

ほぼ線形（~8.6 MiB/s）。参考: debug ビルドでは 2 MiB = 2127 ms（release で約9倍速）。

### §4 しきい値へのマッピング

- `< 5 ms`: 案a 見送り
- `5–50 ms`: グレー域
- `50 ms+`: 案a を実装 ← **該当**

`process_pty_data_fully`（= `reset_and_replay` の支配的コスト）は `App::pump_all`＝
winit イベントループ（UI/描画スレッド）から同期実行される。release 実機でも:

- **2 MiB 履歴の pane へ切替: 毎回 ~232ms UI スレッドブロック**（明確に体感できるジャンク）
- 1 MiB でも ~116ms、256 KiB でも ~30ms（グレー域）

`Ctrl+B n n n` のような高速切替では履歴の多い pane でフリーズが積み上がる。50ms しきい値を
大きく超えるため、**案a（off-thread replay）の本実装を follow-up SDD feature として起票する**
判断とする。

### 留意（実運用での頻度）

コストは実履歴量に比例。履歴の小さい pane（数十 KiB）は ~数 ms で問題にならない。
影響を受けるのは大量出力を流した pane（ビルドログ・長時間セッション等）。これは「常に重い」
わけではないが、対象 pane に切り替えるたびに UI が止まるため UX 影響は大きい。

### follow-up feature のスコープ（確定済み方針）

- **案a: off-thread replay**（ワーカースレッドで `TerminalCore` を構築→メインで差し替え）。
- **コアのみ**。直近 K pane の **LRU キャッシュは含めない**（本 feature で不採用決定済み）。
- 案c（pane 毎常駐 core）は不採用。
- 実装時の難所は IMPLEMENTATION.md / 設計メモ `tmp/perf-snapshot-reparse-offthread-plan.md`
  §2.2 を参照（pending-switch 状態・ライブ bytes のキューイング順序・marks/folds/selection の
  swap 後整合・grid サイズ整合・pump_all への非同期混入によるテスト flaky 化への対処）。

推奨 feature 名: `mux-snapshot-reparse-offthread-impl`（または `mux-offthread-replay`）。

---

## 🐳 E2E テスト

該当なし（Rust の perf/計測 feature。プロジェクト E2E フレームワーク非関与）。

---

## 📋 手動確認が必要な項目

- [ ] （任意）release バイナリ `src-tauri/target-host/release/emterm` で、大量出力を流した
      pane への切替時のジャンクを体感確認（計測値の裏付け）。

---

## ⚠️ 本 feature 外の発見と対応（記録）

実装中、`cargo test -p term_core` が pre-existing な破損でコンパイル不能だったことが判明:

- `crates/term_core/src/parser/tests.rs` の colon サブパラメータテストが未定義の
  `parser_params::SUB_PARAM_FLAG` を参照（3件コンパイル不能）。さらに回帰ガード
  `test_parse_csi_colon_does_not_leak_text` が **実在のレンダリングバグ**を捕捉していた:
  現行パーサーが colon 形式 SGR（`38:5:n` 等）の `:` で CSI をキャンセルし残りをテキスト表示
  （`38:5:196mX` → 7 actions、`5:196m` が画面に漏れる）。
- プロジェクト標準の `cargo test --manifest-path src-tauri/Cargo.toml` は term_core を
  依存ライブラリとしてのみビルドし test 標的をビルドしないため見逃されていた。

**対応**: 本 feature とは無関係なので**別コミット `0b77717`** で修正（`:` を `;` 同様に param
区切りとして消費＝テキスト漏れ解消、colon 4テストを collapse 表現に書き直して全 pass）。
完全な ISO 8613-6 colon サブパラメータ意味論（`4:3` 下線スタイル・`38:2::r:g:b` 等）は
別 follow-up として `tmp/colon-subparam-full-iso-followup.md` に記録。

---

## 🎯 総合評価

✅ **すべての自動検証項目をクリア。FR2 判断 = GO（案a を follow-up feature で実装）。**

- FR1 計測ハーネス・FR3 ロックスコープ guard-rail は実装・テスト済み。
- FR2 は release 実機計測（2 MiB = 232ms、50ms しきい値を大きく超過）に基づき案a 実装を決定。
- 本 feature の変更は `crates/term_core/src/terminal_core.rs`（FR1）と
  `src-tauri/src/mux/ipc/handlers.rs`（FR3）の2ファイル（＋doc）に純化。colon SGR 修正は
  別コミット `0b77717` に分離済み。
