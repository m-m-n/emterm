# Task Document: restored-pane-read-flake-investigation — 復元 pane の書き込み・読み戻しテストの間欠失敗調査

## Change

`mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`
が `--lib` 全体の実行で間欠的に失敗する原因を調査し、結果を
`feature-docs/restored-pane-read-flake-investigation/INVESTIGATION.md` に書く。
`src-tauri/` と `crates/` 配下の追跡ファイルは、計測用の一時的な計装を含めて変更しない。

調査では次を行う。

- 対象テスト（`src-tauri/src/mux/session/pane/tests.rs` の対象関数）と、それが呼ぶ処理の書き込みから読み戻しまでの経路を洗い出し、書き込み・フラッシュ・読み取りの各箇所と、読み取りが返しうるバイト列（短い read、エコー、端末の行規律による変換など）を file:line 付きで記録する。
- 無変更の対象テストを、(a) 対象テストだけの単体実行、(b) `--lib` 全体の実行、の 2 条件で繰り返し実行する。回数は単体 20 回以上、`--lib` 全体 10 回以上とする。`--lib` 全体は、cargo で生成したテストバイナリを直接実行してもよい。条件ごとに実行回数・成功数・失敗数と、失敗時の assert メッセージ（実際に読み取ったバイト列）を記録する。
- 失敗時の assert メッセージだけで原因を区別できない場合は、リポジトリ外の一時的な再現プログラムで同じ書き込み・読み戻しの形を再現して観測する。その観測は、対象テストでの観測と区別して記録する。
- pty-reattach-timeout-investigation の計測（`--lib` 全体 51 回中 11 回失敗）は既存結果として引用し、今回の計測結果と分けて書く。
- 計測には、実行したコマンド・条件・回数を併記する。
- cargo はプロジェクトルートから `CARGO_TARGET_DIR=src-tauri/target` と `--manifest-path src-tauri/Cargo.toml` を付けて実行する。長時間の実行は Bash ツールの timeout パラメータ、または run_in_background と TaskStop で管理する。

## Expected Result

`INVESTIGATION.md` に次が揃っている。

- 失敗の原因の結論（短い read、テストの実行順序や並列実行の影響、その他の原因、またはその重なり）と、それを支える file:line と実測値。成功した実行だけを根拠に原因候補を否定していない。
- 対象テストの書き込みから読み戻しまでの経路と待ち合わせの一覧（file:line 付き）。
- 2 条件それぞれの実行回数・成功数・失敗数と、失敗時に読み取ったバイト列。
- 直すべき点の別タスク候補（対象・問題・根拠）、またはないことの明記。

作業の前後で `git diff -- src-tauri crates` に差分がない。

TM-1: `INVESTIGATION.md` にホームディレクトリ配下の絶対パスが含まれない。リポジトリ内のパスはリポジトリルートからの相対パス、リポジトリ外のホームディレクトリ配下のパスは `~/...` で書き、貼り付けるコマンド出力にも書く前に同じ置き換えをする。
