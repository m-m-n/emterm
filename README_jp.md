# eMterm

Linux/Windows向けのネイティブターミナルエミュレータ。ターミナル本体は winit + wgpu + swash でネイティブ描画し、Markdown / JSON / YAML / 画像ビューア・設定パネルは wry（WebKitGTK / WebView2）の子ウィンドウで表示します。

## 機能

- **コアターミナル**
  - ANSI/VT100/VT220/xterm 制御シーケンス完全対応
  - 独立したPTYセッションを持つマルチタブターミナル
  - `term_core` Rustクレートによるパーサ・グリッド・Unicode 幅
  - リサイズ時の全バッファリフローに対応したリングバッファ（UnifiedBuffer）
  - SlimCellスクロールバック圧縮: StyleTable/CharTable重複排除によるセルあたり76%のメモリ削減（34B→8B）
  - winit イベントループ駆動の wgpu レンダーパイプライン（ダーティ行追跡付き）
  - BCE（背景色消去）対応
  - COLRv1ベクター絵文字レンダリング（skrifa + tiny-skia）: 端数DPIでのカラー絵文字のシャープ化、従来のCBDTビットマップフォントより約5MB小さいバンドルフォント

- **リッチコンテンツ表示**
  - 独自OSC 777拡張によるインラインMarkdown描画（CommonMark、GFM、シンタックスハイライト、Mermaidダイアグラム）
  - 大容量ドキュメント対応: ファイルサイズ制限なし、チャンク受信毎にセッションタイムアウトリセット
  - アウトラインパネル（目次）・ズーム・Space/Shift+Spaceスクロール対応のフルスクリーンMarkdownビューアー
  - Markdownビューアーリンクナビゲーション: `.md`リンクをクリックして関連ファイルを閲覧、インライン画像の遅延読み込み（SSH越しでも動作）
  - Markdownドキュメント内のMermaidダイアグラム描画（フローチャート、シーケンス図など）とチャート/コード/拡大表示（Spreadボタンでズーム・パン可能なフルスクリーンポップアップ）/コピートグルツールバー
  - インライン画像表示（Kitty Graphics ProtocolおよびSIXEL対応）
  - Kittyプロトコル互換性: kitten icat、ratatui-image、treemdなど外部ツールと連携
  - フルスクリーン画像ビューアー（ピクセルパーフェクト・フィット表示、パン操作、ホイールスクロール、Space/Shift+Spaceスクロール）
  - ビューアーはターミナルコンテンツ領域内に描画（ビューアー表示中もタブバーにアクセス可能）
  - CLIコマンド: `emterm markdown` / `emterm image`（SSH越しでも動作、CLIのみビルド対応）
  - OSC 777ファイルダウンロード: ストリーミングI/Oによるファイルサイズ無制限のダウンロード、転送開始時に保存ダイアログを表示

- **ターミナルマルチプレクサ**
  - `emterm mux` でネイティブマルチプレクサデーモンを起動（GUIが生のPTYバイトを受信、二重パースなし）
  - デタッチ（`prefix+d`）／リアタッチ（`emterm mux attach`）と画面状態の完全な復元
  - セッションあたり複数ウィンドウとタブグループUI（全ウィンドウ同時ストリーミングによる瞬時切り替え）
  - ウィンドウ切り替え時にペインごとのスクロール位置とスクロールバック履歴を保持（デタッチ→リアタッチ不要）
  - ウィンドウ管理: `prefix+c`（新規）、`prefix+n`/`prefix+p`（切替）、`prefix+,`（リネーム）、`prefix+m`（移動/並び替え、`[N]`位置バッジ表示）
  - tmux.conf インポート: プレフィックスキー、キーバインド、マウス、status-position
  - インバンドAPCプロトコル: PTYストリーム経由でmux制御メッセージを送受信（SSH透過、追加ソケット転送不要）
  - `emterm mux new-window [-n 名前] [-c コマンド]`: CLIからウィンドウを作成し初期コマンドを実行
  - `emterm mux send-keys [-t ウィンドウ]`: 標準入力のデータをmuxウィンドウにキー入力として送信
  - `emterm mux script`: デーモン起動のみ（アタッチなし）、スクリプトによるワークスペース初期化に使用
  - `emterm mux kill`: IPC経由でデーモンと全PTYセッションをgracefulに終了
  - OSCタイトル伝播: デーモンがシェルのOSC 0/2シーケンスからウィンドウ名を更新（GUI未接続中も有効）
  - デタッチ中のシェル終了を正しくreap; 最後のセッションが空になるとデーモンが自動終了
  - Muxステータスバー: デーモン側でコマンドを定期実行し、テンプレート変数（`{cmd:name}`、`{hostname}`、`{cwd}`）でステータスバーに反映
  - メインバッファスナップショット修正: メインバッファペインではデーモンvt100スクリーンダンプを省略し、スクロールバックバイトを直接リプレイ（`apt install`などの実行後にスナップショット復元でプログレスバー表示が崩れる問題を解消）
  - Windows対応: Named Pipe IPCとプロセスデタッチ（ターミナル終了後もデーモン継続）

