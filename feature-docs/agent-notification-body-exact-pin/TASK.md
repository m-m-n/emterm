# Task Document: task0001 — agent_notification_body のサニタイズ固定テストを本文全体の一致に強化する

## Change

`src-tauri/src/notifications.rs` の tests モジュールにある 2 テストの assertion とコメントを置き換える。テスト関数の名前・入力・ロケール・状態は変えない。製品コード（`sanitize_title` / `agent_notification_body`）と通知本文のフォーマットは変えない。

- `agent_notification_body_strips_csi_sequences_from_tab_title`
    - 入力: `"\x1b[31mred\x1b[0m title"`、`transition(AgentState::Blocked)`、`Locale::En`
    - assertion を `assert_eq!(body, "claude: red title (blocked)")` にする
    - 否定形の assertion（`!body.contains('\x1b')`）は残さない
    - 直前のコメントを、本文全体を固定して CSI の除去と可視テキストの保存を確かめる内容に書き換える。「pins the property, not one exact fixture string (task plan Test Notes)」の記述は削除する
- `agent_notification_body_strips_control_chars_from_tab_title`
    - 入力: `"a\x07b\x00c\u{9f}d"`、`transition(AgentState::Blocked)`、`Locale::En`
    - assertion を `assert_eq!(body, "claude: abcd (blocked)")` にする
    - 否定形の assertion（C0/DEL/C1 の不在チェック）は残さない
    - 直前のコメントを、本文全体を固定して C0/C1 制御文字の除去と可視テキストの保存を確かめる内容に書き換える。入力に DEL は含まれないので DEL には言及しない

## Expected Result

- CSI テストが、CSI の残りかす（`[31m` / `[0m`）の除去と可視テキスト（`red title`）の保存の両方を、本文全体 `"claude: red title (blocked)"` との一致で固定している
- 制御文字テストが、本文全体 `"claude: abcd (blocked)"` との一致で固定している
- `sanitize_title` の CSI 除去が失われて C0 フィルタだけが残る実装（本文 `"claude: [31mred[0m title (blocked)"`）や、タイトルを空にする実装（本文 `"claude:  (blocked)"`）では、どちらのテストも失敗する
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る
