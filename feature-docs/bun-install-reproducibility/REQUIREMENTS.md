---
title: "bun-install-reproducibility"
created_date: 2026-09-05
status: draft
---

# bun-install-reproducibility - 要件定義書

## 1. 概要

### 1.1 背景

`bun install` の解決結果が環境ごとに揺れるため、`bun test` の結果が「どのワークツリー・どのランナーで実行したか」に依存している。新しく作成したワークツリーでは、`bun.lock` が `.gitignore:42`（`# Bun` 見出しの下）で無視されているため `package-lock.json` からの再マイグレーションが走り、依存グラフが再解決される。その結果、viewer entry テストのうち 2 件が失敗する。

失敗する 2 件は次のとおり。

- `renders an injected sample into the fullscreen content structure`（`src-tauri/viewer/web/entry.test.ts:55`）
- `parses the shared Rust/TS payload fixture with all fields`（`src-tauri/viewer/web/entry.test.ts:108`）

いずれも dompurify 3.4.x が `MarkdownRenderer.render()` の出力から先頭の `<h1>` を落とすことで、`h1` の textContent を検証するアサーションが通らなくなっている。sanitize の呼び出しは `src-tauri/web-shared/markdown/renderer.ts:216` の `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)` で、`test-setup.ts` 経由の happy-dom 上で実行される。

`marked` と `happy-dom` のバージョン差分は、報告者が個別ピン留めによってすでに除外済みである（ASM-6）。

### 1.2 目的

- `bun install` がどの環境でも同一の依存グラフに解決されるようにする。
- verify フェーズで毎回発生している「この 2 件の失敗は変更と無関係」というトリアージのコストを取り除く。このトリアージは本物のリグレッションを覆い隠すリスクを抱えている。
- 既存の h1 サニタイズのリグレッション検知を維持したまま、dompurify 3.4.x を安全に採用できる状態に到達する。サニタイザを 3.3.1 に無期限で固定することはしない。

### 1.3 スコープ

**対象**:

- `bun.lock` のコミットと `.gitignore` からの除去（FR1）
- `package-lock.json` の削除（FR2、ASM-2）
- クリーンなワークツリーでの viewer entry テストの成功（FR3）
- dompurify 3.4.x が `h1` を落とす機構の特定と記録（FR4）
- dompurify 3.4.x を安全に採用できる状態への到達（FR5）
- 既存 h1 アサーションの維持（FR6）
- CI におけるクリーンインストール + テスト実行経路（FR7、ASM-3）
- CI での frozen-lockfile による lockfile ドリフト検知（FR8）

**対象外**:

- `plugin/marketplace version regression guard (task0002 AC-9)` の 2 件の失敗。これらは `main` でも失敗しており、別issueとして本フィーチャーの範囲外（ASM-4）。

**デザインステップ**: 実施しない（skipped）。理由は依存解決と CI 再現性の作業であり、ユーザーから見える表面を持たないため。`resolved_input_paths.visual_inputs` は空で、デザインシステムのファイルは本変更の対象に含まれない。触れる可能性のある唯一のランタイム経路（`src-tauri/web-shared/markdown/renderer.ts` のマークダウンサニタイズ）は、FR6 / NFR1 / NFR2 により出力を「変えない」ことが要求されている。バッチポリシーが `create-spec.design-step` を `decide_autonomously` に解決したため、この判断がそのまま決定となる。

## 2. ビジネス要件

### 2.1 ビジネス目標

1. `bun install` がどの環境でも同一の依存グラフに解決されるようにし、`bun test` の結果がどのワークツリー・どのランナーで実行されたかに依存しない状態にする。
2. verify フェーズで繰り返し発生している viewer entry テスト 2 件の「変更とは無関係」というトリアージのコストを取り除く。現状このトリアージは本物のリグレッションを覆い隠すリスクを抱えている。
3. 既存の h1 サニタイズのリグレッション検知を維持したまま、dompurify 3.4.x を安全に採用できる状態に到達する。サニタイザを 3.3.1 に固定し続けることはしない。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm の開発者 | ワークツリーを新規作成して `bun install` / `bun test` を実行する。現状クリーンなワークツリーでは viewer entry テストが 2 件失敗する |
| verify フェーズの検証担当 | テスト結果を判定する。既知の失敗と本物のリグレッションを切り分けるトリアージのコストを負っている |
| CI | クリーンチェックアウトから依存をインストールしテストを実行する |

