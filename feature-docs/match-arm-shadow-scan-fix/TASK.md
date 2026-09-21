# Task Document: match-arm-shadow-scan-fix — rule 3b の match アームシャドウ検出漏れ

## Change

`src-tauri/src/window_host/tests.rs` の `fn is_rebound_before` にある rule 3b
（`match` アームのパターンによる受け手識別子シャドウ検出）のアーム走査ループを
修正する。

現在 `in_pattern` は、トップレベルの `=>` で false になり、
`Tok::Punct(",") if nest == 0 && !in_pattern` の分岐でのみ true に戻る。
このため `None => {}` のように末尾カンマを持たないアーム本体を通過すると
`in_pattern` が false のまま固定され、以降のアームのパターンが走査されない。

閉じ括弧（`}` / `)` / `]`）で `nest` が 0 に戻った時点で `!in_pattern` なら
`in_pattern` を true に戻し、`seg_start` をその次のトークンに進める分岐を追加する。
既存の `,` 分岐はそのまま残す。

あわせて、rule 3b のブロック直前にある「アーム本体は次のアームのパターン検査に
混入しない」旨のドキュメンテーションコメントを、この修正後の実際の挙動
（ブロック本体は分離されるが、非ブロック本体の式が最初のトップレベル閉じ括弧より
後ろに続く場合は次のパターン区間へ混入しうる = 保守的な過剰拒否側に倒れる）に
合わせて書き直す。

回帰テストとして、束縛を 2 番目以降のアームに置いた negative ケースを
`src-tauri/src/window_host/tests.rs` に追加する。

## Expected Result

- `match owner { None => {} Some(ctx) => {} }` に続く
  `svc::apply_with_held(a, ctx.held_flag);` のように、末尾カンマ無しブロック本体
  アームの後ろのアームで受け手識別子がシャドウされる入力が、判定関数に拒否される。
- 束縛が 3 番目のアームにある場合（先行する末尾カンマ無しブロック本体アームが
  2 つ続く場合）も同様に拒否される。
- 既存の
  `judge_call_rejects_a_match_arm_pattern_shadow_of_the_receiver_before_the_call`
  （束縛が第 1 アーム）、および末尾カンマ付きアームの後ろに束縛がある場合も、
  引き続き拒否される。
- 受け手識別子を束縛しない `match`（例:
  `match owner { None => {} Some(other) => {} }`）は引き続き受理され、
  新たな誤検出を起こさない。
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  が緑である。
