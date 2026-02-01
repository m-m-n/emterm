# eMterm E2E Tests

## Overview

eMterm のエンドツーエンドテスト。Tauri の WebView (WebKitGTK) を tauri-driver + WebdriverIO で操作して検証する。

## Test Strategy

### Unit Tests vs E2E Tests

| 種別 | 対象 | 実行環境 | フレームワーク |
|------|------|---------|-------------|
| Unit Tests | TypeScript ロジック (settings-applier, types 等) | Docker (`bun test`) | Bun test runner |
| Unit Tests | Rust ロジック (config, ANSI parser 等) | Docker (`cargo test`) | Rust test |
| E2E Tests | アプリ全体の動作 (UI操作、レンダリング、タブ管理等) | Docker (Xvfb + tauri-driver) | WebdriverIO + Mocha |

### When to Write E2E Tests

- UI操作のフロー（設定パネルの開閉、タブ操作）
- レンダリング結果の確認（画像表示、Markdown表示）
- キーボードショートカットの動作
- 複数コンポーネント間の連携

### When NOT to Write E2E Tests (Unit Test で十分)

- 純粋関数のロジック（`buildFontFamilyChain` 等）
- 型定義の整合性
- serde のシリアライズ/デシリアライズ
- CSS変数の設定ロジック

## Directory Structure

```
e2e-tests/
  specs/           - テストファイル (*.e2e.js)
  helpers/         - テストヘルパー関数
  screenshots/     - スクリーンショット出力 (gitignore済み)
  wdio.conf.js     - ローカル実行用設定
  wdio.docker.conf.js - Docker実行用設定
  package.json     - E2Eテスト用依存関係
```

## Running Tests

### Docker (推奨)

```bash
# フルサイクル: install → build → test
./scripts/run-e2e-docker.sh

# 個別ステップ
./scripts/run-e2e-docker.sh install    # 依存関係インストール
./scripts/run-e2e-docker.sh build      # アプリビルド
./scripts/run-e2e-docker.sh test       # 全テスト実行
./scripts/run-e2e-docker.sh test foo.e2e.js  # 特定テスト実行
./scripts/run-e2e-docker.sh clean      # ボリューム削除
```

### Unit Tests (Docker)

```bash
# TypeScript
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# Rust
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript 型チェック
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

## Docker Compose Services

`docker-compose.e2e.yml` で定義:

| Service | 用途 |
|---------|------|
| `install` | `bun install` + `npm ci` (依存関係) |
| `build` | `bun tauri build --debug --no-bundle` |
| `e2e-test` | Xvfb + tauri-driver + wdio (E2Eテスト) |

### Named Volumes

| Volume | 用途 |
|--------|------|
| `cargo-target` | Rust ビルドキャッシュ |
| `node-modules` | フロントエンド依存関係 |
| `e2e-node-modules` | E2Eテスト依存関係 |

## Writing Tests

### File Naming

テストファイルは `specs/` 以下に `*.e2e.js` の命名規則で配置する。

### Example

```javascript
describe("Feature Name", () => {
  it("should do something", async () => {
    // tauri-driver 経由で WebView を操作
    const element = await $("selector");
    await element.click();

    // 結果を検証
    const result = await $("result-selector");
    expect(await result.getText()).toBe("expected");
  });
});
```

### Screenshot on Failure

テスト失敗時のスクリーンショットは `e2e-tests/screenshots/` に保存される（Docker volume mount 経由でホストに出力）。

## Configuration

### wdio.conf.js (ローカル)

- ビルドを `onPrepare` で自動実行
- タイムアウト: 2分
- `tauri-driver` をPATHから検索

### wdio.docker.conf.js (Docker)

- ビルドは事前に `docker compose run --rm build` で実行済み前提
- タイムアウト: 3分（Docker環境のオーバーヘッド考慮）
- `tauri-driver` と `WebKitWebDriver` のフルパス指定
