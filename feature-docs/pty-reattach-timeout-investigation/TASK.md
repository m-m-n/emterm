# Task Document: pty-reattach-timeout-investigation — reattach カラー照会テストの読み取りタイムアウト調査

## Change

`mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`
が読み取りタイムアウトで失敗する原因を調査し、結果を
`feature-docs/pty-reattach-timeout-investigation/INVESTIGATION.md` に書く。
`src-tauri/` と `crates/` 配下の追跡ファイルは、計測用の一時的な計装を含めて変更しない。

調査では次を行う。

- 対象テストと、それが呼ぶ補助関数の待ち合わせ箇所を洗い出し、各箇所のタイムアウト値・待つ条件・待ち方（ポーリングかブロッキング読み取りか）・タイムアウト時の失敗の仕方を file:line 付きで記録する。
- 無変更の対象テストを、(a) 対象テストだけの単体実行、(b) `--lib` 全体の実行、(c) 並行ビルドなどの CPU 負荷を同時にかけた実行、の 3 条件で繰り返し実行する。条件ごとに実行回数・成功数・失敗数、失敗したときの待ち箇所と経過時間を記録する。回数の目安は単体 20 回以上、`--lib` 全体と負荷下は数回以上とする。
- 計測には、実行したコマンド・条件・回数・同時にかけた負荷の内容を併記する。
- cargo はプロジェクトルートから `CARGO_TARGET_DIR=src-tauri/target` と `--manifest-path src-tauri/Cargo.toml` を付けて実行する。負荷プロセスと長時間の実行は Bash ツールの timeout パラメータ、または run_in_background と TaskStop で管理する。

## Expected Result

`INVESTIGATION.md` に次が揃っている。

- 失敗の原因が次のどれに当たるかの結論と、それを支える file:line と実測値。
  (1) 負荷による一時的な失敗で、テスト設計としては許容できる範囲。
  (2) テスト側のタイムアウト・待ち合わせの設計の問題。
  (3) どちらでもない原因（mux 本体の挙動など）、または複数の原因の重なり。
- 対象テストの待ち合わせ箇所とタイムアウト値の一覧（file:line 付き）。
- 3 条件それぞれの実行回数・成功数・失敗数と、失敗時の待ち箇所・経過時間。
- 直すべき点の別タスク候補（対象・問題・根拠）、またはないことの明記。

作業の前後で `git diff -- src-tauri crates` に差分がない。

TM-1: `INVESTIGATION.md` にホームディレクトリ配下の絶対パスが含まれない。リポジトリ内のパスはリポジトリルートからの相対パス、リポジトリ外のホームディレクトリ配下のパスは `~/...` で書き、貼り付けるコマンド出力にも書く前に同じ置き換えをする。
