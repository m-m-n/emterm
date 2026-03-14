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

内部で `bun tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc` を実行します。

**前提条件:**

```bash
# cargo-xwinのインストール
cargo install cargo-xwin

# システム依存パッケージのインストール (Ubuntu/Debian)
sudo apt install clang lld llvm nsis librsvg2-bin
```

- `clang`, `lld` — C/C++クロスコンパイラおよびリンカー（`clang-cl`, `lld-link`）
- `llvm` — リソースコンパイラ（`llvm-rc`）
- `nsis` — NSISインストーラ生成（`makensis`）
- `librsvg2-bin` — SVGからPNGへのアイコン変換（`rsvg-convert`）

## CLIコマンド

eMtermは制御シーケンスを出力するためのCLIコマンドを提供します：

```bash
# ターミナルでMarkdownを表示
emterm markdown <file.md>

# ターミナルで画像を表示
emterm image <image.png>
```

## tmux利用時の注意事項

### tmux内でのCLIコマンド

tmux内では、CLIコマンド（`emterm markdown`、`emterm image`）が制御シーケンスを自動的にDCSパススルーでラップします。tmux設定で`allow-passthrough`を有効にしてください：

```bash
set -g allow-passthrough on
```

### tmux内でのSFTPアップロード先ディレクトリ

SSHタブへのドラッグ&ドロップによるファイルアップロード時、eMtermは[OSC 7](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands)（作業ディレクトリ通知）を使用してリモートのアップロード先を決定します。しかし、tmuxは内部ペインのOSC 7をインターセプトし、外側のターミナルに転送しません。そのため、アップロード先が常にホームディレクトリになります。

tmux内で正しい作業ディレクトリを取得するには、リモートサーバーのシェル設定に以下を追加してください：

**bash** (`~/.bashrc`):
```bash
if [ -n "$TMUX" ]; then
  _osc7_dcs() {
    printf '\ePtmux;\e\e]7;%s\e\e\\\e\\' "$PWD"
  }
  PROMPT_COMMAND="_osc7_dcs${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi
```

**zsh** (`~/.zshrc`):
```zsh
if [[ -n "$TMUX" ]]; then
  _osc7_dcs() {
    printf '\ePtmux;\e\e]7;%s\e\e\\\e\\' "$PWD"
  }
  precmd_functions+=(_osc7_dcs)
fi
```

OSC 7をDCSパススルーでラップすることで、tmuxがeMtermに転送するようになります。

### tmux内でのOSCシーケンス

tmux 3.4以降はOSC 8（ハイパーリンク）、OSC 52（クリップボード）などの標準OSCシーケンスをネイティブサポートしています。これらの機能を有効にするには、`hyperlinks`ターミナルフィーチャーを追加してください：

```bash
set -ga terminal-features ",xterm-256color:hyperlinks"
```

eMterm独自の拡張（OSC 777のMarkdown/ダウンロード、OSC 1337のiTerm2画像）については、上記の`allow-passthrough`設定を使用してください。`emterm markdown`と`emterm image`コマンドはDCSラップを自動的に行います。

## OSCシーケンス対応状況

eMtermがサポートするOSC（Operating System Command）シーケンス一覧：

| OSC | 名称 | 説明 |
|-----|------|------|
| 0 | SetTitleAndIcon | ウィンドウタイトルとアイコン名を設定 |
| 1 | SetIconName | アイコン名を設定 |
| 2 | SetTitle | ウィンドウタイトルを設定 |
| 4 | SetColorPalette | カラーパレットの照会/設定 |
| 7 | SetWorkingDirectory | 作業ディレクトリを設定（SFTPアップロード先の決定に使用） |
| 8 | Hyperlink | クリック可能なハイパーリンク（`Ctrl+クリック`で開く） |
| 9 | Notification / Progress | デスクトップ通知とプログレスインジケーター（ConEmu互換） |
| 10 | SetForegroundColor | デフォルト前景色の照会/設定 |
| 11 | SetBackgroundColor | デフォルト背景色の照会/設定 |
| 12 | SetCursorColor | カーソル色の照会/設定 |
| 22 | CursorShape | カーソル形状スタックのpush/pop |
| 52 | Clipboard | システムクリップボードの読み書き（設定で切替可能） |
| 104 | ResetColorPalette | カラーパレットをデフォルトにリセット |
| 110 | ResetForegroundColor | デフォルト前景色をリセット |
| 111 | ResetBackgroundColor | デフォルト背景色をリセット |
| 112 | ResetCursorColor | カーソル色をリセット |
| 133 | SemanticPrompt | プロンプト/コマンド/出力のゾーンマーカー（Ctrl+上/下ジャンプ、出力折りたたみに使用） |
| 777 | eMterm拡張 | インラインMarkdown描画、ファイルダウンロード、出力折りたたみ |
| 1337 | iTerm2プロトコル | インライン画像表示、ユーザー変数 |

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

