# 実装計画書: 基本PTY接続機能

## 1. 概要

### 1.1 目的

本計画書は、eMtermターミナルエミュレータにおける基本PTY接続機能の実装計画を定義する。

### 1.2 参照仕様書

- 要件定義書: `doc/tasks/pty-connection/要件定義書.md`
- 技術仕様書: `doc/tasks/pty-connection/SPEC.md`

### 1.3 スコープ

仕様書に基づき、以下の機能を実装する：

- PTYプロセスの生成・管理（FR-PTY-001〜003）
- デフォルトシェル検出（FR-SHELL-001〜003）
- プロセスライフサイクル管理（FR-LIFE-001〜004）
- 入出力処理（FR-IN-001〜003, FR-OUT-001〜003）
- IPC通信（FR-IPC-001〜005）
- ウィンドウリサイズ（FR-RESIZE-001〜003）

---

## 2. 実装フェーズ

### フェーズ1: Rust基盤モジュール

- **ゴール**: PTYモジュールの基本構造と依存関係を確立する
- **成果物**:
  - `src-tauri/Cargo.toml`: 依存クレート追加
  - `src-tauri/src/pty/mod.rs`: モジュールエクスポート、型定義
  - `src-tauri/src/pty/shell.rs`: シェル検出ユーティリティ
- **対応要件**: FR-SHELL-001〜003
- **完了条件**:
  - `cargo check`が成功すること
  - `detect_default_shell()`が各プラットフォームで適切なシェルパスを返すこと
  - ユニットテストでシェル検出ロジックが検証されること
- **推定工数**: 小

### フェーズ2: PTYセッション実装

- **ゴール**: 単一PTYセッションの生成・操作機能を実装する
- **成果物**:
  - `src-tauri/src/pty/session.rs`: PtySession構造体と操作メソッド
- **対応要件**: FR-PTY-001〜002, FR-LIFE-001〜003, FR-IN-003
- **完了条件**:
  - `PtySession::new()`でPTYペアとシェルプロセスが起動すること
  - `write()`, `resize()`, `take_reader()`, `try_wait()`, `kill()`が動作すること
  - ユニットテストでセッション生成・終了が検証されること
- **推定工数**: 中

### フェーズ3: PTYマネージャー実装

- **ゴール**: 複数PTYセッションを管理する機構を実装する
- **成果物**:
  - `src-tauri/src/pty/manager.rs`: PtyManager構造体
- **対応要件**: FR-PTY-003, FR-LIFE-004
- **完了条件**:
  - セッションの作成・取得・削除が非同期で動作すること
  - 複数セッションを同時管理できること
  - セッションIDによる識別が正しく機能すること
- **推定工数**: 小

### フェーズ4: Tauriコマンド実装

- **ゴール**: フロントエンドから呼び出し可能なTauriコマンドを実装する
- **成果物**:
  - `src-tauri/src/lib.rs`: コマンド登録、イベント発行
- **対応要件**: FR-IPC-001〜005, FR-OUT-001〜003
- **完了条件**:
  - `pty_spawn`, `pty_write`, `pty_resize`, `pty_kill`コマンドが登録されること
  - `pty_output`, `pty_exit`, `pty_error`イベントが発行されること
  - 出力リーダースレッドが正常に動作すること
- **推定工数**: 中

### フェーズ5: TypeScript型定義

- **ゴール**: フロントエンド用の型定義を整備する
- **成果物**:
  - `src/types/pty.ts`: IPC通信用の型定義
- **対応要件**: FR-IPC-004〜005
- **完了条件**:
  - Rust側のペイロード構造と一致する型が定義されていること
  - TypeScriptの型チェックが通ること
- **推定工数**: 小

### フェーズ6: PTYクライアント実装

- **ゴール**: バックエンドと通信するPTYクライアントクラスを実装する
- **成果物**:
  - `src/pty/client.ts`: PtyClientクラス
- **対応要件**: FR-IN-001, FR-OUT-001, FR-IPC-001〜003
- **完了条件**:
  - `spawn()`, `write()`, `resize()`, `kill()`メソッドが動作すること
  - `onOutput()`, `onExit()`, `onError()`でイベント購読できること
  - `dispose()`でリスナーが解除されること
- **推定工数**: 中

### フェーズ7: キーボード入力ハンドラー

- **ゴール**: キーボードイベントをPTYバイトシーケンスに変換する
- **成果物**:
  - `src/pty/keyboard.ts`: キー変換ユーティリティ
