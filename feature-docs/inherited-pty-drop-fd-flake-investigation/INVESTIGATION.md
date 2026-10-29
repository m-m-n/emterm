# 調査: inherited_pty の drop 後 fd 比較テストの間欠失敗

対象テスト: `mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor`
（`src-tauri/src/mux/inherited_pty.rs:456-479`）。

調査日は 2026-10-29。作業ブランチの HEAD は `362b368d`。`src-tauri/` と `crates/` 配下の追跡ファイルは、
計測用の一時的な計装も含めて変更していない（`git diff -- src-tauri crates` は作業の前後とも空）。
計装と計測プログラムはすべてリポジトリの外（`/tmp` 配下）に置いた（3.1、4.2）。

`portable-pty 0.8.1` と `filedescriptor 0.8.3` は `Cargo.lock` のバージョンで、これらのパスは各 crate のルートからの相対で書く。

## 1. 結論

**問い 1「間欠失敗の原因は何か」**: テストの比較設計が、再利用された fd 番号の持ち主を区別できないこと。
`Drop` が `dup_fd` を close した後、`stat_rdev(dup_fd)` を呼ぶまでの間（`inherited_pty.rs:464` から `:472`）に、
同じプロセス内の別スレッドが fd を 1 つ開くと、カーネルは最小の空き番号として解放済みの `dup_fd` の番号を渡す。
その記述子が PTY master（またはその複製）だと `st_rdev` は元の値と同じ 5:2（数値 1282）になり、
`assert_ne!(rdev, original_rdev)`（`inherited_pty.rs:474:27`）が両辺 1282 で失敗する。

**問い 2「再利用された fd 番号が別テストの PTY master を指しているのか」**: はい。ただし根拠は間接的である。

- 失敗時の両辺 1282 は、`/dev/ptmx` を開いて得た記述子の `st_rdev` と等しい（`/dev/ptmx` は 5:2、`stat` の結果は 3.1）。
  別々に開いた 4 組の PTY の master、その `dup`、`F_DUPFD_CLOEXEC` の複製はどれも 5:2 だった（3.2）。
  slave（136:N）、通常ファイル、pipe、socket、`/dev/null` などは 5:2 にならない（3.2、3.3）。
- このテストのスレッドは `drop(adapter)` と `stat_rdev` の間に何も開かない（`strace` で 2 つの syscall が隣り合う、4.1）。
  このスレッド以外で `/dev/ptmx` を開くのは、同時に走る別テストとそのテストが起動したスレッドだと考えられる。
- 同じ失敗（`inherited_pty.rs:474:27`、`left: 1282` `right: 1282`）は、無変更のテストバイナリに対し、
  drop の直後に `/dev/ptmx` を 1 つ開く外部の注入を行うと、競合なしで再現した（1 回の実行、4.2）。
  `/dev/null` を注入したときは成功した（1 回の実行）。

証拠の限界（9 に詳細）:

- 今回の計測では、このテストの自然な失敗を観測していない（注入なしの単体実行 2 回と `--lib` 全体のベースライン 1 回は成功。
  注入ありの 1 回の失敗は自然な失敗ではない）。自然な失敗は 7 の既存計測の引用だけである。
- 自然な失敗の時点で、その fd 番号をどのテストが持っていたかは観測していない。失敗メッセージには fd 番号も持ち主も出ない。
- `--lib` 全体で約 25 回に 1 回という頻度（7）が、`drop` から `fstat` までの狭い区間で説明できるかは確かめていない。

## 2. 対象テストと関わる記述子

テストの手順と、各手順で関わる記述子。fd 番号は 4.1 の `strace`（対象テスト単体）で観測した値で、
`--lib` 全体の実行では他のテストの記述子があるため異なる。

