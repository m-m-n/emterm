# Task Document: match-arm-shadow-scan-fix — rule 3b の match アームシャドウ検出漏れ

## Change

`src-tauri/src/window_host/tests.rs` の `fn is_rebound_before` にある rule 3b
（`match` アームのパターンによる受け手識別子シャドウ検出）のアーム走査ループを
修正する。

現在 `in_pattern` は、トップレベルの `=>` で false になり、
`Tok::Punct(",") if nest == 0 && !in_pattern` の分岐でのみ true に戻る。
このため `None => {}` のように末尾カンマを持たないアーム本体を通過すると
`in_pattern` が false のまま固定され、以降のアームのパターンが走査されない。

閉じ波括弧 `}` が `nest` を 0 に戻し、かつ `!in_pattern` のときに限り
`in_pattern` を true に戻し、`seg_start` をその次のトークンに進める。
閉じ丸括弧 `)` と閉じ角括弧 `]` ではアーム終端とみなさない。
既存の `,` 分岐はそのまま残す。

既存の閉じ括弧分岐は `nest -= 1` するだけなので、次の形に置き換える:

```rust
Tok::Punct(p) if p == "}" || p == ")" || p == "]" => {
    nest -= 1;
    if p == "}" && nest == 0 && !in_pattern {
        in_pattern = true;
        seg_start = m + 1;
    }
}
```

あわせて、rule 3b のブロック直前にある「アーム本体は次のアームのパターン検査に
混入しない」旨のドキュメンテーションコメントを、この修正後の実際の挙動に
合わせて書き直す。書くべき実挙動は次の 3 点:

- ブロック本体のアームは、末尾カンマの有無にかかわらず、その閉じ `}` で分離される。
- 非ブロック本体のアームは、従来どおりトップレベルの `,` で分離される。
- 構造体リテラルや `if`／入れ子 `match` をトップレベルに含むアーム本体は、その内側の
  `}` をアーム終端と誤認しうる。判定は保守的な過剰拒否側に倒れる（テストが赤くなる）
  ため安全側であり、実際の名前解決は行っていない。

回帰テストとして、束縛を 2 番目以降のアームに置いた negative ケースを
`src-tauri/src/window_host/tests.rs` に追加する。

## Expected Result

- `match owner { None => {} Some(ctx) => {} }` に続く
  `svc::apply_with_held(a, ctx.held_flag);` のように、末尾カンマ無しブロック本体
  アームの後ろのアームで受け手識別子がシャドウされる入力が、判定関数に拒否される。
- 束縛が 3 番目のアームにある場合（先行する末尾カンマ無しブロック本体アームが
  2 つ続く場合）も同様に拒否される。
- アーム本体の内側に閉じ丸括弧・閉じ角括弧を含む場合
  （例: `match owner { None => { consume((1, [2])); } Some(ctx) => {} }`）でも、
  内側の閉じ括弧をアーム終端と誤認せず、後続アームの束縛を拒否する。
- 既存の
  `judge_call_rejects_a_match_arm_pattern_shadow_of_the_receiver_before_the_call`
  （束縛が第 1 アーム）、および末尾カンマ付きアームの後ろに束縛がある場合も、
  引き続き拒否される。
- 受け手識別子を束縛しない `match`（例:
  `match owner { None => {} Some(other) => {} }`）は引き続き受理され、
  新たな誤検出を起こさない。
- アーム本体がメソッド連鎖を含む受理ケース
  （例: `match owner { Some(x) => make().with(ctx), None => {} }` に続く呼び出しで
  `ctx` を束縛するパターンが後続に無い形）が、閉じ丸括弧を終端とみなさないことに
  よって引き続き受理される。
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  が緑である。

## Design note (Codex consultation, 2026-09-21)

当初の起票は「`}` / `)` / `]` のいずれでも `nest` が 0 に戻った時点で
`in_pattern` を復帰させる」案（案 A）を指定していた。Codex（gpt-6-astra,
read-only, reasoning xhigh）に案 A / 案 B（`}` のみを終端とする）/ その他 の
比較を諮り、**案 B を採用**した。根拠:

- 案 B でも今回の見逃し（末尾カンマ無しブロック本体アーム以降の読み飛ばし）は
  完全に解消する。過剰拒否が安全側だとしても、同じ効果が得られる以上、案 A の
  余分な過剰拒否を積極的に選ぶ理由が無い。
- 案 A が新たに過剰拒否するのは「トップレベルの閉じ括弧より後ろに検査対象名が
  現れる非ブロック本体」。具体例: `make().with(target)`, `make().target()`,
  `factory()(target)`, `make()[target]`, `make() + target`。通常の
  `foo(bar),` 形だけでは誤検出にならない（`bar` は切り替え位置より前、
  続く `None` は実際に次のパターン）点も確認した。
- 案 C（本体の先頭トークンが `{` のときだけ閉じ `}` を終端とみなすフラグ方式）は
  過剰拒否を減らす代わりに、`None => if cond { a() } else { b() }` や末尾カンマ
  無しの入れ子 `match` 本体で見逃しを増やすため採らない。
- 案 B に残る限界（構造体リテラル `Value { .. }` や `if`/`match` 本体の内側の
  `}` をアーム終端と誤認しうる）は案 A・案 C とも共通であり、過剰拒否側に倒れる
  ためこの層では許容する。上記ドキュメンテーションコメントに明記する。
