# Task Document: anchor-scan-const-generic-angle — turbofish 内の括弧の中の `<` / `>` を山括弧と誤認する件

## Change

`src-tauri/src/window_host/tests.rs` の `anchor_scan` モジュールにある
`fn find_balancing_angle_close`（`locate_call` step 2 がターボフィッシュ形の
呼び出し `callee::<...>(...)` を認識するときに使う、ジェネリック引数リストの
対応する閉じ `>` を探す関数）を修正する。

現在は `<` / `>` トークンをすべて数えるため、const ジェネリクスのブロック
（`{ 1 > 0 }`、`{ N >> 1 }`）や配列長（`[u8; 1 << 2]`）の中の比較・シフト演算子を
ジェネリック引数リストの山括弧とみなす。

`(` / `[` / `{` で 1 増え `)` / `]` / `}` で 1 減る入れ子の深さを 1 つ追い、
入れ子の深さが 0 のときの `<` / `>` だけで山括弧の深さを増減する。入れ子の深さが
正のあいだも括弧の深さは更新し続ける。入れ子の深さを負にする閉じ括弧に出会ったら、
走査を打ち切って `None` を返す（リストが閉じていない）。`end` に達しても対応する
`>` が無ければ従来どおり `None` を返す。関数のシグネチャと呼び出し元は変えない。

`->` の `>` は従来どおり入れ子の深さ 0 なら閉じとして数える（本タスクの対象外）。

関数のドキュメンテーションコメントと `locate_call` step 2 のコメントを、この挙動に
合わせて書き直す。

回帰テストを `src-tauri/src/window_host/tests.rs` に追加する。

## Expected Result

`fn probe_fn(a: i32, b: i32, c: i32, d: i32) { BODY }` を
`anchor_scan::locate_call(src, "probe_fn", "probe_call", 4)` に通したとき
（`P` は `probe_call(a, b, c, d);`）:

- `probe_call::<{ 1 > 0 }>(w, x, y, z); P` が `Err` を返す（タスクの再現手順）。
- 非呼び出しの参照 `let f = probe_call::<{ 1 > (0) }>; P` は `Ok` を返し、
  返される引数範囲 `&src[args_start..args_end]` が `a, b, c, d` と一致する。
- 呼び出し順を逆にした `P probe_call::<{ 1 > 0 }>(w, x, y, z);` も `Err` を返す。
- 次の本体がそれぞれ `Err` を返す:
  - `probe_call::<{ 1 < 2 }>(w, x, y, z); P`
  - `probe_call::<{ N >> 1 }>(w, x, y, z); P`
  - `probe_call::<[u8; 1 << 2]>(w, x, y, z); P`
  - `probe_call::<{ [1 << 2][0] > (0) }>(w, x, y, z); P`
  - `probe_call::<Result<Vec<u8>, Option<u16>>>(w, x, y, z); P`
  - `probe_call::<(Vec<u8>, u8)>(w, x, y, z); P`
  - `probe_call::<fn(Vec<u8>)>(w, x, y, z); P`
  - `probe_call/*a*/::<{ 1 > 0 }>/*b*/(w, x, y, z); P`
- `probe_call::<{ 1 > 0 }>(a, b, c, d);` のみの本体は、既存契約どおり
  ターボフィッシュ形単独として `Err` を返す。
- 既存の anchor_scan テスト（`>>` の 2 段閉じ、入れ子のジェネリック、コメント挟み込み、
  `let f = probe_call::<u8>;` の非呼び出し参照を含む）と良性編集テストが緑のまま。
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  が緑である。

## Design note (Codex consultation, 2026-09-24)

Codex（read-only）に 3 点を諮り、比較してより良いと判断された選択肢を採用した。

- 入れ子の追い方: A（`(` `[` `{` を 1 つの深さで追い、負になる閉じ括弧で `None`）、
  B（負になる閉じ括弧を無視して続ける）、C（括弧の種類をスタックで照合）から **A を採用**。
  B は対応しない閉じ括弧を越えて無関係な `>` を拾いうる。C は種類の不一致まで検出するが
  本件には不要。
- `(` の扱い: `[` `{` と同じく中の `<` / `>` を数えない側を採用。正しい構文では
  `(Vec<u8>, u8)` や `fn(Vec<u8>)` の内側の山括弧は内側で対応するため、外側の
  対応付けは保たれる。
- 基点: main を基点にする（`base_branch: main`）。未マージの兄弟ブランチ
  `em-workflow/anchor-scan-turbofish-arrow/integration` も同じ関数を変更しているため、
  後で取り込むときは本件の入れ子の処理と兄弟側の `->` 判定の両方を残す。