| # | 場所 | 手順 | 記述子 | `fstat` の `st_rdev` |
|---|---|---|---|---|
| T1 | `inherited_pty.rs:459` → `:193-213` → `:201-203` | `open_master_dup()` が `portable_pty::native_pty_system().openpty()` で PTY を開く | `pair.master`（fd 3）と `pair.slave`（fd 4）。`_pair` に束縛され、テスト終了まで開いたまま | master 5:2（1282）、slave 136:N（3.2 の C プログラムの測定。テスト内では `fstat` していない） |
| T2 | `inherited_pty.rs:204-212` | `pair.master.as_raw_fd()` を `libc::dup` する | `dup_fd`（fd 5）。`pair.master` とは別の番号で、同じ open file description を指す | 5:2（1282）（4.1 の `fstat(5)`） |
| T3 | `inherited_pty.rs:460-461` → `:266-273` | `original_rdev = stat_rdev(dup_fd)` | （`dup_fd` の `fstat`） | 1282 |
| T4 | `inherited_pty.rs:462` → `:37-49` | `InheritedMasterPty::new(dup_fd)`。`isatty` だけを確認し、`dup_fd` の所有権を取る | `dup_fd` | `isatty` は 4.1 で `ioctl(5, TCGETS2)` として観測 |
| T5 | `inherited_pty.rs:464` → `:57-65`、`close` は `:62` | `drop(adapter)` が `libc::close(self.fd)` を無条件に呼ぶ（分岐なし、戻り値は捨てる） | `dup_fd` が閉じる | 4.1 で `close(5) = 0` |
| T6 | `inherited_pty.rs:472` → `:266-273` | `stat_rdev(dup_fd)` | 5 番を再び `fstat` | 4.1（自然な実行）は `EBADF`。`None` で成功 |
| T7 | `inherited_pty.rs:473-477` | `None` なら成功。`Some(rdev)` なら `assert_ne!(rdev, original_rdev)`（`:474:27`） | | `rdev` が 1282 だと失敗 |

T5 と T6 の間に、drop した番号を再び割り当てる処理はこのスレッドにない。`drop(adapter)` が返ってから `stat_rdev` を
呼ぶまでが、他のスレッドが番号を取りうる区間になる。fd の番号は「最小の空き番号」が渡るため、
番号が取られるのは、割り当ての時点でその番号が最小の空きであるとき、
または `openpty()` のように 1 回の呼び出しが続けて 2 つ取る場合の 2 つ目（slave）に当たるときである（3.3 の最後の行）。

### drop の後に `dup_fd` の番号を得うる記述子と `st_rdev`

3 の C プログラムで測った値（Linux、この環境）。

| 番号を得る記述子 | `st_rdev` | テストの判定 |
|---|---|---|
| 別の `openpty()` で開いた master | 5:2（1282） | 失敗 |
| 別の PTY master の `dup()` / `F_DUPFD_CLOEXEC` | 5:2（1282） | 失敗 |
| `open("/dev/ptmx")` の直接呼び出し | 5:2（1282） | 失敗 |
| 別の PTY の slave（`openpty()` が返す slave、または `/dev/pts/N` を開いたもの） | 136:N | 成功 |
| 通常ファイル、pipe、socket、eventfd、epoll | 0（0:0） | 成功 |
| `/dev/null`、`/dev/zero`、`/dev/urandom` | 1:3、1:5、1:9 | 成功 |
| 誰も取らない | `fstat` が `EBADF`（`None`） | 成功 |

## 3. 実測 1: C プログラム（テストとは独立）

### 3.1 環境とコマンド

Linux（カーネル 7.3.0）、glibc 2.43、gcc 15.2.0、論理 CPU 24、`/dev/pts` は devpts（`ptmxmode=000`）。
スレッドなし・ループなし・負荷なしの小さな C プログラムで、`/tmp` 配下のスクラッチディレクトリ
（以下 `$SCRATCH`）に置いた。ソースは追跡していない。

- ビルド: `gcc -O0 -Wall -Wextra -o "$SCRATCH/ptyfd" "$SCRATCH/ptyfd.c" -lutil`
- 測定対象の PTY は `libc::openpty(&m, &s, NULL, NULL, &winsize 24x80)` で開き、その後に master と slave へ
  `FD_CLOEXEC` を付ける。これは `portable-pty 0.8.1` の `src/unix.rs:18-61`（`openpty` の呼び出しは `:29-39`、
  `FD_CLOEXEC` は `:57-58`）と同じ手順。`strace` で、glibc の `openpty` が `openat("/dev/ptmx")`、`TIOCGPTN`、
  `TIOCSPTLCK`、`TIOCGPTPEER`（slave を得る）、`TIOCSWINSZ` を呼ぶことを確認した（`ptyfd one` を `strace -f` で実行）。
