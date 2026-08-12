---
title: "notification-summary-markup-escape"
created_date: 2026-08-12
status: draft
---

# notification-summary-markup-escape - 要件定義書

## 1. 概要

### 1.1 背景

dunst の `markup=full` は通知の summary に含まれるマークアップを解釈する。攻撃者が制御しうるタイトル（OSC 9 のタイトル、OSC 0/2 由来のタブタイトルによるフォールバック）が summary でマークアップとして解釈される。PR #35 は body に対して同種の保護を与えたが、summary は未対応のまま残っている。

出典: PR [https://github.com/m-m-n/emterm/pull/35](https://github.com/m-m-n/emterm/pull/35) review round1 の指摘 `11996759d76a5041`（severity medium / category security）。

### 1.2 目的

通知 summary におけるマークアップ解釈を通じた通知フィッシングの経路を塞ぐ。攻撃者が制御しうるタイトルが summary でマークアップとして解釈されない状態にし、PR #35 が body に与えた保護と同等の保護を summary にも与える。

### 1.3 スコープ

**対象**

- FR1: 通知 summary のマークアップメタ文字の無害化
- FR2: OSC 9 タイトル経路のエンドツーエンドでのカバー
- FR3: body 側エスケープの現状維持
- NFR1: 単一の egress 点での適用
- NFR2: プラットフォームスコープ（`#[cfg(unix)]`）

**対象外**

- body 側のエスケープ（PR #35 で対応済み）
- capability 取得失敗時の fail-open（別タスク）
- `sanitize_title` の変更（方針 (a) を選んだため）

## 2. ビジネス要件

### 2.1 ビジネス目標

- dunst の `markup=full` が通知 summary のマークアップを解釈する点に起因する通知フィッシングの攻撃面を塞ぐ。攻撃者が制御しうるタイトル（OSC 9 のタイトル、OSC 0/2 のタブタイトルによるフォールバック）が summary でマークアップとして解釈されないようにし、PR #35 が body に与えた保護に揃える。

### 2.2 対象ユーザー

requirements_analysis に対象ユーザーの記述はない。

### 2.3 期待される効果

- 攻撃者が制御しうるタイトルが summary でマークアップとして解釈されなくなる。
- summary と body の保護水準が揃う。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | OSC 9 による通知送出 | PTY 上のプロセス（攻撃者が制御しうる出力元） | 高 |
| UC02 | タイトル省略時のフォールバック通知送出 | PTY 上のプロセス（攻撃者が制御しうる出力元） | 高 |

### 3.2 ユースケース詳細

#### UC01: OSC 9 による通知送出

**アクター**: PTY 上のプロセス

**事前条件**:

- Unix プラットフォームで動作している（`#[cfg(unix)]` スコープ）。
- 通知サーバの capability を `body_markup_confirmed(get_capabilities())` が「confirmed」と解決する。

**基本フロー**:

1. PTY 上のプロセスが OSC 9 シーケンスを出力する。
2. `parse_osc9`（`src-tauri/src/callbacks.rs:681-693`）がタイトルと本文を取り出す。
3. `handle_notify`（`src-tauri/src/callbacks.rs:451-462`）が `sink.send` を呼ぶ。
4. `NotifyRustSink::send`（`src-tauri/src/callbacks.rs:148-174`）が capability ゲートを評価する。
5. capability が confirmed のとき、summary（title）にも `escape_body_markup` を適用したうえで D-Bus に送出する。

**代替フロー**:

- capability が unconfirmed の場合、title はバイト単位で変更されないまま送出される（body 経路と同じ fail-open 挙動）。

**事後条件**:

- D-Bus に渡る summary にマークアップメタ文字が生の形で含まれない（capability confirmed 時）。

#### UC02: タイトル省略時のフォールバック通知送出

**アクター**: PTY 上のプロセス

**事前条件**:

- UC01 と同じ。

**基本フロー**:

1. OSC 9 のタイトルセグメントが空である。
2. フォールバックとして現在のタブタイトル（OSC 0/2 由来で untrusted）、それも無い場合は `"emterm"` が使われる。
3. UC01 と同じ経路で `NotifyRustSink::send` に到達し、同じエスケープ済み summary 経路を通る。

**事後条件**:

- フォールバック由来のタイトルも同じくエスケープされて送出される。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 通知 summary のマークアップメタ文字の無害化 | capability confirmed 時、summary（title）に `escape_body_markup` を適用する | 高 |
| FR2 | OSC 9 タイトル経路のエンドツーエンドでのカバー | `parse_osc9` → `handle_notify` → `sink.send` の経路とフォールバック分岐を含めてカバーする | 高 |
| FR3 | body 側エスケープの現状維持 | body 側の挙動をバイト単位で変更しない | 高 |

### 4.2 機能詳細

#### FR1: 通知 summary のマークアップメタ文字の無害化

**説明**: `NotifyRustSink::send`（`src-tauri/src/callbacks.rs:148-174`）において、既存の送出ごとの capability ゲート `body_markup_confirmed(get_capabilities())` が「confirmed」と解決したとき、body に対して既に行っているのと同じ形で summary（title）にも `escape_body_markup` を適用する。適用範囲は既存の `#[cfg(unix)]` スコープと同一。エスケープされていない `.summary(title)`（`src-tauri/src/callbacks.rs:166`）が修正箇所である。`sanitize_title`（`src-tauri/src/notifications.rs:145-159`）は変更しない（回答済みゲートに従い方針 (a) を採用）。

**入力**:

- title: `&str` - 通知の summary となるタイトル（OSC 9 由来、またはフォールバック）
- capability: `body_markup_confirmed(get_capabilities())` の解決結果

**出力**:

- summary: `String` - capability confirmed 時は `escape_body_markup` 適用後の文字列、unconfirmed 時は入力のまま

**処理フロー**:

```mermaid
flowchart TD
    A[NotifyRustSink::send] --> B{body_markup_confirmed(get_capabilities())}
    B -->|confirmed| C[escape_body_markup を title と body に適用]
    B -->|unconfirmed| D[title と body をそのまま使用]
    C --> E[summary/body を設定して D-Bus へ送出]
    D --> E
```

**ビジネスルール**:

- エスケープの適用点は D-Bus egress の 1 箇所のみ（NFR1）。
- エスケープ順序は body 経路と同じ `&` → `<` → `>`（FR3）。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| capability が confirmed に解決しない | `body_markup_confirmed(get_capabilities())` が unconfirmed | title をバイト単位で変更せず送出する（body 経路と同じ fail-open。capability 取得失敗時の fail-open 自体は本タスクの対象外） |

#### FR2: OSC 9 タイトル経路のエンドツーエンドでのカバー

**説明**: エスケープは `parse_osc9`（`src-tauri/src/callbacks.rs:681-693`）から `handle_notify`（`src-tauri/src/callbacks.rs:451-462`）を経て `sink.send` に至るタイトルをカバーし、フォールバック分岐（untrusted な OSC 0/2 由来の現在のタブタイトル、または `"emterm"`）も含む。単一の D-Bus egress でエスケープすることで、他の summary 生成元（タブアクティビティ、エージェント状態、リンクホバー）も同時にカバーされる。これは PR #35 の D1 単一チョークポイント設計と一致する。

#### FR3: body 側エスケープの現状維持

**説明**: 既存の body エスケープ挙動（エスケープ順序 `&` → `<` → `>`、capability ゲート、unconfirmed 時の fail-open）はバイト単位で変更しない。`src-tauri/src/callbacks/tests.rs` にある PR #35 の既存テストは引き続き成功する。

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 名称 | 内容 |
|----|------|------|
| NFR1 | 単一 egress 点 | エスケープは唯一の D-Bus egress である `NotifyRustSink::send` で 1 度だけ適用し、生成元ごとのエスケープは導入しない |
| NFR2 | プラットフォームスコープ | 修正は既存の `#[cfg(unix)]` ゲートの中に置く。Windows のトースト経路は変更しない（そちらでは notify-rust が `get_capabilities()` を公開していない） |

### 5.2 セキュリティ要件

- 入力検証: capability confirmed 時、summary に含まれるマークアップメタ文字を `escape_body_markup` で無害化する（FR1）。
- 適用点: 唯一の D-Bus egress で 1 度だけ適用する（NFR1）。

### 5.3 互換性要件

- プラットフォーム: `#[cfg(unix)]` のみ。Windows のトースト経路は変更しない（NFR2）。

### 5.4 その他の非機能要件

パフォーマンス・可用性・保守性について requirements_analysis に記述はない。

## 6. UI/UX要件

該当なし。design step は skipped（理由: Rust の通知 egress に閉じたバックエンドのみのセキュリティ修正であり、UI 面も visual input も無い）。

## 7. データ要件

requirements_analysis にデータモデル・保持期間の記述はない。

## 8. 外部連携

### 8.1 連携システム

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| デスクトップ通知サーバ（dunst 等） | D-Bus（notify-rust 経由） | summary（title）、body、capability 情報 |

### 8.2 API仕様要件

- `get_capabilities()` の結果を `body_markup_confirmed` で判定し、confirmed のときのみエスケープを適用する（既存の送出ごとのゲートをそのまま使用）。
- Windows 側では notify-rust が `get_capabilities()` を公開していないため、この判定経路は存在しない。

## 9. 制約条件

### 9.1 技術的制約

- 修正は `#[cfg(unix)]` スコープ内に置く。
- `sanitize_title`（`src-tauri/src/notifications.rs:145-159`）は変更しない。
- 内部の複製（`src-tauri/src/callbacks.rs:454-457` の `pending_notifications`、レートリミッタのキー）は生のタイトルを保持し、無害化は D-Bus egress でのみ行う。

### 9.2 ビジネス上の制約

requirements_analysis に記述はない。

### 9.3 スケジュール制約

requirements_analysis に記述はない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| summary をマークアップ描画しない仕様準拠のサーバでは、エスケープ結果（例: `&lt;`）がそのまま表示される表示劣化が生じる | 中 | 方針 (a)（escape-at-sink）採用に伴う受容済みのトレードオフとする |

### 10.2 ビジネスリスク

requirements_analysis に記述はない。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] タグを含むタイトル（例: `<a href>` を含むもの）が、capability が body-markup を confirm したとき summary でエスケープされ、unconfirmed のときは変更されない（body 経路と同じ fail-open）ことをユニットテストで固定する。
- [ ] PR #35 の既存の body エスケープテストが変更なしで引き続き成功する。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が成功する。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | 対象要件 | シナリオ |
|----|----------|----------|
| TS1 | FR1 | `<`、`>`、`&` を含むタイトルへの `escape_body_markup` 適用が、body 経路と同じエンティティ出力になる（`&` を先に処理する順序。既存エンティティの二重エスケープは許容し、`src-tauri/src/callbacks/tests.rs:695` に倣う） |
| TS2 | FR1, FR3 | sink の判定を組み合わせた確認 — capability confirmed でタイトルがエスケープされ、unconfirmed ではバイト単位で変更されない（`src-tauri/src/callbacks/tests.rs:751` に倣う） |
| TS3 | FR1 | `sanitize_title` により 100 文字に切り詰められ末尾が `<` になったタイトルが、完結したエンティティにエスケープされる（切り詰めの後にエスケープ。body について同じ合成を固定している `src-tauri/src/callbacks/tests.rs:707` に倣う） |
| TS4 | FR2 | OSC 9 のフォールバックタイトル分岐（タイトルセグメントが空 → タブタイトルまたは `"emterm"`）が、同じエスケープ済み summary 経路を通る |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| summary | 通知のタイトルにあたるフィールド。dunst の `markup=full` ではマークアップが解釈される |
| body | 通知の本文フィールド。PR #35 でエスケープ済み |
| `escape_body_markup` | マークアップメタ文字をエンティティへ変換する既存関数（`src-tauri/src/callbacks.rs:183-188`）。順序は `&` → `<` → `>` |
| `body_markup_confirmed` | `get_capabilities()` の結果から body-markup 対応可否を判定する送出ごとの capability ゲート（`src-tauri/src/callbacks.rs:156-163`） |