### 2.3 期待される効果

- 同一コミットからの 2 回の独立したクリーンインストールが、同一の dompurify バージョンに解決される。
- クリーンなワークツリーでの `bun test src-tauri/viewer/web/entry.test.ts` が 14 pass / 0 fail になる。
- `package.json` を変更して `bun.lock` を再生成し忘れた場合、CI が黙って別のグラフを解決するのではなく明示的に失敗する。
- dompurify 3.4.x の h1 消失機構が観測に基づいて記録され、盲目的な回避策ではなく根拠のある対応になる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | クリーンなワークツリーで依存をインストールしテストを実行する | eMterm の開発者 | 高 |
| UC02 | CI がクリーンチェックアウトでロック済みグラフをインストールしテストを実行する | CI | 高 |
| UC03 | 依存バージョンの変更が lockfile 再生成なしにコミットされたことを検知する | CI | 高 |

### 3.2 ユースケース詳細

#### UC01: クリーンなワークツリーで依存をインストールしテストを実行する

**アクター**: eMterm の開発者

**事前条件**:

- `bun.lock` が git 管理下にあり、`.gitignore` に `bun.lock` のエントリが存在しない（FR1 / AC-2）
- `package-lock.json` がリポジトリに存在しない（FR2 / AC-3）

**基本フロー**:

1. `git worktree add` でワークツリーを新規作成する
2. `bun install` を実行する
3. `bun test src-tauri/viewer/web/entry.test.ts` を実行する
4. 14 pass / 0 fail が報告される（FR3 / AC-1 / TS-1）

**代替フロー**:

- 同一コミットから 2 つの別ワークツリーへインストールした場合、解決される `dompurify` のバージョン文字列は一致する（NFR4 / AC-4 / TS-2）

**事後条件**:

- `entry.test.ts:55` と `entry.test.ts:108` を含む 14 件が成功している

#### UC02: CI がクリーンチェックアウトでロック済みグラフをインストールしテストを実行する

**アクター**: CI

**事前条件**:

- ロック済みの依存グラフがコミットされている

**基本フロー**:

1. CI がクリーンチェックアウトを行う
2. frozen-lockfile インストール（`bun install --frozen-lockfile`）を実行する（FR8）
3. `bun test` を実行し、`src-tauri/viewer/web/entry.test.ts` がその実行に含まれる（FR7 / AC-7 / TS-8）

**代替フロー**:

- 該当のワークフローが存在しない場合、テスト実行経路を追加する。`.github/workflows/release.yml`（解析に供給された唯一のワークフロー）は 218 行目（build-linux）と 316 行目（build-windows）で `bun install` を実行するが、テストコマンドは一切実行していない（FR7 / ASM-5）

**事後条件**:

- CI の実行ログにクリーンインストールと `bun test` のステップ、および viewer entry テストの出力が現れる

#### UC03: 依存バージョンの変更が lockfile 再生成なしにコミットされたことを検知する

**アクター**: CI

**事前条件**:

- CI のインストール経路が frozen-lockfile インストールを使用している（FR8）

**基本フロー**:

1. `package.json` の依存レンジが変更され、`bun.lock` が再生成されないままコミットされる
2. CI が `bun install --frozen-lockfile` を実行する
3. インストールステップが非ゼロ終了で失敗する（AC-8 / TS-3）

**事後条件**:

- 異なるグラフが黙って解決されることなく、実行が失敗している

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | bun lockfile のコミット | `bun.lock` を `.gitignore` から外し、生成済み lockfile をコミットする | resolved |
| FR2 | 陳腐化した package-lock.json の削除 | `package-lock.json` をリポジトリから削除する | resolved |
| FR3 | クリーンなワークツリーでの viewer entry テスト成功 | 新規ワークツリーでの `bun install` 後に entry テストが 14 pass / 0 fail になる | resolved |
| FR4 | dompurify 3.4.x の h1 消失機構の特定 | h1 が落ちる機構を観測により特定し記録する | resolved |
| FR5 | dompurify 3.4.x を安全に採用できる状態への到達 | FR4 の機構判明後、3.4.x を採用できる状態にする | resolved |
| FR6 | h1 リグレッション検知の維持 | 既存の h1 アサーションを弱めず維持する | resolved |
| FR7 | CI でのクリーン・ロック済みインストール上でのテスト実行 | CI がロック済みグラフのクリーンインストール上で `bun test` を実行する | resolved |
| FR8 | lockfile ドリフトの CI での明示的失敗 | frozen-lockfile インストールでドリフトを失敗させる | resolved |