- `$SCRATCH/ptyfd survey` を 1 回、`$SCRATCH/ptyfd reuse` を 2 回（出力は pts 番号と inode 以外同一）、`$SCRATCH/ptyfd ptn` を 1 回、
  `$SCRATCH/ptyfd one` を直接 1 回と `strace -f` 付きで 1 回。

`/dev/ptmx` の `stat` の結果:

```
/dev/ptmx      type=chr  st_rdev=5:2 (1282)  st_dev=0:9   st_ino=90
/dev/pts/ptmx  type=chr  st_rdev=5:2 (1282)  st_dev=0:29  st_ino=2
```

### 3.2 `survey`: master・複製・slave の `st_rdev`

別々に開いた 4 組（pair 0-3、slave は `/dev/pts/23`-`/dev/pts/26`）。fd 番号は途中で閉じるものがあるため一定ではない。

| 記述子 | 4 組での `st_rdev` | `st_dev` / `st_ino` |
|---|---|---|
| master（`openpty`） | すべて 5:2（1282）。fd 3、7、11、15 | すべて 0:9 / 90（`/dev/ptmx` と同じ） |
| master の `dup()`（テストと同じ） | すべて 5:2（1282）。fd 5、9、13、17 | すべて 0:9 / 90 |
| master の `F_DUPFD_CLOEXEC`（`dup_cloexec` と同じ） | すべて 5:2（1282）。fd 6、10、14、18 | すべて 0:9 / 90 |
| slave（`openpty`） | 136:23、136:24、136:25、136:26（数値 34839-34842）。fd 4、8、12、16 | 0:29 / 26-29 |
| slave（`/dev/pts/N` を開き直す、テストの `slave_device_path` と同じ手順） | slave と同じ 136:N | 同上 |
| `open("/dev/ptmx")` を直接 2 回 | 5:2（1282）、5:2（1282） | 0:9 / 90 |

`openpty` の master の `st_rdev` は pair によらず 5:2 で、`/dev/ptmx` の `st_rdev` と等しい。pair ごとに違うのは slave 側の
minor（`TIOCGPTN` の値と一致）と `st_ino` だけ。`st_dev` と `st_ino` も master では pair によらず同じで、`fstat` の結果で master 同士を区別できる欄はない。

PTY 以外の参照値（同じ実行）:

| 記述子 | 種別 | `st_rdev` |
|---|---|---|
| 通常ファイル（`open("/proc/self/exe")`） | reg | 0:0 |
| pipe の両端 | fifo | 0:0 |
| `socketpair`（AF_UNIX） | sock | 0:0 |
| eventfd、epoll | other | 0:0 |
| `/dev/null` / `/dev/zero` / `/dev/urandom` | chr | 1:3（259）/ 1:5（261）/ 1:9（265） |
| `/dev/tty` | 開けない（`ENXIO`、制御端末なし） | - |

### 3.3 `reuse`: 対象テストの手順を、順序を明示して再現

スレッドの競合に頼らない。1 つのスレッドで、対象テストの手順 T1-T6 を行い、T5 と T6 の間に「別のテスト」の役を
明示的に 1 つ入れる。`NOCLOSE` を除き、`close` は T5 と同じく `dup_fd` に 1 回行う。
**この表は C プログラム上の観測であり、対象テストでの観測ではない（対象テストでの観測は 4）。**

| シナリオ（T5 と T6 の間の処理） | 解放された番号を得たもの | T6 の `fstat` | 元の値との比較 | テストの判定 |
|---|---|---|---|---|
| なし | - | `EBADF`（`None`） | - | 成功 |
| `Drop` が close しない（仮定） | 番号は閉じられず `dup_fd` のまま | 5:2 | 等しい（1282 と 1282） | **失敗**（毎回） |
| `openpty()` で別の PTY を開く | 新しい master（fd 5、slave は fd 6） | 5:2 | 等しい | **失敗** |
| 別の PTY master の `dup()` | その複製（fd 7） | 5:2 | 等しい | **失敗** |
| 別の PTY master の `F_DUPFD_CLOEXEC` | その複製（fd 7） | 5:2 | 等しい | **失敗** |
| `open("/dev/ptmx")` | fd 5 | 5:2 | 等しい | **失敗** |
| 別の PTY の slave を `/dev/pts/N` で開く | fd 7 | 136:23 | 異なる（34839 と 1282） | 成功 |
| 通常ファイルを開く | fd 5 | 0:0 | 異なる（0 と 1282） | 成功 |
| pipe を作る | 読み出し側 fd 5 | 0:0 | 異なる | 成功 |
| `socketpair` | fd 5 | 0:0 | 異なる | 成功 |
| `open("/dev/null")` | fd 5 | 1:3 | 異なる（259 と 1282） | 成功 |
| より小さい空き番号があるときの `openpty()`（fd 3 を先に空ける） | master は fd 3、slave が解放された fd 6 を取る | 136:24 | 異なる（34840 と 1282） | 成功 |

