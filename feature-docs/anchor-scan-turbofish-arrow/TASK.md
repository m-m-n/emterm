# Task Document: anchor-scan-turbofish-arrow — turbofish 内の `->` を閉じ `>` と誤認する件

## Change

`src-tauri/src/window_host/tests.rs` の `anchor_scan` モジュールにある
`fn find_balancing_angle_close`（`locate_call` step 2 がターボフィッシュ形の
呼び出し `callee::<...>(...)` を認識するときに使う、ジェネリック引数リストの
対応する閉じ `>` を探す関数）を修正する。

現在は `>` トークンを見るたびに深さを 1 減らすため、`fn(u8) -> u8` のように
ジェネリック引数に `->` を含む型があると、`->` の `>` でリストが閉じたと判定する。
その直後が `(` ではないため呼び出しとして数えられず、呼び出しが 2 つある本体でも
`locate_call` が `Ok` を返す。

直前のトークンが `Punct('-')` で、かつその終端バイト位置が `>` の開始バイト位置と
一致する（`toks[m - 1].end == toks[m].start`、`m - 1` は `after_open` 以上）`>` は
`->` の一部とみなし、深さを変えずに読み飛ばす。それ以外の `>` は従来どおり深さを
1 減らす（`>>` は 2 段閉じる）。空白やコメントを挟んだ `- >` は `->` とみなさない。

関数のドキュメンテーションコメントと `locate_call` step 2 のコメントを、この挙動に
合わせて書き直す。

回帰テストを `src-tauri/src/window_host/tests.rs` に追加する。

## Expected Result

`fn probe_fn() { BODY }` を `locate_call(src, "probe_fn", "probe_call", 4)` に通したとき
（`P` は `probe_call(a, b, c, d);`）:

- `probe_call::<fn(u8) -> u8>(w, x, y, z); P` が `Err` を返す（タスクの再現手順）。
- 呼び出し順を逆にした `P probe_call::<fn(u8) -> u8>(w, x, y, z);` も `Err` を返す。
- `probe_call::<Box<dyn Fn(u8) -> u8>>(w, x, y, z); P`、
  `probe_call::<fn() -> fn(u8) -> u8>(w, x, y, z); P`、
  `probe_call::<fn() -> Vec<u8>>(w, x, y, z); P` が `Err` を返す。
- `probe_call::<fn(u8) /*a*/ -> /*b*/ u8>/*c*/(w, x, y, z); P` が `Err` を返す。
- `probe_call::<fn(u8) -> u8>(w, x, y, z);` のみの本体は、既存契約どおり
  ターボフィッシュ形単独として `Err` を返す。
- 非呼び出しの参照 `let f = probe_call::<fn(u8) -> u8>; P` は `Ok` を返し、
  返される引数範囲が `a, b, c, d` と一致する。
- `>` の前に空白やコメントを挟んだ `-`（`probe_call::<- >(w, x, y, z); P`、
  `probe_call::<-/*c*/>(w, x, y, z); P`）は `->` とみなされず、`>` がリストを閉じて
  呼び出しが数えられ `Err` を返す。
- 負の const ジェネリック引数 `probe_call::<-1>(w, x, y, z); P` は、`>` の直前が数値
  トークンなので通常どおりリストが閉じ、`Err` を返す。
- 既存の anchor_scan テスト（`>>` の 2 段閉じ、入れ子のジェネリック、コメント挟み込みを
  含む）と良性編集テストが緑のまま。
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  が緑である。

## Design note (Codex consultation, 2026-09-24)

`->` の判別方法として、A（直前トークンが `-` かつバイト隣接の `>` を `->` とみなす）、
B（隣接を問わず直前トークンが `-` なら `->` とみなす）、C（その他）を Codex
（read-only, reasoning xhigh）に諮り、**A を採用**した。根拠:

- scan は空白・コメントを捨てるため、トークン列上の連続は元の文字の隣接を意味しない。
  B は `- >` や `-/*c*/>` まで `->` と誤認する。
- 既存の `PosTok` のバイト位置だけで実際の `->` を判別でき、変更は
  `find_balancing_angle_close` 内で完結する。
- `>>` は従来どおり 2 段閉じ、`::<-1>` の `>` は直前が数値トークンなので通常どおり閉じる。
- ターボフィッシュ形単独の呼び出しを `Err` にする既存契約を保てる。
