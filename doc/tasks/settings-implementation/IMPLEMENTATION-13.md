# Implementation Plan: Phase 13 - URL Detection

## Overview

URL Detection 設定を実際に機能させる。現在は設定画面で値を保存するのみで、URL 検出・ハイライト機能が未実装。

## Objectives

- ターミナル出力内の URL を正規表現で検出してハイライト表示する
- Ctrl+クリックで外部ブラウザに遷移する
- 設定を OFF にすると URL 検出が無効になる

## Target Files

### Files to Create

| File | Purpose |
|------|---------|
| `src/terminal/url-detector.ts` | URL 検出ロジック（正規表現マッチ、位置計算） |

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/canvas-renderer.ts` | URL ハイライトの描画 |
| `src/terminal-app/index.ts` | Ctrl+クリックハンドラ、URL 検出の有効/無効制御 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/url-detector.test.ts` を新規作成
   - URL 正規表現のマッチテスト（各種 URL フォーマット）
   - 無効化時のテスト

2. **URL 検出モジュールの作成**
   - `src/terminal/url-detector.ts` を新規作成
   - URL 正規表現パターン: `http(s)://`, `ftp://`, `file://` で始まる URL を検出
   - 入力: テキスト行
   - 出力: URL の開始位置、終了位置、URL 文字列のリスト

3. **URL ハイライトの描画**
   - テキスト描画後、可視行の URL を検出
   - URL 部分にアンダーラインと目立つ色のオーバーレイを描画
   - `url_detection` 設定が OFF の場合はスキップ

4. **Ctrl+クリックハンドラ**
   - クリックイベントで Ctrl キーが押されているか判定
   - クリック位置が URL 上にあるか判定
   - URL 上であれば Tauri の `shell.open()` API でブラウザを起動

## Component Contracts

### URL Detector

| Item | Description |
|------|-------------|
| Input | テキスト行（文字列） |
| Output | URL マッチのリスト（開始列、終了列、URL 文字列） |
| Constraint | `http://`, `https://`, `ftp://`, `file://` プロトコルをサポート |

### URL Highlight Rendering

| Item | Description |
|------|-------------|
| Precondition | `url_detection` 設定が ON、可視行のテキストが利用可能 |
| Postcondition | URL 部分にアンダーライン+色のハイライトが描画される |

### Ctrl+Click Handler

| Item | Description |
|------|-------------|
| Precondition | Ctrl+クリックイベントが発生し、クリック位置に URL がある |
| Postcondition | URL がデフォルトブラウザで開かれる |

## Processing Flow

```
1. テキストが描画された後
2. url_detection 設定を確認
   +-- OFF --> 何もしない
   +-- ON --> 続行
3. 可視行のテキストを URL 検出器に渡す
4. 検出された URL の位置にハイライトを描画

5. Ctrl+クリックイベント発生時
   +-- クリック位置をグリッド座標に変換
   +-- その位置に URL があるか判定
   +-- URL がある場合 --> Tauri shell.open() で開く
   +-- URL がない場合 --> 何もしない
```

## Test Strategy

### Test File: `src/terminal/url-detector.test.ts` (new)

| Test Case | Description |
|-----------|-------------|
| Detects `https://example.com` | HTTPS URL の検出 |
| Detects `http://example.com/path?q=1` | パスとクエリパラメータ付き URL |
| Detects `ftp://files.example.com` | FTP URL の検出 |
| Detects `file:///tmp/file.txt` | file URL の検出 |
| Returns empty for no URLs | URL がない行では空リスト |
| Multiple URLs on one line | 1 行に複数 URL |
| URL detection disabled returns empty | 無効化時は空リスト |

## Acceptance Criteria

- [ ] ターミナル出力内の URL が検出・ハイライトされる
- [ ] Ctrl+クリックで外部ブラウザに遷移する
- [ ] 設定を OFF にすると URL 検出が無効になる