読み取れること:

- 解放された番号は、次に fd を割り当てる呼び出しがあればそれに渡った（`なし` と `Drop が close しない` を除くすべての行で確認）。
- 渡された記述子が PTY master かその複製なら、`st_rdev` は元の値と等しくなり、テストは失敗する。
  数値 1282 は、既存計測の失敗メッセージの値と同じである。
- 最後の行のように、`openpty()` が先に取る別の小さい空き番号がある場合は、解放された番号が slave に渡り、成功する。
  失敗するのは、解放された番号が最小の空きで、かつそれを master または複製が取る場合である。
- `Drop` が close しないと仮定した行は毎回失敗する。観測されている間欠的な失敗とは形が違う（6 の (b)）。

### 3.4 `ptn`: master 同士を区別できる識別子（3.3 とは別の測定）

`ioctl(fd, TIOCGPTN)` を、同じ実行内の 2 組（A と B）の記述子へ呼んだ。

| 記述子 | `TIOCGPTN` |
|---|---|
| A の master、その `dup()`、その `F_DUPFD_CLOEXEC` | 22、22、22 |
| B の master | 23 |
| A と B の slave、通常ファイル、`/dev/null`、pipe、socket | 失敗（`ENOTTY`） |
| 閉じた番号（9999） | 失敗（`EBADF`） |

master は pair ごとに異なる値、複製は元と同じ値を返す。この値は、10 の別タスク候補の根拠に使う。

## 4. 実測 2: 対象テストそのもの

テストのソースもバイナリも変更していない。フィルタ付きの単体実行を逐次で 4 回（上限 5 回以内）行った。
`<bin>` は cargo が作ったテストバイナリ `src-tauri/target/debug/deps/emterm-48705c2bd8f909e6` で、cargo と同じく `src-tauri/` から実行した。
`<test>` は `mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor`。