- **対応要件**: FR-IN-002
- **完了条件**:
  - 通常文字が正しくエンコードされること
  - 制御文字（Ctrl+C, Ctrl+D等）が正しいバイト列に変換されること
  - 矢印キー、ファンクションキー等がエスケープシーケンスに変換されること
  - ユニットテストで変換ロジックが検証されること
- **推定工数**: 中

### フェーズ8: ウィンドウサイズ計算

- **ゴール**: ウィンドウサイズから行数・列数を計算する機能を実装する
- **成果物**:
  - `src/pty/size.ts`: サイズ計算ユーティリティ
- **対応要件**: FR-RESIZE-001〜002
- **完了条件**:
  - `calculateTerminalSize()`が正しい行数・列数を返すこと
  - `measureCharacterSize()`が文字サイズを計測できること
  - パディングを考慮した計算が行われること
- **推定工数**: 小

### フェーズ9: 統合とTauri設定

- **ゴール**: 全コンポーネントを統合し、Tauri設定を更新する
- **成果物**:
  - `src-tauri/capabilities/default.json`: パーミッション追加
  - 統合テスト
- **対応要件**: NFR-SEC-002
- **完了条件**:
  - 必要最小限のパーミッションが設定されていること
  - `bun tauri dev`でアプリケーションが起動すること
  - シェルプロンプトが表示されること
- **推定工数**: 小

### フェーズ10: E2Eテストと検証

- **ゴール**: 全受け入れ基準を満たすことを検証する
- **成果物**:
  - 統合テストスイート
  - 手動テスト結果ドキュメント
- **対応要件**: 全受け入れ基準
- **完了条件**:
  - 要件定義書セクション7の全チェック項目が合格すること
  - Linux, macOS, Windowsでの動作確認が完了すること
- **推定工数**: 中

---

## 3. コンポーネント設計

### 3.1 Rustバックエンド

#### 3.1.1 モジュール構成

```
src-tauri/src/
├── lib.rs              # Tauriアプリエントリ、コマンド登録
├── main.rs             # バイナリエントリポイント
└── pty/
    ├── mod.rs          # モジュールエクスポート、共通型
    ├── manager.rs      # セッション管理
    ├── session.rs      # 個別セッション
    └── shell.rs        # シェル検出
```

#### 3.1.2 コンポーネント責務

| コンポーネント | 責務 |
|---------------|------|
| `pty/mod.rs` | `SessionId`型定義、`PtyError`列挙型、`generate_session_id()`関数、サブモジュールの再エクスポート |
| `pty/session.rs` | `PtySession`構造体：PTYペア保持、シェルプロセス管理、read/write/resize操作 |
| `pty/manager.rs` | `PtyManager`構造体：セッションの登録・取得・削除、`RwLock<HashMap>`によるスレッドセーフな管理 |
| `pty/shell.rs` | `detect_default_shell()`関数：プラットフォーム別デフォルトシェル検出 |
| `lib.rs` | Tauriコマンド定義（`pty_spawn`, `pty_write`, `pty_resize`, `pty_kill`）、イベント発行、リーダースレッド生成 |

#### 3.1.3 インターフェース定義

**PtySession**

```
new(id, shell, cols, rows) -> Result<Self, PtyError>
resize(cols, rows) -> Result<(), PtyError>
write(data) -> Result<(), PtyError>  [async]
take_reader() -> Result<Box<dyn Read + Send>, PtyError>
try_wait() -> Result<Option<ExitStatus>, PtyError>
kill() -> Result<(), PtyError>
```

**PtyManager**

```
new() -> Self
create_session(shell, cols, rows) -> Result<SessionId, PtyError>  [async]
get_session(id) -> Option<Arc<Mutex<PtySession>>>  [async]
remove_session(id) -> Option<Arc<Mutex<PtySession>>>  [async]
```

**Tauriコマンド（SPEC.md 4.1準拠）**

| コマンド | 引数 | 戻り値 |
|---------|------|--------|
| `pty_spawn` | `shell?: string`, `cols?: u16`, `rows?: u16` | `{ session_id: string }` |
| `pty_write` | `session_id: string`, `data: Vec<u8>` | `()` |
| `pty_resize` | `session_id: string`, `cols: u16`, `rows: u16` | `()` |
| `pty_kill` | `session_id: string` | `()` |

**Tauriイベント（SPEC.md 4.2準拠）**

| イベント | ペイロード |
|---------|-----------|
| `pty_output` | `{ session_id: string, data: Vec<u8> }` |
| `pty_exit` | `{ session_id: string, code: i32 }` |
| `pty_error` | `{ session_id: string, message: string }` |

### 3.2 TypeScriptフロントエンド

#### 3.2.1 モジュール構成

