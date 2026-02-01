# Implementation Plan: Phase 10 - Shell Path / Shell Args

## Overview

Shell Path と Shell Args 設定を実際に機能させる。現在は設定画面で値を保存するのみで、PTY spawn 時に設定値が参照されない。フロントエンドとバックエンドの両方に変更が必要。

## Objectives

- 設定されたシェルパスで新しいタブのシェルを起動する
- 設定されたシェル引数をシェル起動時に渡す
- 空のシェルパスではデフォルトシェルを使用する
- 設定変更は新しいタブから適用される（既存タブには影響しない）

## Target Files

### Files to Modify

**Frontend:**

| File | Change Summary |
|------|----------------|
| `src/types/pty.ts` | `PtySpawnOptions` に `args` フィールドを追加 |
| `src/pty/client.ts` | `spawn()` で `args` をバックエンドに渡す |
| `src/terminal-app/index.ts` | `init()` で設定から `shell_path` と `shell_args` を読み取り、spawn に渡す |

**Backend (Rust):**

| File | Change Summary |
|------|----------------|
| `src-tauri/src/lib.rs` | `pty_spawn` コマンドに `args` パラメータを追加 |
| `src-tauri/src/pty/manager.rs` | `create_session()` と `create_session_atomic()` に `args` パラメータを追加 |
| `src-tauri/src/pty/session.rs` | `PtySession::new()` に `args` パラメータを追加、`CommandBuilder` に引数を設定 |

## Implementation Steps

1. **テストを先に書く**
   - `src/pty/client.test.ts` に `spawn()` がシェルと引数を渡すテストを追加
   - `src-tauri/src/pty/session.rs` のテストに引数付き spawn のテストを追加

2. **Backend: `PtySession::new()` に args パラメータを追加**
   - `args: Option<Vec<String>>` パラメータを追加
   - `CommandBuilder` にシェル引数を設定

3. **Backend: `PtyManager` の `create_session` 系メソッドに args を伝搬**
   - `create_session()` と `create_session_atomic()` に `args` パラメータを追加

4. **Backend: `pty_spawn` コマンドに args パラメータを追加**
   - `args: Option<Vec<String>>` を追加
   - `create_session_atomic()` に渡す

5. **Frontend: `PtySpawnOptions` に args フィールドを追加**
   - `args?: string[]` フィールドを追加

6. **Frontend: `PtyClient.spawn()` で args を渡す**
   - `invoke("pty_spawn", ...)` に `args` を含める

7. **Frontend: `TerminalApp.init()` で設定値を使用**
   - `SettingsService.getCached()` から `shell_path` と `shell_args` を読み取る
   - `ptyClient.spawn()` に `shell` と `args` を渡す

## Component Contracts

### `PtySession::new(id, shell, args, cols, rows)` (updated)

| Item | Description |
|------|-------------|
| Precondition | `shell` はシェルパス、`args` はオプションの引数リスト |
| Postcondition | 指定されたシェルと引数でプロセスが起動する |
| Postcondition (args=None) | 引数なしでシェルが起動する |

### `pty_spawn` command (updated)

| Item | Description |
|------|-------------|
| Precondition | `shell: Option<String>`, `args: Option<Vec<String>>`, `cols`, `rows` |
| Postcondition | セッションが作成され、session_id が返される |

### `PtyClient.spawn(options)` (updated)

| Item | Description |
|------|-------------|
| Precondition | `options` に `shell?`, `args?`, `cols?`, `rows?` |
| Postcondition | バックエンドの `pty_spawn` にすべてのパラメータが渡される |

### `TerminalApp.init()` (updated)

| Item | Description |
|------|-------------|
| Precondition | `SettingsService.getCached()` が有効な設定を返す |
| Postcondition | 設定の `shell_path` と `shell_args` が `ptyClient.spawn()` に渡される |
| Postcondition (empty shell_path) | デフォルトシェルが使用される |

## Processing Flow

```
1. TerminalApp.init() が呼ばれる
2. SettingsService.getCached() から設定を取得
3. shell_path が空か判定
   +-- 空 --> shell = undefined (バックエンドでデフォルトを使用)
   +-- 非空 --> shell = shell_path
4. shell_args が空か判定
   +-- 空 --> args = undefined
   +-- 非空 --> args = shell_args
5. ptyClient.spawn({ shell, args, cols, rows }) を呼ぶ
6. Frontend が invoke("pty_spawn", { shell, args, cols, rows }) を実行
7. Backend が PtySession::new(id, shell, args, cols, rows) でプロセスを起動
8. CommandBuilder にシェルと引数を設定してプロセスを spawn
```

## Test Strategy

### Test File (Rust): `src-tauri/src/pty/session.rs`

| Test Case | Description |
|-----------|-------------|
| Session creation with default shell | デフォルトシェルでの起動 |
| Session creation with custom args | カスタム引数付きの起動 |

### Test File (TypeScript): `src/pty/client.test.ts`

| Test Case | Description |
|-----------|-------------|
| `spawn()` passes shell and args to invoke | shell と args がバックエンドに渡されること |
| `spawn()` without args omits args parameter | args が未指定の場合は省略されること |

## Acceptance Criteria

- [ ] シェルパスを設定すると、新しいタブで指定シェルが起動する
- [ ] シェル引数を設定すると、起動時に引数が渡される
- [ ] 空のシェルパスではデフォルトシェルが使用される
- [ ] 設定変更は新しいタブから適用される（既存タブには影響しない）