| # | コマンド | 結果 |
|---|---|---|
| 1 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib <test> -- --exact` | 成功 |
| 2 | `strace -f -e trace=openat,fstat,newfstatat,close,dup,fcntl,ioctl <bin> <test> --exact` | 成功 |
| 3 | `PTYFD_INJECT=ptmx LD_PRELOAD="$SCRATCH/inject.so" <bin> <test> --exact` | **失敗**（注入、4.2） |
| 4 | `PTYFD_INJECT=null LD_PRELOAD="$SCRATCH/inject.so" <bin> <test> --exact` | 成功（注入、4.2） |

加えて、プロジェクトのテストコマンド（`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`）を、
ファイルを作る前に 1 回だけ実行した（ベースライン）。4835 件成功、1 件失敗、3 件無視。この対象テストは成功した。
失敗した 1 件は別のテスト `mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`
（`src-tauri/src/mux/ipc/pty_spawn/tests.rs:5145:10`、`Timeout`）で、本調査の対象外であり分析していない。
`--lib` 全体の反復実行と、CPU の取り合いを起こす条件での実行は行っていない。

単体実行の成功（#1、#2、#4）は、間欠失敗がないことを意味しない。

### 4.1 自然な実行の syscall 列（#2）

スレッド番号の列を省き、このテストのスレッドの syscall を順に並べた（テストが始まる前のハーネスの syscall は省略した）。

```
openat(AT_FDCWD, "/dev/ptmx", O_RDWR)        = 3         # portable-pty openpty (glibc)
ioctl(3, TIOCGPTN, [22])                     = 0
ioctl(3, TIOCSPTLCK, [0])                    = 0
ioctl(3, TIOCGPTPEER, 0x102)                 = 4         # slave
ioctl(4, TIOCSWINSZ, {ws_row=24, ws_col=80, ...}) = 0
fcntl(3, F_GETFD)                            = 0         # portable-pty unix.rs:57-58
fcntl(3, F_SETFD, FD_CLOEXEC)                = 0
fcntl(4, F_GETFD)                            = 0
fcntl(4, F_SETFD, FD_CLOEXEC)                = 0
dup(3)                                       = 5         # inherited_pty.rs:210
fstat(5, {st_mode=S_IFCHR|0666, st_rdev=makedev(0x5, 0x2), ...}) = 0   # :460-461
ioctl(5, TCGETS2, {...})                     = 0         # isatty, :40
close(5)                                     = 0         # drop(adapter), :62
fstat(5, ...)                                = -1 EBADF  # :472
close(4)                                     = 0         # _pair の drop
close(3)                                     = 0
```

`close(5)` と次の `fstat(5)` の間に、このスレッドの syscall はない。ほかにテストは走っていない（単体実行）ので、番号 5 は誰にも取られず `EBADF` になった。
テストの開始前に、ハーネスのスレッドは `/sys/fs/cgroup/.../cpu.max` などの通常ファイルを開いて閉じている（fd 3 を使う）。これらの `st_rdev` は 0 なので、
仮に番号を取ってもこのテストは成功する。

### 4.2 外部からの注入（#3、#4）。自然な失敗ではない

`LD_PRELOAD` で読み込む小さな共有ライブラリ（`$SCRATCH/inject.c`、追跡していない）で、競合の代わりに「別のテストが番号を取る」を決まった順序で 1 回だけ起こした。

- `dup(fd)` を包み、`fd` が `st_rdev` 5:2 の記述子なら、返った番号をこのスレッドで記憶する。
- `close(fd)` を包み、本物の `close` が済んだ直後、記憶した番号と一致したら、`$PTYFD_INJECT` が指す記述子を 1 つ開く。

どちらもスレッドは 1 本、CPU 負荷なし、1 回の実行。

| `PTYFD_INJECT` | 注入された `open` の結果 | テストの出力 |
|---|---|---|
| `ptmx`（`open("/dev/ptmx")`） | fd 5（解放された番号） | 失敗: `src-tauri/src/mux/inherited_pty.rs:474:27`、`assertion left != right failed: the owned descriptor must be closed once the adapter drops`、`left: 1282`、`right: 1282` |
| `null`（`open("/dev/null")`） | fd 5（解放された番号） | 成功 |

`ptmx` を注入した失敗は、既存計測（7）の失敗と、場所（`inherited_pty.rs:474:27`）・メッセージ・両辺の値（1282）が一致する。
これは「drop から `fstat` までの間に PTY master が番号を取る」とこの失敗が一致することを示すだけで、
自然な失敗でそれが起きたことの観測ではない。

## 5. コメントの主張と実測の突き合わせ

| 場所 | コメントの主張 | 実測 | 判定 |
|---|---|---|---|
| `inherited_pty.rs:466-468` | `_pair.master` が「このテスト自身の pty device の minor 番号」を開いたままにする | `_pair.master` は開いたまま（fd 3、4.1）。ただし master の `st_rdev` は pair によらず 5:2（3.2）で、minor 番号が pair ごとに違うのは slave（136:N）。master は pair 固有の `st_rdev` を持たない | 前提が成り立たない |
| `inherited_pty.rs:468-471` | 番号が別の記述子に渡っても、それは「DIFFERENT device」を指し「never ours」なので drop の証明になる | 別の PTY master（またはその複製）は 5:2 で元と等しい（3.3、4.2）。別の device になるのは slave、通常ファイル、pipe、socket、`/dev/null` など | 成り立たない（PTY master と複製の場合） |
| `inherited_pty.rs:260-265` | `st_rdev` の比較で、「番号が別の記述子に渡った」場合と「閉じていない」場合を区別できる | 区別できるのは、渡った記述子の `st_rdev` が 1282 以外の場合（3.3 の成功の行）。渡った記述子が master か複製だと区別できず、「閉じていない」場合と同じ結果になる | PTY master と複製の場合に成り立たない |
| `inherited_pty.rs:185-192` | `dup` で、`pair.master` と別の番号で同じ open master を指す記述子を得る | `dup` は別の fd 番号で、`st_rdev` は同じ 5:2（3.2）。ただし `st_rdev` は open file description ではなくデバイスの識別なので、同じ master であることの証明にはならない | 記述は正しいが、比較の材料にならない |

`CLOSED_FD` のコメント（`inherited_pty.rs:285-303`）は、解放された番号を並行テストの `open` が取る、同種の競合がすでに別のテストで起きたことを書いている。
この競合の知識自体はコードにあり、不足しているのは master 同士の区別である。

## 6. 原因候補の採否

| 候補 | 採否 | 根拠 |
|---|---|---|
| (a) 別の PTY master またはその複製による番号の再利用 | **採用** | `inherited_pty.rs:464` から `:472` の間に、解放された番号が最小の空きなら、その間に開かれる fd がその番号を得る（3.3）。PTY master と複製は 5:2（1282）で、失敗メッセージの両辺 1282 と一致する（3.2、7）。同じ失敗を、順序を明示した C プログラム（3.3）と、無変更のテストバイナリへの注入（4.2）で再現した。このスレッドは間に何も開かない（4.1）ので、番号を取るのは別スレッド（同時に走る別テスト）と考えられる。自然な失敗での持ち主は未観測（9） |
| (b) `Drop` で記述子が close されていない | 却下 | `Drop`（`inherited_pty.rs:57-65`）は `libc::close(self.fd)` を無条件に呼び、分岐がない。自然な実行で `close(5) = 0` の後に `fstat(5)` が `EBADF` になった（4.1）。close が起きない場合は 3.3 のとおり毎回失敗するはずだが、既存計測は 2 つの条件とも 25 回中 24 回成功している（7）ので間欠失敗の形と合わない。失敗した実行の `strace` は取れていないので、失敗した実行で close が成功したことそのものは観測していない（`close` の戻り値は捨てられている、`:62`） |
| (c) PTY 以外の記述子による番号の再利用 | 失敗の原因としては却下。ただし番号の再利用自体は起きうる | 通常ファイル、pipe、socket、eventfd、epoll は 0:0、`/dev/null` などは 1:3 から 1:9 で、元の 1282 と異なるので、再利用されてもテストは成功する（3.3、4.2 の `null`）。1282 になる文字デバイスは `/dev/ptmx` と `/dev/pts/ptmx` の 5:2 だけ（3.1）で、これは PTY master を開くことと同じである。失敗時の値 1282 を PTY 以外の記述子では説明できない |

どの候補についても、成功した実行だけを根拠に却下してはいない。(b) の却下は、`Drop` のコードの無条件の close と、
失敗の形（毎回ではなく間欠、値 1282 が PTY master の値と一致）による。

候補 (a)(b)(c) のほかに調べたもの:

| 候補 | 採否 | 根拠 |
|---|---|---|
| (d) このテストのスレッド自身が間に fd を開く | 却下 | `inherited_pty.rs:464` から `:472` の間に開く処理はなく、`strace` でも 2 つの syscall が隣り合う（4.1） |

## 7. 既存計測の引用（今回の計測とは別）

`feature-docs/restored-pane-read-flake-investigation/INVESTIGATION.md` 5.3 の「Other tests that failed in these `--lib` runs」の表が、
この対象テストの失敗を記録している。数字はその調査の計測であり、今回の計測（3、4）には含めていない。

- 条件 (b2): `--lib` 全体を、テストバイナリを引数なしで 25 回実行 -> 1 回失敗。
- 条件 (c1): 同じ全体を、ログ用の `LD_PRELOAD` ライブラリ（v1）を付けて 25 回実行 -> 1 回失敗。
- 失敗の場所は `src-tauri/src/mux/inherited_pty.rs:474:27`、`assertion left != right failed`、両辺 1282。
- 同じ調査は、1282 を「この環境の `/dev/ptmx` のデバイス番号（`stat` は 5:2、5*256+2）」と記録し、
  `inherited_pty.rs:466-471` の前提と値が矛盾すること、原因が未確定であることを書いている。

今回の 3.1 は同じ値（`/dev/ptmx` は 5:2 = 1282）を独立に測り、4.2 は同じ失敗の形を注入で再現した。
その調査の表は 2 つの条件だけを挙げており、同じ調査の別の条件（12 回、30 回）でこの失敗があったかは書かれていない。
この 2 回の失敗がどの順序で並行テストと重なったかの記録はない。

## 8. PTY master またはその複製を開く処理の代表例

`--lib` のテストプロセスの中で、解放された番号を取りうる処理。`portable-pty` と `filedescriptor` のパスは各 crate のルートからの相対。

| 種類 | 場所 | 内容 |
|---|---|---|
| PTY の master と slave を開く | `portable-pty src/unix.rs:18-61`（`libc::openpty` は `:29-39`） | master（fd を 1 つ）と slave（もう 1 つ）を連続して取る |
| reader の複製 | `portable-pty src/unix.rs:314-317` -> `filedescriptor src/lib.rs:219-221` -> `src/unix.rs:144-168`（`F_DUPFD_CLOEXEC` は `:149`） | `try_clone_reader` |
| writer の複製 | `portable-pty src/unix.rs:319-326`（複製は `:324`） | `take_writer` |
| 本リポジトリの複製 | `src-tauri/src/mux/inherited_pty.rs:71-82`（`F_DUPFD_CLOEXEC` は `:74`）、使用側 `:152-158`、`:160-165` | `dup_cloexec` |
| このモジュールのテスト | `inherited_pty.rs:193-213`（`openpty` は `:201-203`、`libc::dup` は `:210`）。呼び出しは `:310`、`:323`、`:346`、`:374`、`:407`、`:459`、`:493` の 7 か所 | `open_master_dup()`。対象テストを含む同じモジュールの 7 つのテストが、同じ形で master を開いて複製する |
| pane のテスト | `src-tauri/src/mux/session/pane/tests.rs:2909-2917`（`open_test_pty_pair`、9 か所から呼ぶ）、直接の `openpty` は `:329`、`:963`、`:1003`、`:1068`、`:1174`、`:1201`、`:1226`、`:1254`、`:1291`、`:1349` ほか。`try_clone_reader` は `:3713` | PTY を開き、`take_writer` で複製する |
| アップグレードのテスト | `src-tauri/src/mux/upgrade/tests.rs` の `openpty` は `:172`、`:236`、`:324`、`:651`、`:685`、`:932`。`libc::dup` は `:271`、`:364`、`:656`、`:687`、`:964`（`:729`、`:765` は通常ファイル） | 同上。複製を `InheritedMasterPty` に渡す |
| そのほかのテスト | `src-tauri/src/mux/daemon/tests.rs:2320`、`src-tauri/src/mux/ipc/pty_spawn/tests.rs:2296`、`src-tauri/src/pty/mod.rs:659-672`（`openpty` と `try_clone_reader`） | 同上 |
| 製品コードを呼ぶテスト | `src-tauri/src/mux/ipc/pty_spawn/mod.rs:165-215`（`spawn_pty`。`openpty` は `:175`、`take_writer` は `:206`、`try_clone_reader` は `:210`）を `pty_spawn/tests.rs:8163`（`:8185`、`:8201`、`:8220`）が呼ぶ（製品側の呼び出しは `src-tauri/src/mux/ipc/handlers/mod.rs:74`）。`src-tauri/src/pty/mod.rs:170-`（`PtySession::spawn`。`openpty` は `:187-189`、`try_clone_reader` は `:252`、`take_writer` は `:255`）を `pty/mod.rs:856` のテストが呼ぶ。復元経路は `src-tauri/src/mux/upgrade.rs:873`（`take_writer`）、`:887`（`try_clone_reader`） | 子プロセス付きで PTY を開く |

同じモジュールの 9 つのテスト（`open_master_dup()` を呼ぶのは対象テストを含む 7 つ）は名前順で隣り合い、ベースラインの出力でも連続して並んだ。
対象テスト以外の 6 つの `open_master_dup()` が、対象テストの drop と `fstat` の間に当たる可能性があるが、それが実際に起きていたかは確かめていない（9）。

通常ファイル、socket などの PTY 以外の fd を開く処理は、`tempfile`（`inherited_pty.rs:430`）、`UnixListener::bind`
（`src-tauri/src/mux/daemon/tests.rs:97`、`src-tauri/src/mux/upgrade/tests.rs:760`、`src-tauri/src/tmux_sockets.rs:615`）などに多数あり、
これらが番号を取ってもこのテストは成功する（3.3）。

## 9. 証拠の限界と未確定の事柄

- **自然な失敗は今回観測していない。** 注入なしの単体実行 2 回と `--lib` 全体 1 回は、このテストについてすべて成功した（注入ありの 2 回は、
  1 回が失敗、1 回が成功で、どちらも自然な実行ではない）。
  結論の根拠は、既存の失敗値（7）、`st_rdev` の測定（3）、決まった順序の再現（3.3）、無変更バイナリへの注入（4.2）である。
- **持ち主の特定は未確定。** 自然な失敗の時点で、解放された番号をどのテストの記述子が持っていたかは分からない。
  失敗メッセージには `st_rdev` の値しか出ず、fd 番号も `TIOCGPTN` も `/proc/self/fd` の指す先も出ない。
  「別のテスト」であることは、1282 の記述子が `/dev/ptmx` 由来であることと、このスレッドが間に何も開かないことからの推論である。
- **頻度は説明できていない。** `drop` から `fstat` までは 2 つの syscall が隣り合う短い区間（4.1）である。
  それでも既存計測で約 25 回に 1 回起きた（7）理由は確かめていない。
  同じモジュールの他の 6 つのテストが名前順で隣り合って開始され、同時に `open_master_dup()` を呼ぶことが理由かもしれない
  が、これは仮説である。確かめるには `--lib` の反復実行か負荷条件が必要で、本タスクではどちらも行わず、未確定として止めた。
- **測定は 1 つの環境のみ。** Linux（カーネル 7.3.0）、glibc 2.43、`/dev/ptmx` が 5:2 の環境だけで測った。
  「すべての PTY master の `st_rdev` が等しい」ことは、他のカーネルや設定では確かめていない。
- **注入と `strace` はタイミングを変える。** 4.1 と 4.2 は syscall の順序と判定を見るためだけに使い、頻度の根拠にはしていない。
- ベースラインで失敗した別のテスト（4）は分析していない。

## 10. 直すべき点の別タスク候補

コードはこのタスクでは変更していない。

1. **drop 後の fd 比較で、PTY master 同士を区別する識別子に変える。**
   - 対象: `src-tauri/src/mux/inherited_pty.rs:257-273`（`stat_rdev`）、`:456-479`（対象テスト）、
     コメント `:260-265` と `:466-471`。`stat_rdev` の呼び出しはこの 2 テスト内の `:461` と `:472` だけ。
   - 問題: 比較に使う `st_rdev` は、すべての PTY master で 5:2 になるため、番号が別の PTY master または複製に渡ると
     「閉じていない」場合と区別できず、間欠的に失敗する。コメントの前提（再利用された記述子は「DIFFERENT device」を指す）が成り立たない。
   - 根拠: 3.2（4 組の master と複製は 5:2、`st_dev` と `st_ino` も同じ）、3.3（再現）、4.2（無変更バイナリへの注入で同じ失敗）、5（コメントの突き合わせ）。
   - 測定済みの識別子の候補: `ioctl(TIOCGPTN)`（3.4）。master と複製は同じ pts 番号、別の pair の master は別の番号、
     slave、通常ファイル、`/dev/null`、pipe、socket、閉じた番号は失敗する。「drop 後に `dup_fd` で `TIOCGPTN` が失敗するか、
     元と異なる番号を返す」で閉じたことを確認できる見込み。ただし Rust のテストでの実装と、負荷下での安定性は未検証。
   - 同じ変更で、失敗メッセージに fd 番号と取得した識別子を出すと、次の失敗で持ち主の手がかりが残る（9）。
2. **ほかに同じ前提のテストはない（別タスク不要）。** `src-tauri/src/mux/upgrade/tests.rs:722-751` は同じ考え方（解放された番号が別の記述子に渡っても
   識別で区別する）で `(st_dev, st_ino)`（`:45-54`）を使うが、対象が `tempfile`（`:725`）の通常ファイルで、別のテストが同じ inode を開くことはなく、
   PTY master の問題は当てはまらない。テストで `st_rdev` を使う箇所はほかにない（`src-tauri/src` を `st_rdev`、`stat_rdev`、`fstat(` で検索した範囲）。