```
src/
├── main.ts             # アプリケーションエントリ
├── types/
│   └── pty.ts          # PTY関連型定義
└── pty/
    ├── client.ts       # PTYクライアントクラス
    ├── keyboard.ts     # キー入力変換
    └── size.ts         # ターミナルサイズ計算
```

#### 3.2.2 コンポーネント責務

| コンポーネント | 責務 |
|---------------|------|
| `types/pty.ts` | IPC通信用の型定義（`SpawnResult`, `PtyOutputPayload`, `PtyExitPayload`, `PtyErrorPayload`, `PtySpawnOptions`） |
| `pty/client.ts` | `PtyClient`クラス：バックエンドとのIPC通信抽象化、イベントリスナー管理 |
| `pty/keyboard.ts` | `keyEventToBytes()`関数：KeyboardEventからバイト配列への変換、特殊キーマッピング |
| `pty/size.ts` | `calculateTerminalSize()`、`measureCharacterSize()`関数：ターミナルサイズ計算 |

#### 3.2.3 インターフェース定義

**PtyClient**

```
spawn(options?) -> Promise<string>
write(data: Uint8Array | string) -> Promise<void>
resize(cols, rows) -> Promise<void>
kill() -> Promise<void>
onOutput(callback) -> Promise<void>
onExit(callback) -> Promise<void>
onError(callback) -> Promise<void>
dispose() -> void
```

**keyboard.ts**

```
keyEventToBytes(event: KeyboardEvent) -> Uint8Array | null
```

**size.ts**

```
calculateTerminalSize(container, charWidth, charHeight) -> { cols, rows }
measureCharacterSize(fontFamily, fontSize) -> { width, height }
```

---

## 4. 依存関係

### 4.1 フェーズ間依存関係

```
フェーズ1 ──┬──► フェーズ2 ──► フェーズ3 ──► フェーズ4 ──┐
           │                                            │
フェーズ5 ─┴──► フェーズ6 ──► フェーズ7                  ├──► フェーズ9 ──► フェーズ10
                              │                         │
フェーズ8 ────────────────────┴─────────────────────────┘
```

### 4.2 並列実行可能なフェーズ

- フェーズ1 と フェーズ5, フェーズ8 は並列実行可能
- フェーズ7 と フェーズ3, フェーズ4 は並列実行可能

### 4.3 外部依存関係（仕様書準拠）

**Rust クレート（SPEC.md 2.1）**

| クレート | バージョン | 用途 |
|----------|-----------|------|
| portable-pty | 0.8 | クロスプラットフォームPTY抽象化 |
| tokio | 1 (sync, rt-multi-thread, macros) | 非同期ランタイム |
| uuid | 1 (v4) | セッションID生成 |
| thiserror | - | エラー型定義 |

**NPM パッケージ**

| パッケージ | 用途 |
|-----------|------|
| @tauri-apps/api | Tauri IPC API |

---

## 5. テスト計画

### 5.1 ユニットテスト（Rust）

| テスト対象 | テスト項目 | 対応要件 |
|-----------|-----------|----------|
| `shell.rs` | `detect_default_shell()`が空でない文字列を返す | FR-SHELL-001〜003 |
| `shell.rs` | Linux/macOSで`SHELL`環境変数が反映される | FR-SHELL-001 |
| `shell.rs` | Windowsで`powershell.exe`が返される | FR-SHELL-002 |
| `mod.rs` | `generate_session_id()`がユニークなIDを生成する | FR-IPC-005 |
| `session.rs` | PTYセッションの生成・終了が成功する | FR-PTY-001〜002 |
| `manager.rs` | セッションの追加・取得・削除が動作する | FR-PTY-003 |

### 5.2 ユニットテスト（TypeScript）

| テスト対象 | テスト項目 | 対応要件 |
|-----------|-----------|----------|
| `keyboard.ts` | 通常文字がUTF-8エンコードされる | FR-IN-002 |
| `keyboard.ts` | Ctrl+CがETX(0x03)に変換される | FR-IN-002 |
| `keyboard.ts` | 矢印キーがエスケープシーケンスに変換される | FR-IN-002 |
| `keyboard.ts` | Alt+キーがESCプレフィックス付きで出力される | FR-IN-002 |
| `size.ts` | 正しい行数・列数が計算される | FR-RESIZE-002 |

### 5.3 統合テスト

| テスト項目 | 検証内容 | 対応要件 |
|-----------|---------|----------|
| シェル起動 | `pty_spawn`でシェルが起動し、プロンプトが出力される | FR-PTY-001〜002 |
| 入力送信 | `pty_write`で送信したデータがシェルに届く | FR-IN-001 |
| 出力受信 | シェルからの出力が`pty_output`イベントで届く | FR-OUT-001〜002 |
| リサイズ | `pty_resize`後に`stty size`の出力が変わる | FR-RESIZE-001〜003 |
| 終了検出 | `exit`コマンドで`pty_exit`イベントが発行される | FR-LIFE-001 |
| エラー検出 | 不正なセッションIDでエラーが返される | NFR-REL-002 |