- **ステータスバー**
  - ウィンドウ下部に表示する設定可能なステータスバー（デフォルトOFF、設定から有効化）
  - テンプレート変数: `{time}`、`{cwd}`、`{git_branch}`、`{cmd:name}`（変数ごとのリフレッシュレート設定可能）
  - gitブランチカラー: クリーン（緑）、ダーティ（黄色）
  - OSC 777 statusbarプロトコルによる外部コンテンツ注入（set/clear/show/hide）
  - カスタムコマンド: ユーザー定義の実行ファイルを設定したインターバルで実行

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
  - WSLディストリビューション検出・インポート・プロファイル統合（Windowsのみ）
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
- [Bun](https://bun.sh/) 1.0以上（子WebView用 TypeScript バンドルのビルドに使用）

### Linux (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libglib2.0-dev \
                 libasound2-dev librsvg2-dev librsvg2-bin build-essential dpkg
```

子WebView（Markdownビューア・設定パネル・データビューア）が wry 経由で `libwebkit2gtk-4.1-dev` / `libgtk-3-dev` を必要とします。`libasound2-dev` はベル音用、`librsvg2-bin` の `rsvg-convert` はアイコン生成に使用します。

### Windows

[Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) をインストールしてください。WebView2 ランタイムは Windows 10/11 標準で組み込まれており別途インストール不要です。

## インストール

```bash
# リポジトリをクローン
git clone https://github.com/m-m-n/emterm.git
cd emterm

# JS 依存と Rust ツールチェイン（クロスビルド用）をインストール
bun install
make setup   # rustup target add x86_64-pc-windows-msvc + cargo install cargo-xwin
```

## 開発

```bash
make dev   # bun run build:viewer + build:settings → cargo run
```

## ビルド

```bash
make build   # Linux GUI リリース（src-tauri/target-host/release/emterm）
make dpkg    # build/emterm_<ver>_<arch>.deb（GUI、libwebkit2gtk-4.1-0 に依存）
```

### CLI単体ビルド

GUI 不要の軽量バイナリです。SSH 越しに `emterm markdown` / `emterm image` / `emterm json` / `emterm yaml` を実行すると、ローカル側 eMterm が制御シーケンスを受け取ってリッチ表示します。

```bash
make cli-build   # cargo build --release --no-default-features
make cli-dpkg    # build/emterm-cli_<ver>_<arch>.deb（libc6 にのみ依存）
```

`gui` feature フラグがデフォルトで有効です。`--no-default-features` を指定すると winit / wgpu / wry / swash などのGUI依存をすべて除外し、CLI サブコマンドのみのバイナリを生成します。

### Windowsクロスコンパイル（Linuxから）

[cargo-xwin](https://github.com/rust-cross/cargo-xwin) を使用してLinuxからWindows向けにクロスコンパイルします：

```bash
make win-build
```

`cargo xwin build --release --target x86_64-pc-windows-msvc` を実行し、`src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe` を生成します。wry 0.53 が WebView2 ローダーを exe に埋め込み、WebView2 ランタイム本体は Windows 10/11 の Edge ランタイムが提供するため、配布に追加 DLL は不要です。

**前提条件:**

```bash
# cargo-xwinのインストール
cargo install cargo-xwin

# システム依存パッケージのインストール (Ubuntu/Debian)
sudo apt install clang lld llvm librsvg2-bin
```

- `clang`, `lld` — C/C++クロスコンパイラおよびリンカー（`clang-cl`, `lld-link`）
- `llvm` — リソースコンパイラ（`llvm-rc`）
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

> **注意:** `terminal-features`はクライアント接続時に評価されます。設定変更後はデタッチ（`Ctrl+b d`）して再アタッチ（`tmux attach`）する必要があります。`tmux display -p '#{client_termfeatures}'`で確認でき、`hyperlinks`が含まれていれば有効です。

eMterm独自の拡張（OSC 777のMarkdown/ダウンロード、OSC 1337のiTerm2画像）については、上記の`allow-passthrough`設定を使用してください。`emterm markdown`と`emterm image`コマンドはDCSラップを自動的に行います。

#### tmux内でのOSC 133セマンティックプロンプト

tmux 3.4以降はOSC 133マーカーを内部で消費し（tmux自身の`next-prompt`/`previous-prompt`に使用）、外側のターミナルには転送しません。そのため、tmux内ではeMtermのCtrl+上/下プロンプトジャンプやコマンド出力の折りたたみがデフォルトでは動作しません。

DCSパススルーでOSC 133マーカーをeMtermに転送するには、シェル設定に以下を追加してください。シェルが通常発行するOSC 133はそのまま動作し、tmux自身のプロンプトナビゲーションにも影響しません。

**bash** (`~/.bashrc`):
```bash
if [ -n "$TMUX" ]; then
  _emterm_osc133() { printf '\ePtmux;\e\e]133;%s\e\e\\\e\\' "$1"; }
  _emterm_first=1
  _emterm_precmd() {
    local ec=$?
    if [ -z "$_emterm_first" ]; then
      _emterm_osc133 "D;$ec"
    fi
    _emterm_first=
    _emterm_osc133 "A"
  }
  PROMPT_COMMAND="_emterm_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
  _emterm_b=$'\ePtmux;\e\e]133;B\e\e\\\e\\'
  PS1="${PS1}\[${_emterm_b}\]"
  _emterm_c=$'\ePtmux;\e\e]133;C\e\e\\\e\\'
  PS0="${PS0}${_emterm_c}"
fi
```

**zsh** (`~/.zshrc`):
```zsh
if [[ -n "$TMUX" ]]; then
  _emterm_osc133() { printf '\ePtmux;\e\e]133;%s\e\e\\\e\\' "$1" }
  _emterm_first=1
  _emterm_precmd() {
    local ec=$?
    if [[ -z "$_emterm_first" ]]; then
      _emterm_osc133 "D;$ec"
    fi
    _emterm_first=
    _emterm_osc133 "A"
  }
  _emterm_preexec() { _emterm_osc133 "C" }
  precmd_functions+=(_emterm_precmd)
  preexec_functions+=(_emterm_preexec)
  PS1="${PS1}%{$(printf '\ePtmux;\e\e]133;B\e\e\\\\\e\\\\')%}"
fi
```

tmux設定で`allow-passthrough on`が必要です。

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
| 777 | eMterm拡張 | インラインMarkdown描画、ファイルダウンロード、出力折りたたみ、ステータスバー制御 |
| 1337 | iTerm2プロトコル | インライン画像表示、ユーザー変数 |

## プロジェクト構成

```
emterm/
├── src-tauri/                  # emterm Rust クレート
│   ├── src/                    # Rust ソース
│   │   ├── main.rs             # entry: CLI / --viewer / --settings / terminal
│   │   ├── lib.rs              # モジュール宣言（GUI は #[cfg(feature = "gui")]）
│   │   ├── cli/                # markdown / json / yaml / image サブコマンド
│   │   ├── settings_core.rs    # CLI 共有（Language enum + settings_path）
│   │   ├── settings.rs         # GUI 設定ランタイム
│   │   └── ...                 # render/, ui/, tabs/, mux/, viewer/, window_host.rs ...
│   ├── viewer/web/             # Markdown / image / data ビューア TS エントリ
│   ├── settings/web/           # 設定パネル TS エントリ
│   ├── web-shared/             # 子WebView 間で共有する TS モジュール
│   ├── assets/fonts/           # 同梱 Noto フォント
│   ├── build.rs                # gui 有効時に viewer/dist + settings/dist を埋め込み
│   ├── Cargo.toml              # features: default=["gui"]、--no-default-features = CLI のみ
│   └── tests/                  # cli_subcommands.rs（統合テスト）
├── crates/
│   ├── app_settings/           # settings.json スキーマ（serde）
│   ├── term_core/              # ANSI パーサ + グリッド + Unicode 幅
│   ├── term_images/            # Kitty / SIXEL デコーダ
│   └── mux_ipc/                # mux プロトコル型定義
├── scripts/
│   ├── build-dpkg.sh           # deb パッケージャ（GUI / EMTERM_CLI_ONLY=1）
│   ├── generate-icons.sh       # SVG → PNG アイコン生成
│   └── measure-hidden-rss.sh   # RSS サンプラ
├── package.json                # bun: build:viewer / build:settings / test / typecheck
└── Makefile                    # make dev / build / cli-build / win-build / dpkg / cli-dpkg
```

## スクリプト

| コマンド | 説明 |
|---------|------|
| `bun run build:viewer` | Markdown ビューアをバンドル（`src-tauri/viewer/dist`） |
| `bun run build:settings` | 設定パネルをバンドル（`src-tauri/settings/dist`） |
| `bun run typecheck` | TypeScript 型チェック |
| `bun test` | TypeScript テスト |
| `make dev` | eMterm を起動（debug, GUI） |
| `make build` | リリースビルド（GUI） |
| `make cli-build` | リリースビルド（CLI のみ） |
| `make win-build` | cargo-xwin で Windows クロスビルド |
| `make dpkg` | GUI deb を生成 |
| `make cli-dpkg` | CLI deb を生成 |