## 14. 確認事項

### 14.1 確認済み事項

- [x] タイトルのエスケープ方針（`requirement.title-escape-approach`）: 方針 (a) escape-at-sink を採用。capability 確認済みのとき `NotifyRustSink::send` で `summary(title)` にも `escape_body_markup` を適用し、`sanitize_title` は変更しない。batch モードで Codex 相談（packet `create-spec-q0001`、source `batch-codex-consultation`、option `escape-at-sink`）により解決したものであり、ユーザーによる回答ではない。`batch-policies.yaml` の `record_as_assumption: true` に従い前提として記録する。

### 14.2 前提（想定）事項

- `assume.approach-a-escape-at-sink`: 上記のとおり方針 (a) escape-at-sink を採用。受容したトレードオフは、summary をマークアップ描画しない仕様準拠のサーバでの表示劣化（`&lt;` がそのまま表示される）。可逆。
- `assume.unix-only-gate`: 修正は既存の body エスケープと同じ `#[cfg(unix)]` スコープに適用し、Windows のトースト経路は変更しない。可逆。
- `assume.escape-at-egress-only`: 内部の複製（`src-tauri/src/callbacks.rs:454-457` の `pending_notifications`、レートリミッタのキー）は生のタイトルを保持し、無害化は D-Bus egress でのみ行う。可逆。

### 14.3 未確認・保留事項

なし。

## 15. 参考資料

- 修正箇所: `src-tauri/src/callbacks.rs:166`（`.summary(title)`）、capability ゲート `src-tauri/src/callbacks.rs:156-163`、`escape_body_markup` `src-tauri/src/callbacks.rs:183-188`、`parse_osc9` `src-tauri/src/callbacks.rs:681-693`、`handle_notify` `src-tauri/src/callbacks.rs:451-462`
- 変更しない関数: `sanitize_title` `src-tauri/src/notifications.rs:145-159`
- 参照する既存テスト: `src-tauri/src/callbacks/tests.rs:695`、`:707`、`:751`、`:772`
- 発端: PR [https://github.com/m-m-n/emterm/pull/35](https://github.com/m-m-n/emterm/pull/35) review round1、指摘 `11996759d76a5041`（severity medium / category security）
