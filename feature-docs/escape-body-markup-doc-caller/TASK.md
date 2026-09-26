# Task Document: task0001 — escape_body_markup の doc コメントの呼び出し元記述を実態に合わせる

## Change

`src-tauri/src/callbacks.rs` の `fn escape_body_markup` の直前にある doc コメントのうち、最後の 1 文だけを書き換える。関数本体・シグネチャ・`#[cfg(unix)]` 属性、および他のすべての項目は変えない。

- 変更前: `Unix-only because the sole caller (`notify_worker`) only reaches it behind the `#[cfg(unix)]` capability gate above.`
- 変更後: `Unix-only because [`escape_for_send`], the only production caller, is guarded by `#[cfg(unix)]`.`
- doc コメントの他の文（経緯の注記、`&` を先に置換する順序、純粋関数であること、notification-markup-fail-closed SPEC の NFR3 への言及）はそのまま残す

## Expected Result

- `escape_body_markup` の doc コメントが、本番コードでの唯一の直接の呼び出し元として `escape_for_send` を示している（本番の経路は `notify_worker` → `escape_for_send_on_demand` → `escape_for_send` → `escape_body_markup`）
- doc コメントから「sole caller (`notify_worker`)」と「capability gate above」の記述が無くなっている
- 差分が `escape_body_markup` の doc コメントの行だけに収まっている
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る
