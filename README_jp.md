# eMterm

Tauriで構築されたクロスプラットフォーム・ターミナルエミュレータ。インライン画像やMarkdown表示などのリッチな描画機能を備えています。

## 機能

- [ ] ANSI制御シーケンス完全対応
- [ ] Kitty Graphics Protocol対応
- [ ] SIXEL対応
- [ ] インラインMarkdown描画（独自OSC拡張）
- [ ] 低遅延なタイピングパフォーマンス
- [ ] クロスプラットフォーム（Linux, macOS, Windows）

## 必要条件

- [Rust](https://rustup.rs/) 1.85以上
- [Bun](https://bun.sh/) 1.0以上
- Tauriのシステム依存関係（[Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)を参照）

### Linux (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### macOS

```bash
xcode-select --install
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

