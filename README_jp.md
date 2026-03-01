# eMterm

Tauriで構築されたLinux/Windows向けターミナルエミュレータ。インライン画像やMarkdown表示などのリッチな描画機能を備えています。

## 機能

- **コアターミナル**
  - ANSI/VT100/VT220/xterm 制御シーケンス完全対応
  - 独立したPTYセッションを持つマルチタブターミナル
  - WASMベースのターミナルコアによる高性能グリッドレンダリング
  - リサイズ時の全バッファリフローに対応したリングバッファ（UnifiedBuffer）
  - ダーティ行追跡付きCanvas 2Dレンダラー

- **リッチコンテンツ表示**
  - 独自OSC 777拡張によるインラインMarkdown描画（CommonMark、GFM、シンタックスハイライト、Mermaidダイアグラム）
  - 大容量ドキュメント対応: ファイルサイズ制限なし、チャンク受信毎にセッションタイムアウトリセット
  - アウトラインパネル（目次）・ズーム・キーボード操作対応のフルスクリーンMarkdownビューアー
  - Markdownドキュメント内のMermaidダイアグラム描画（フローチャート、シーケンス図など）
  - インライン画像表示（Kitty Graphics ProtocolおよびSIXEL対応）
  - Kittyプロトコル互換性: kitten icat、ratatui-image、treemdなど外部ツールと連携
  - フルスクリーン画像ビューアー（ピクセルパーフェクト・フィット表示、パン操作、ホイールスクロール）
  - CLIコマンド: `emterm markdown` / `emterm image`（SSH越しでも動作）

- **入力・IME**
  - 高スループットなキー入力（イベントベースのバイナリIPC、JSONシリアライズなし）
  - 完全なIMEサポート: EditContext API（Chromium）および隠しtextareaフォールバック（WebKit）
  - IMEと共存するキャプチャフェーズのクリップボードショートカット（Ctrl+Shift+C/V）
  - ミドルクリックペースト（設定で切替可能）
  - AIインターフェースでのマルチライン入力向けShift+EnterをAlt+Enterとして送信（デフォルトON、設定で切替可能）
  - ダブルクリック後のドラッグで単語単位の選択拡張

- **ナビゲーション**
  - OSC 133 セマンティックプロンプトジャンプ（Ctrl+上/下）
  - マッチハイライト付きインクリメンタルテキスト検索（Ctrl+F）
  - コマンド出力の折りたたみ（展開/折りたたみ）
  - ファイルパスのCtrl+クリックでエディタ起動
  - URLのCtrl+クリックでブラウザ起動

- **設定・外観**
  - アイコン付き折りたたみナビゲーションレール搭載の7カテゴリ設定パネル
  - ダーク・ライト・システムテーマと4種のアクセントカラープリセット（パープル・ブルー・グリーン・オレンジ）
  - ターミナルカラースキーム: 内蔵プリセット＋ユーザーカスタムパレット
  - 3フィールドフォント設定（プライマリ・CJK/セカンダリ・絵文字）とシステムフォントピッカー
  - 設定パネル用UIフォント設定
  - ターミナルプロファイル: シェル・引数・環境変数・作業ディレクトリを持つ名前付きシェル設定
  - カーソル形状、スクロールバー、透明度、行高、スクロールバック行数、シェルなど各種設定
  - 全キーボードショートカット設定可能

- **通知**
  - バックグラウンドタブで新しい出力やプロセスイベントがあると、タブにアクティビティドットを表示
  - ウィンドウが非フォーカス時のOSデスクトップ通知（設定で切替可能）
  - 高頻度出力時の通知スロットリングによるスパム防止

- **国際化**
  - 日本語・英語UI（OSロケールから自動検出）
  - Unicode 17.0 / Emoji 17.0 の文字幅対応

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
# プロダクションビルド
bun run tauri:build
```

ビルドされたアプリケーションは `src-tauri/target/release/` に出力されます。

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