### 5.4 手動テストチェックリスト（受け入れ基準）

**基本動作（要件定義書7.1）**

- [ ] Linux, macOS, Windowsでシェルが起動すること
- [ ] キーボード入力がシェルに送信されること
- [ ] シェルの出力が画面に表示されること
- [ ] `exit`コマンドでセッションが正常終了すること

**リサイズ（要件定義書7.2）**

- [ ] ウィンドウリサイズ時に`stty size`の出力が更新されること（Linux/macOS）
- [ ] ウィンドウリサイズ時に`$Host.UI.RawUI.WindowSize`の出力が更新されること（Windows/PowerShell）

**エラーハンドリング（要件定義書7.3）**

- [ ] 存在しないシェルパス指定時にエラーが表示されること
- [ ] Ctrl+Cでプロセス中断できること

**追加動作確認（SPEC.md 8.3）**

- [ ] シェルプロンプトが表示される
- [ ] タイピングで文字が表示される
- [ ] 矢印キーで履歴をナビゲートできる
- [ ] Tabキーで補完が動作する

### 5.5 パフォーマンステスト（NFR検証）

| テスト項目 | 検証内容 | 合格基準 | 対応要件 |
|-----------|---------|----------|----------|
| 入力遅延測定 | キー入力からエコー表示までの時間 | 50ms以下 | NFR-PERF-001 |
| 大量出力テスト | `yes` コマンドを5秒間実行中のUI応答性 | UIがフリーズしない | NFR-PERF-002 |
| メモリ使用量測定 | 1セッション稼働時のプロセスメモリ | 50MB以下 | NFR-PERF-003 |
| 長時間稼働テスト | 1時間連続稼働後のメモリリーク | 起動時比+10%以内 | NFR-PERF-003 |

**測定方法:**
- 入力遅延: DevToolsのPerformanceタブまたはタイムスタンプログ
- メモリ: タスクマネージャー/Activity Monitor/htop

---

## 6. リスクと対策

### 6.1 技術的リスク

| リスク | 影響度 | 発生確率 | 対策 |
|--------|--------|----------|------|
| portable-ptyのWindows ConPTY対応が不完全 | 高 | 低 | Windows固有のワークアラウンド層を用意。issue監視 |
| Tauri 2.xのイベントAPIの制限 | 中 | 低 | 代替としてチャネル方式を検討 |
| 大量出力時のパフォーマンス低下 | 中 | 中 | バッファリング・スロットリング機構を後続フェーズで追加可能な設計にする |
| tokio/futuresの競合 | 中 | 低 | 専用リーダースレッドで同期的に処理し、非同期部分を最小化 |

### 6.2 スケジュールリスク

| リスク | 対策 |
|--------|------|
| Windows環境でのテスト遅延 | 早期にWindows環境を準備、CI/CDパイプラインにWindows追加 |
| 想定外のプラットフォーム差異 | フェーズ2完了後に各プラットフォームで動作確認を実施 |

---

## 7. 検証チェックリスト

### 7.1 仕様書整合性チェック

- [x] 全FR要件に対応するフェーズが存在する
- [x] 全NFR要件への考慮がある（NFR-REL-003は後続タスクとしてスコープ外）
- [x] インターフェース定義がSPEC.mdと一致する
- [x] 依存クレート/パッケージがSPEC.mdと一致する
- [x] モジュール構成がSPEC.md 2.2と一致する
- [x] パフォーマンス要件（NFR-PERF-*）の検証項目がテスト計画に含まれている

### 7.2 設計原則チェック

- [x] SSOT: 仕様書に記載のない機能を計画に含めていない
- [x] No Code: 計画書はWHATとコントラクトのみ記述
- [x] YAGNI: 将来機能への過度な先回り設計を避けている
- [x] 検証可能性: 各フェーズに明確な完了条件がある

### 7.3 制約事項準拠チェック

- [x] Tauri 2.x APIのみ使用
- [x] Rust edition 2024、rust-version 1.85以上
- [x] フロントエンドはVanilla TypeScript
- [x] 複数セッション対応設計（FR-PTY-003）
- [x] ANSIパース・画像/Markdown表示はスコープ外
- [x] 切断時の再接続/新規セッションUIはスコープ外（NFR-REL-003）
- [x] WindowsはPowerShellのみサポート

