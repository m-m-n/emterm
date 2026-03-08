# eMterm

Tauriで構築されたLinux/Windows向けターミナルエミュレータ。インライン画像やMarkdown表示などのリッチな描画機能を備えています。

## 機能

- **コアターミナル**
  - ANSI/VT100/VT220/xterm 制御シーケンス完全対応
  - 独立したPTYセッションを持つマルチタブターミナル
  - WASMベースのターミナルコアによる高性能グリッドレンダリング
  - リサイズ時の全バッファリフローに対応したリングバッファ（UnifiedBuffer）
  - ダーティ行追跡付きCanvas 2Dレンダラー
  - BCE（背景色消去）対応

- **リッチコンテンツ表示**
  - 独自OSC 777拡張によるインラインMarkdown描画（CommonMark、GFM、シンタックスハイライト、Mermaidダイアグラム）
  - 大容量ドキュメント対応: ファイルサイズ制限なし、チャンク受信毎にセッションタイムアウトリセット
  - アウトラインパネル（目次）・ズーム・Space/Shift+Spaceスクロール対応のフルスクリーンMarkdownビューアー
  - Markdownドキュメント内のMermaidダイアグラム描画（フローチャート、シーケンス図など）とチャート/コードトグルツールバー
  - インライン画像表示（Kitty Graphics ProtocolおよびSIXEL対応）
  - Kittyプロトコル互換性: kitten icat、ratatui-image、treemdなど外部ツールと連携
  - フルスクリーン画像ビューアー（ピクセルパーフェクト・フィット表示、パン操作、ホイールスクロール、Space/Shift+Spaceスクロール）
  - ビューアーはターミナルコンテンツ領域内に描画（ビューアー表示中もタブバーにアクセス可能）
  - CLIコマンド: `emterm markdown` / `emterm image`（SSH越しでも動作、CLIのみビルド対応）

- **入力・IME**
  - 高スループットなキー入力（イベントベースのバイナリIPC、JSONシリアライズなし）
  - 完全なIMEサポート: EditContext API（Chromium）および隠しtextareaフォールバック（WebKit）
  - TUIアプリ使用時のIME位置自動調整（カーソル非表示時は左下に配置）
  - IMEと共存するキャプチャフェーズのクリップボードショートカット（Ctrl+Shift+C/V）
  - ミドルクリックペースト（設定で切替可能）
  - AIインターフェースでのマルチライン入力向けShift+EnterをAlt+Enterとして送信（デフォルトON、設定で切替可能）
  - ダブルクリック後のドラッグで単語単位の選択拡張
  - 包括的な特殊キーマッピング（Ctrl+記号、修飾キー付き矢印キー・ファンクションキー、Shift+Tab）

- **ナビゲーション**
  - OSC 133 セマンティックプロンプトジャンプ（Ctrl+上/下）
  - マッチハイライト付きインクリメンタルテキスト検索（Ctrl+F）
  - コマンド出力の折りたたみ（展開/折りたたみ）
  - ファイルパスのCtrl+クリックでエディタ起動（ホバー時のみ下線表示）
  - URLのCtrl+クリックでブラウザ起動（ホバー時のみ下線表示）
  - ターミナル領域・タブ・タブバーへの右クリックコンテキストメニュー

- **設定・外観**
  - アイコン付き折りたたみナビゲーションレール搭載の7カテゴリ設定パネル
  - ダーク・ライト・システムテーマと5種のアクセントカラープリセット（パープル・ブルー・グリーン・オレンジ・ピンク）
  - ターミナルカラースキーム: 内蔵プリセット＋ユーザーカスタムパレット（横並びレイアウト）
  - ANSIボールド時の輝度アップ（ボールド+標準色(0-7)が明るいバリアント(8-15)に自動切替、設定可能）
  - 3フィールドフォント設定（プライマリ・CJK/セカンダリ・絵文字）とシステムフォントピッカー（クリアボタン付き）
  - 設定パネル用UIフォント設定
  - ターミナルプロファイル: シェル・引数・環境変数・作業ディレクトリを持つ名前付きシェル設定
  - SSH接続管理: sshコマンド自動検出、~/.ssh/configからのインポート、接続エントリのCRUD
  - SFTPファイルアップロード: SSHタブへのドラッグ&ドロップでファイル転送（非SSHタブはファイルパスをペースト）
  - カーソル形状、スクロールバー、透明度、行高、スクロールバック行数、シェルなど各種設定
  - 全キーボードショートカット設定可能

- **通知**
  - バックグラウンドタブで新しい出力やプロセスイベントがあると、タブにアクティビティドットを表示
  - ウィンドウが非フォーカス時のOSデスクトップ通知（設定で切替可能）
  - 高頻度出力時の通知スロットリングによるスパム防止

- **国際化**
  - 日本語・英語UI（OSロケールから自動検出）
  - Unicode 17.0 / Emoji 17.0 の文字幅対応
  - TUIアプリ互換のEAW=A（曖昧幅）文字レンダリング
  - Extended_Pictographic文字のテキスト表示形式の強制適用

## 必要条件

- [Rust](https://rustup.rs/) 1.85以上
- [Bun](https://bun.sh/) 1.0以上
- Tauriのシステム依存関係（[Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)を参照）

### Linux (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### Windows

[Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)と[WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)をインストールしてください。

## インストール

```bash
# リポジトリをクローン
git clone https://github.com/yourusername/emterm.git
cd emterm

# 依存関係をインストール
bun install
```

## 開発

```bash
# 開発サーバーとTauriアプリを起動
bun run tauri:dev
```

## ビルド

```bash
# プロダクションビルド（Linux: deb/rpm, Windows: nsis）
make build
```

ビルドされたアプリケーションは `src-tauri/target/release/bundle/` に出力されます。

### CLI単体ビルド

GUIアプリケーションなしで、CLIコマンド（`emterm markdown`、`emterm image`）のみをビルドします：

```bash
cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features
```

`gui` featureフラグがデフォルトで有効になっています。`--no-default-features` を指定するとGUI依存（Tauri、WebViewなど）を除外し、軽量なCLIバイナリを生成します。

### Windowsクロスコンパイル（Linuxから）

[cargo-xwin](https://github.com/rust-cross/cargo-xwin) を使用してLinuxからWindows向けにクロスコンパイルします：

```bash
make win-build
```

内部で `bun tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc` を実行します。事前に `cargo-xwin` のインストールが必要です（`cargo install cargo-xwin`）。

## CLIコマンド

eMtermは制御シーケンスを出力するためのCLIコマンドを提供します：

```bash
# ターミナルでMarkdownを表示
emterm markdown <file.md>

# ターミナルで画像を表示
emterm image <image.png>
```

## プロジェクト構成

```
emterm/
├── src/                    # フロントエンド (TypeScript)
│   ├── index.html
│   ├── main.ts
│   └── styles.css
├── src-tauri/              # バックエンド (Rust)
│   ├── src/
│   │   ├── main.rs
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── serve.ts                # 開発サーバー
├── package.json
└── tsconfig.json
```

## スクリプト

| コマンド | 説明 |
|---------|------|
| `bun run dev` | フロントエンド開発サーバーを起動 |
| `bun run tauri:dev` | Tauriアプリを開発モードで起動 |
| `bun run tauri:build` | プロダクションビルド |
| `bun run typecheck` | TypeScript型チェック |
| `bun test` | テスト実行 |

