# Task Document: inherited-pty-drop-fd-flake-investigation — inherited_pty の drop 後 fd 比較テストの間欠失敗調査

## Change

`mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor`
（`src-tauri/src/mux/inherited_pty.rs:456-479`）が `--lib` 全体の実行で間欠的に失敗する原因を調査し、
結果を `feature-docs/inherited-pty-drop-fd-flake-investigation/INVESTIGATION.md` に日本語で書く。
`src-tauri/` と `crates/` 配下の追跡ファイルは、計測用の一時的な計装を含めて変更しない。

調査では次を行う。

- 対象テストと、それが使う `open_master_dup`（`inherited_pty.rs:193-213`）、`stat_rdev`（`inherited_pty.rs:257-273`）、`InheritedMasterPty` の Drop について、関わる記述子（`pair.master` の fd、`dup_fd`、drop 後に `dup_fd` の番号を得うる記述子）と、各記述子に `fstat` が返す `st_rdev` を file:line 付きで記録する。比較設計の前提を述べたコメント（`inherited_pty.rs:466-471`、`inherited_pty.rs:260-265`）の主張を、実測値と突き合わせて記録する。
- リポジトリ外（`/tmp` 配下）の小さな C プログラムで、別々に開いた複数の PTY ペアについて master fd・その dup・slave fd の `st_rdev`（major:minor と数値）を `fstat` で測り、`/dev/ptmx` の `stat` 結果と並べて記録する。master は portable-pty 0.8 と同じ `openpty` 経路で開いたものを含める。
- 同じ C プログラムで、対象テストと同じ手順（PTY ペアを開く → master を dup → dup の `st_rdev` を記録 → dup を close → 別の PTY ペアを開く、または別の master を dup する → 解放された番号を `fstat` して `st_rdev` を比較）を、スレッド競合に頼らず明示的な順序で実行し、解放された番号が再利用されたか、その記述子の `st_rdev` が元の値と等しいかを記録する。この観測は対象テストでの観測と区別して記録する。
- 原因候補として少なくとも (a) 別の PTY master またはその複製による番号の再利用、(b) Drop で記述子が close されていない、(c) PTY 以外の記述子による番号の再利用、を挙げ、それぞれの採否を file:line と実測値で示す。成功した実行だけを根拠に候補を否定しない。
- `--lib` のテストプロセス内で PTY master またはその複製を開く処理の代表例（portable-pty の `openpty`、`try_clone_reader` / `take_writer` の dup、`dup_cloexec` など）をコード読解で特定し、file:line で記録する。
- 無変更の対象テストは、フィルタを付けた単体実行を逐次で最大 5 回まで行ってよい。単体実行での成功を間欠失敗の否定に使わない。`--lib` 全体の反復実行や、CPU の取り合いを起こす条件での再現は行わない。負荷をかけないと確かめられない事柄は、その時点で止めて未確定として記録する。
- restored-pane-read-flake-investigation の計測（2 条件それぞれ 25 回中 1 回失敗、`inherited_pty.rs:474:27`、両辺 1282）は既存結果として引用し、今回の計測と分けて書く。
- 計測には、実行したコマンド・条件・回数を併記する。
- cargo はプロジェクトルートから `CARGO_TARGET_DIR=src-tauri/target` と `--manifest-path src-tauri/Cargo.toml` を付けて実行する。長時間の実行は Bash ツールの timeout パラメータ、または run_in_background と TaskStop で管理する。

## Expected Result

`INVESTIGATION.md` に次が揃っている。

- 問い（間欠失敗の原因は何か、再利用された fd 番号が別テストの PTY master を指しているのか）への結論と、それを支える file:line と実測値、および証拠の限界。
- PTY の master・その dup・slave と `/dev/ptmx` の `st_rdev` の実測値（実行コマンドと条件付き）。
- 順序を明示した再現の手順と結果（番号の再利用の有無、`st_rdev` の比較結果）。対象テストでの観測と区別されている。
- 原因候補 (a)(b)(c) それぞれの採否と根拠。
- PTY master またはその複製を開く処理の代表例（file:line 付き）。
- 既存計測の引用（今回の計測と分けて記載）。
- 直すべき点の別タスク候補（対象・問題・根拠）、またはないことの明記。

記録された計測コマンドに、CPU 負荷をかける処理や CPU 競合を再現手段とする反復実行が含まれない。

作業の前後で `git diff -- src-tauri crates` に差分がない。

TM-1: `INVESTIGATION.md` にホームディレクトリ配下の絶対パスが含まれない。リポジトリ内のパスはリポジトリルートからの相対パス、リポジトリ外のホームディレクトリ配下のパスは `~/...` で書き、貼り付けるコマンド出力にも書く前に同じ置き換えをする。