### 4.2 機能詳細

#### FR1: bun lockfile のコミット

**説明**: `bun.lock` を `.gitignore`（現在 `.gitignore:42` の `# Bun` 見出しの下に記載）から削除し、生成された lockfile をコミットする。これにより、新規ワークツリーでの `bun install` が `package-lock.json` からの再マイグレーションではなく、凍結されたグラフから解決するようになる。

**ビジネスルール**:

- `bun.lock` は git により追跡される（AC-2）
- `.gitignore` に `bun.lock` のエントリは存在しない（AC-2）

**検証**: `git check-ignore bun.lock` がマッチするルールを見つけず、`git status` が lockfile を untracked ではなく tracked として示す（TS-9）

#### FR2: 陳腐化した package-lock.json の削除

**説明**: `package-lock.json` をリポジトリから削除する。bun lockfile が JavaScript 依存グラフの単一の情報源となり、2 つ目の JS lockfile は再導入しない。

**ビジネスルール**:

- `package-lock.json` は作業ツリーからもコミット済みインデックスからも存在しない（AC-3）
- 2 つ目の JS lockfile を再導入しない

#### FR3: クリーンなワークツリーでの viewer entry テスト成功

**説明**: 新規に作成したワークツリーでの `bun install` 後、`bun test src-tauri/viewer/web/entry.test.ts` が 14 pass / 0 fail を報告する。ここには `renders an injected sample into the fullscreen content structure`（`entry.test.ts:55`）と `parses the shared Rust/TS payload fixture with all fields`（`entry.test.ts:108`）が含まれる。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| 上記 2 件の失敗 | 依存グラフが再解決され dompurify 3.4.x が入る | FR1 / FR2 によるグラフの凍結、および FR4 / FR5 による機構の特定と採用可能状態への到達 |

#### FR4: dompurify 3.4.x の h1 消失機構の特定

**説明**: dompurify 3.4.x が `MarkdownRenderer.render()` の出力から先頭の `<h1>` を落とす機構を、盲目的な回避ではなく観測によって特定し記録する。対象の呼び出しは `src-tauri/web-shared/markdown/renderer.ts:216` の `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)` で、`test-setup.ts` 経由の happy-dom 上で実行される。

**特定は次の 3 つを区別する**:

1. dompurify の挙動変更
2. 3.4.x が異なる解釈をする `PURIFY_CONFIG` のオプション
3. happy-dom と dompurify の相互作用

**検証**: 上記 3 つの候補レイヤのどれが原因かを特定する再現可能な観測とともに記録される（AC-5）

#### FR5: dompurify 3.4.x を安全に採用できる状態への到達

**説明**: FR4 の機構が判明した後、h1 アサーションが成功し、かつサニタイズの厳格性が変わらない状態で dompurify 3.4.x を採用できる状態にプロジェクトを持っていく。コミットされる lockfile に記録されるバージョンはその結果を反映する。3.4.x をまだ採用できない場合、ピン留めされたバージョンは説明のないピンではなく FR4 の知見を理由として伴う。

#### FR6: h1 リグレッション検知の維持

**説明**: 次のアサーションをそのまま維持する。

- `expect(content.querySelector("h1")?.textContent).toContain("Title")`（`entry.test.ts:67`）
- `...toContain("Hi")`（`entry.test.ts:128`）

**ビジネスルール**:

- これらを弱める・緩める・削除することは FR3 到達の手段として許容されない
- マッチャもセレクタも緩和しない（AC-6）

#### FR7: CI でのクリーン・ロック済みインストール上でのテスト実行

**説明**: CI がロック済み依存グラフのクリーンインストール上で `bun test`（`src-tauri/viewer/web/entry.test.ts` を含む）を実行する。`.github/workflows/release.yml`（解析に供給された唯一のワークフロー）は 218 行目（build-linux）と 316 行目（build-windows）で `bun install` を実行するが、テストコマンドを一切実行していない。したがって本要件の充足には、既存経路の確認ではなくテスト実行経路の追加が必要となる。

**ビジネスルール**:

- 実装はテストステップの配置先を決める前に `.github/workflows/` を列挙し、`bun test` を既に実行している他のワークフローが存在するかを確認する（ASM-5）

#### FR8: lockfile ドリフトの CI での明示的失敗

**説明**: CI のインストール経路は frozen-lockfile インストール（`bun install --frozen-lockfile`）を使用する。これにより、`bun.lock` を再生成せずに `package.json` の変更がコミットされた場合、異なるグラフが黙って解決されるのではなく実行が失敗する。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| インストールステップの失敗 | `package.json` の依存編集が `bun.lock` 再生成なしでコミットされた | 実行を失敗させて先へ進めない（AC-8 / TS-3） |

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし。本フィーチャーはパフォーマンス目標を持たない。

### 5.2 セキュリティ要件

**NFR1: サニタイズの厳格性を緩めない**

`PURIFY_CONFIG` の `ALLOWED_TAGS` / `FORBID_TAGS` / `FORBID_ATTR` / `ALLOWED_URI_REGEXP` に対する、子 WebView の DOM に到達しうる範囲を広げる変更は、修正手段として許容されない。子 WebView における XSS 保護は製品の柱として明示されている（CLAUDE.md「Robust isolation」）。

**検証**: `PURIFY_CONFIG` に、変更前ファイルと比べて新たに許可されたタグ・属性がなく、削除された `FORBID_*` エントリもない（AC-9）

### 5.3 可用性要件

該当なし。本フィーチャーは稼働率・障害復旧時間の目標を持たない。

### 5.4 保守性要件

**NFR4: 再現性は主張ではなく検証可能であること**

再現性の主張は、`package.json` のレンジの目視確認ではなく、同一コミットからの 2 回の独立したクリーンインストールが同一の dompurify バージョンに解決されることによって実証される。

**検証**: 2 つの別ワークツリーでの同一コミットからのクリーンインストールが、`dompurify` を同一のバージョン文字列に解決する（AC-4 / TS-2）

### 5.5 互換性要件

**NFR2: 現在成功しているケースの出力は変えない**

既に成功している 12 件の viewer entry テスト、および `bun test` スイートの残りは成功したままとする。`entry.test.ts` で検証されている front-matter、outline、theme-token、MD3-token-parity の各挙動は変更しない。

**NFR3: GUI ビルドの入力が両プラットフォームでビルド可能なままであること**

`bun run build:viewer` と `bun run build:settings` は、`src-tauri/build.rs` が埋め込むバンドルを引き続き生成する。これは Linux と Windows の CI 経路の双方で成り立つ（`release.yml:318-321` が Windows でバンドルをビルドし、`scripts/build-dpkg.sh` が Linux をカバーする）。

## 6. UI/UX要件

該当なし。本フィーチャーは依存解決と CI 再現性の作業であり、ユーザーから見える表面を持たない。

## 7. データ要件

該当なし。本フィーチャーはデータモデルを導入・変更しない。

## 8. 外部連携

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| GitHub Actions | ワークフロー定義（`.github/workflows/`） | クリーンチェックアウト、frozen-lockfile インストール、`bun test` の実行 |

## 9. 制約条件

### 9.1 技術的制約

- 子 WebView の XSS 保護は製品の柱であり、サニタイズの厳格性を緩めることはできない（NFR1）
- 既存の h1 アサーションを弱める・削除することは FR3 の達成手段として使えない（FR6）
- `.github/workflows/release.yml` はテストコマンドを一切実行していないため、FR7 の充足にはテスト実行経路の追加が必要（FR7 / ASM-5）
- GUI ビルドは `viewer/dist` / `settings/dist` を `src-tauri/build.rs` で埋め込むため、バンドル生成が壊れてはならない（NFR3）

### 9.2 ビジネス上の制約

- サニタイザを 3.3.1 に無期限で固定する選択は取らない（ビジネス目標 3）

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:

- `feature-docs/bun-install-reproducibility/**`
- `test-docs/bun-install-reproducibility/**`

`feature-docs/bun-install-reproducibility/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/bun-install-reproducibility/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/bun-install-reproducibility/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:

- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/bun-install-reproducibility/` ディレクトリを生成しないが、宣言された `test-docs/bun-install-reproducibility/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| dompurify 3.4.x の h1 消失機構が、dompurify 挙動変更 / `PURIFY_CONFIG` オプション解釈 / happy-dom 相互作用のいずれか不明 | 高 | 観測により 3 候補レイヤを切り分けて記録する（FR4 / AC-5） |
| 3.4.x をまだ採用できない可能性 | 中 | ピン留めしたバージョンに FR4 の知見を理由として記録する（FR5） |
| `bun test` を実行するワークフローが供給された入力の中に存在しない | 中 | 実装が `.github/workflows/` を列挙してから配置先を決める（FR7 / ASM-5） |

### 10.2 ビジネスリスク

| リスク | 影響度 | 対応策 |
|--------|--------|--------|
| 既知の 2 件失敗のトリアージが本物のリグレッションを覆い隠す | 高 | クリーンインストールでの 14 pass / 0 fail を達成し、トリアージ自体を不要にする（ビジネス目標 2 / FR3） |
| `package.json` 変更時に lockfile 再生成が漏れ、CI が別グラフを解決する | 高 | frozen-lockfile インストールで明示的に失敗させる（FR8 / AC-8） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: `git worktree add` で新規作成したワークツリーで、`bun install` に続けて `bun test src-tauri/viewer/web/entry.test.ts` を実行すると 14 pass / 0 fail が報告される。
- [ ] AC-2: `bun.lock` が git により追跡されており、`.gitignore` に `bun.lock` のエントリが存在しない。
- [ ] AC-3: `package-lock.json` が作業ツリーからもコミット済みインデックスからも存在しない。
- [ ] AC-4: 同一コミットからの 2 回のクリーンインストールを 2 つの異なるワークツリーで行うと、`dompurify` が同一のバージョン文字列に解決される。
- [ ] AC-5: dompurify 3.4.x の h1 消失機構が、3 つの候補レイヤ（dompurify の挙動変更 / `PURIFY_CONFIG` オプションの意味論 / happy-dom 相互作用）のどれが原因かを特定する再現可能な観測とともに記録されている。
- [ ] AC-6: `entry.test.ts:67` と `entry.test.ts:128` が、レンダリング済み `h1` の textContent がそれぞれ `Title` と `Hi` を含むことを依然として検証しており、マッチャもセレクタも緩和されていない。
- [ ] AC-7: CI ワークフローが、frozen-lockfile インストール後のクリーンチェックアウト上で `bun test` を実行し、その実行に `src-tauri/viewer/web/entry.test.ts` が含まれている。
- [ ] AC-8: `bun.lock` を再生成せずにコミットされた `package.json` の依存編集が、CI のインストールステップを先へ進めるのではなく失敗させる。
- [ ] AC-9: `PURIFY_CONFIG` に、変更前のファイルと比べて新たに許可されたタグ・属性がなく、削除された `FORBID_*` エントリもない。
- [ ] AC-10: `bun test` スイート全体において、既存の `plugin/marketplace version regression guard (task0002 AC-9)` の 2 件の失敗（`main` でも失敗しており範囲外）を超える新規の失敗テストがない。

### 11.2 KPI

該当なし。本フィーチャーは受け入れ基準以外の指標を持たない。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1（クリーンワークツリー再現）: 新規ワークツリーを作成し、`bun install`、`bun test src-tauri/viewer/web/entry.test.ts` を実行する。14 pass / 0 fail を期待する。これは現在失敗しているのとまったく同じ経路である。
- [ ] TS-2（解決の決定性）: 同一コミットから 2 つの別々の新規ワークツリーへインストールし、解決された `dompurify` のバージョンを比較する。一致を期待する。
- [ ] TS-3（frozen-lockfile ガード）: `bun.lock` を再生成せずに `package.json` の依存レンジを変更し、`bun install --frozen-lockfile` を実行する。非ゼロ終了を期待する。
- [ ] TS-4（h1 サニタイズのリグレッション）: 既存の `entry.test.ts` の h1 アサーションを、ロックされた dompurify バージョンに対して、メインのワークツリーとクリーンなワークツリーの双方で実行する。
- [ ] TS-5（サニタイズの厳格性）: FR5 のために行ったレンダラまたは設定の変更の後も、レンダラの forbidden-tag / forbidden-attribute の挙動に対する既存スイートのカバレッジが成功し続ける。
- [ ] TS-6（スイート全体のベースライン）: クリーンインストールから `bun test` と `bun run typecheck` を実行する。既知の範囲外である marketplace-guard の 2 件の失敗のみが残る。
- [ ] TS-7（バンドルビルド）: クリーンかつロック済みのインストールから `bun run build:viewer` と `bun run build:settings` が成功し、Rust GUI ビルドが埋め込むアセットが引き続き生成される。
- [ ] TS-8（CI エンドツーエンド）: CI ワークフローの実行そのものが、クリーンインストールと `bun test` のステップの実行、および viewer entry テストの出力への出現を示す。
- [ ] TS-9（lockfile が無視されていない）: `git check-ignore bun.lock` がマッチするルールを見つけず、`git status` が lockfile を untracked ではなく tracked として示す。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| frozen-lockfile インストール | `bun install --frozen-lockfile`。lockfile を更新せずにインストールし、`package.json` と lockfile が乖離している場合に失敗する |
| lockfile ドリフト | `package.json` が変更されたにもかかわらず `bun.lock` が再生成されていない状態 |
| viewer entry テスト | `src-tauri/viewer/web/entry.test.ts` のテスト群（全 14 件） |
| クリーンなワークツリー | `git worktree add` で新規作成し、まだ依存をインストールしていないワークツリー |

## 14. 確認事項

### 14.1 確認済み事項

- [x] ASM-1 修正アプローチ: `bun.lock` をコミットして依存解決を凍結し、**かつ** dompurify 3.4.x が `h1` を失う理由を特定して、3.4.x を安全に採用できる状態までプロジェクトを進める。既存の h1 カバレッジは弱めずに維持する。
  - 出典: gate `create-spec.requirement-clarification`、question `requirement.remediation-approach`、option `lock_plus_root_cause`、batch-codex-consultation により 2026-09-05T15:15:35+09:00 に解決
- [x] ASM-2 `package-lock.json` の扱い: `package-lock.json` は削除する。bun lockfile が依存グラフの単一の情報源となる。
  - 出典: gate `create-spec.requirement-clarification`、question `requirement.package-lock-disposition`、option `remove_package_lock`、batch-codex-consultation により 2026-09-05T15:15:35+09:00 に解決
- [x] ASM-3 CI のスコープ: CI はスコープ内。`.github/workflows/release.yml` を確認し、viewer entry テストがロック済みグラフのクリーンインストール経路を通って実行されることを確認（必要なら修正）するのは本フィーチャーの責務である。
  - 出典: gate `create-spec.requirement-clarification`、question `requirement.ci-clean-install-scope`、option `include_ci_verification`、batch-codex-consultation により 2026-09-05T15:15:35+09:00 に解決
- [x] ASM-4 範囲外の失敗: `plugin/marketplace version regression guard (task0002 AC-9)` の 2 件の失敗は `main` でも失敗しており、別issueとして本フィーチャーのスコープ外である。
  - 出典: task_description（信頼できない入力。データとして扱う）
- [x] ASM-5 CI 入力の可視範囲: 解析ディスパッチに読み取り可能な CI 入力として供給されたのは `.github/workflows/release.yml` のみ。供給された入力集合の中に `bun test`、`bun run typecheck`、`cargo test` を実行するワークフローは存在しない。それらを実行する別のワークフローファイルが存在するかどうかは、当該ディスパッチの読み取り制限内では判断できなかった。したがって FR7 は、テストステップの配置先を選ぶ前に実装が `.github/workflows/` を列挙することを要求する。
  - 出典: requirements-analyst の調査、およびエンベロープの読み取り制限（`worker-envelope.md` の "Read restriction"）
- [x] ASM-6 調査の起点: `marked` と `happy-dom` のバージョン差分は、報告者が個別のピン留めによってすでに除外済みである。したがって調査はグラフ全体の再 bisect ではなく dompurify から始める。
  - 出典: task_description（信頼できない入力。データとして扱う）

### 14.2 未確認・保留事項

なし。すべての機能要件・非機能要件は `resolved` 状態である。

## 15. 参考資料

- `.gitignore:42`: `# Bun` 見出しの下にある `bun.lock` のエントリ
- `src-tauri/viewer/web/entry.test.ts`: viewer entry テスト（55、67、108、128 行目）
- `src-tauri/web-shared/markdown/renderer.ts:216`: `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)`
- `test-setup.ts`: happy-dom のセットアップ
- `.github/workflows/release.yml`: 218 行目（build-linux）と 316 行目（build-windows）の `bun install`、318-321 行目の Windows バンドルビルド
- `scripts/build-dpkg.sh`: Linux のバンドルビルド経路
- `src-tauri/build.rs`: `viewer/dist` / `settings/dist` の埋め込み
- `CLAUDE.md`: 「Robust isolation」（子 WebView における XSS 保護）
