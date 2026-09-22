/**
 * Regression test for scripts/verify-font-bootstrap-verdict.sh (task0001,
 * verify-font-bootstrap-classification).
 *
 * `classify_unfetched_verdict` and `count_executed_tests` are exercised as
 * real subprocesses (`bash -c 'source <helper>; <call>'`), driving the
 * shipped helper file itself against synthetic captured logs — never a
 * re-implementation of the logic in TypeScript. This follows the
 * subprocess-driven precedent already in this repository
 * (`plugins/emterm/hooks/scripts/notify-status.test.ts`), per
 * IMPLEMENTATION.md D7. No cargo is invoked and no network is reached
 * anywhere in this file (NFR1) — that is what lets this test run inside the
 * bun CI job, which installs no Rust toolchain.
 *
 * The AC-8/AC-9/AC-13 script-level assertions and the AC-11/AC-14
 * static assertions read scripts/verify-font-bootstrap.sh as text rather
 * than executing it (a real end-to-end run needs cargo, network, and
 * several minutes — VERIFICATION.md's manual section, not this task).
 *
 * See feature-docs/verify-font-bootstrap-classification/{SPEC,IMPLEMENTATION,tasks/task0001}.md.
 */

import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** True when this process runs as root, under which chmod 000 does not
 * block reads — the permission-based unreadable-log cases below cannot be
 * constructed in that environment and must be skipped rather than
 * weakened (task0002 test notes). */
function isRoot(): boolean {
  return typeof process.getuid === "function" && process.getuid() === 0;
}

const SCRIPT_DIR = import.meta.dir;
const REPO_ROOT = join(SCRIPT_DIR, "..");
const HELPER_PATH = join(SCRIPT_DIR, "verify-font-bootstrap-verdict.sh");
const MAIN_SCRIPT_PATH = join(SCRIPT_DIR, "verify-font-bootstrap.sh");
const CI_YML_PATH = join(REPO_ROOT, ".github", "workflows", "ci.yml");
const PRIOR_SPEC_PATH = join(
  REPO_ROOT,
  "feature-docs",
  "worktree-font-bootstrap",
  "SPEC.md",
);

/** Creates a fresh temp dir holding one log file with the given content,
 * returning its path. Each call gets its own directory so tests never share
 * mutable state. */
function writeLog(
  content: string,
  fileName = "captured.log",
): {
  dir: string;
  path: string;
} {
  const dir = mkdtempSync(join(tmpdir(), "verify-font-bootstrap-verdict-"));
  const path = join(dir, fileName);
  writeFileSync(path, content);
  return { dir, path };
}

function cleanup(dir: string) {
  rmSync(dir, { recursive: true, force: true });
}

/** Runs the shipped helper's classify_unfetched_verdict as a real
 * subprocess, sourcing the file itself (never a copy). `status` accepts a
 * string too so task0002's non-integer-status cases can be driven without a
 * cast at every call site. */
function runClassify(
  status: number | string,
  logPath: string,
): {
  outcome: string;
  message: string;
  stderr: string;
  exitCode: number;
  rawStdout: string;
} {
  const result = Bun.spawnSync([
    "bash",
    "-c",
    'source "$1"; classify_unfetched_verdict "$2" "$3"',
    "_",
    HELPER_PATH,
    String(status),
    logPath,
  ]);
  const rawStdout = result.stdout.toString();
  const line = rawStdout.replace(/\n$/, "");
  const spaceIndex = line.indexOf(" ");
  const outcome = spaceIndex === -1 ? line : line.slice(0, spaceIndex);
  const message = spaceIndex === -1 ? "" : line.slice(spaceIndex + 1);
  return {
    outcome,
    message,
    stderr: result.stderr.toString(),
    exitCode: result.exitCode,
    rawStdout,
  };
}

/** Runs the shipped helper's count_executed_tests as a real subprocess. */
function runCount(logPath: string): { stdout: string; exitCode: number } {
  const result = Bun.spawnSync([
    "bash",
    "-c",
    'source "$1"; count_executed_tests "$2"',
    "_",
    HELPER_PATH,
    logPath,
  ]);
  return {
    stdout: result.stdout.toString().trim(),
    exitCode: result.exitCode,
  };
}

/** Runs the shipped helper's count_executed_tests as a real subprocess,
 * with `logPath` resolved relative to `cwd` — used to address a log by a
 * relative name beginning with `-` or containing `=`, so the argument
 * genuinely starts with that character rather than being buried after a
 * directory prefix (task0002 AC-8). */
function runCountAt(
  logPath: string,
  cwd: string,
): { stdout: string; exitCode: number } {
  const result = Bun.spawnSync(
    [
      "bash",
      "-c",
      'source "$1"; count_executed_tests "$2"',
      "_",
      HELPER_PATH,
      logPath,
    ],
    { cwd },
  );
  return {
    stdout: result.stdout.toString().trim(),
    exitCode: result.exitCode,
  };
}

const CARGO_FAILURE_STATUS = 101; // cargo test's own failure exit status
const CARGO_BUILD_ERROR_LOG =
  "error[E0433]: failed to resolve: use of undeclared crate or module\n" +
  " --> src-tauri/src/lib.rs:1:1\nerror: could not compile `emterm` (lib)\n";

describe("classify_unfetched_verdict: warn outcome (AC-1, AC-2)", () => {
  test("a non-zero failed count with a non-zero exit status yields warn, message states the exit status and the derived count", () => {
    const { dir, path } = writeLog(
      "test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s\n",
    );
    try {
      const { outcome, message } = runClassify(CARGO_FAILURE_STATUS, path);
      expect(outcome).toBe("warn");
      expect(message).toContain(String(CARGO_FAILURE_STATUS));
      expect(message).toContain("5"); // 3 passed + 2 failed
    } finally {
      cleanup(dir);
    }
  });

  test("the warn message never contains the build-stop wording", () => {
    const { dir, path } = writeLog(
      "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    try {
      const { outcome, message } = runClassify(CARGO_FAILURE_STATUS, path);
      expect(outcome).toBe("warn");
      expect(message).not.toContain("stopped the build");
    } finally {
      cleanup(dir);
    }
  });
});

describe("classify_unfetched_verdict: fail outcome on a build stop (AC-3)", () => {
  test("no test result line with a non-zero exit status yields fail, message names a build stop and states the exit status", () => {
    const { dir, path } = writeLog(CARGO_BUILD_ERROR_LOG);
    try {
      const { outcome, message } = runClassify(CARGO_FAILURE_STATUS, path);
      expect(outcome).toBe("fail");
      expect(message).toContain("stopped the build");
      expect(message).toContain(String(CARGO_FAILURE_STATUS));
    } finally {
      cleanup(dir);
    }
  });
});

describe("classify_unfetched_verdict: no hardcoded counts (AC-4)", () => {
  test("two logs with different executed counts produce two warn messages stating those two different counts", () => {
    const logA = writeLog(
      "test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n",
    );
    const logB = writeLog(
      "test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n" +
        "test result: FAILED. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s (doctests)\n",
    );
    try {
      const a = runClassify(CARGO_FAILURE_STATUS, logA.path);
      const b = runClassify(CARGO_FAILURE_STATUS, logB.path);
      expect(a.outcome).toBe("warn");
      expect(b.outcome).toBe("warn");
      expect(a.message).toContain("5");
      expect(b.message).toContain("12");
      expect(a.message).not.toBe(b.message);
    } finally {
      cleanup(logA.dir);
      cleanup(logB.dir);
    }
  });
});

describe("classify_unfetched_verdict: fail outcome with unchanged wording for a clean-exit stall (AC-5)", () => {
  test("no test result line with a zero exit status yields fail with the unchanged literal message", () => {
    const { dir, path } = writeLog(
      "build_rs.font_missing: bundled font missing\n",
    );
    try {
      const { outcome, message } = runClassify(0, path);
      expect(outcome).toBe("fail");
      expect(message).toBe("command exited 0 but 0 tests executed");
    } finally {
      cleanup(dir);
    }
  });
});

describe("classify_unfetched_verdict: pass outcome (AC-6)", () => {
  test("a non-zero passed count with a zero exit status yields pass with an empty message", () => {
    const { dir, path } = writeLog(
      "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s\n",
    );
    try {
      const { outcome, message } = runClassify(0, path);
      expect(outcome).toBe("pass");
      expect(message).toBe("");
    } finally {
      cleanup(dir);
    }
  });
});

describe("classify_unfetched_verdict: branch selected by count before status (AC-7)", () => {
  test("the same non-zero exit status yields different outcomes solely because the executed count differs", () => {
    const zeroExecuted = writeLog(CARGO_BUILD_ERROR_LOG);
    const nonZeroExecuted = writeLog(
      "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    try {
      const zero = runClassify(CARGO_FAILURE_STATUS, zeroExecuted.path);
      const nonZero = runClassify(CARGO_FAILURE_STATUS, nonZeroExecuted.path);
      expect(zero.outcome).toBe("fail");
      expect(nonZero.outcome).toBe("warn");
    } finally {
      cleanup(zeroExecuted.dir);
      cleanup(nonZeroExecuted.dir);
    }
  });
});

describe("classify_unfetched_verdict: edge cases", () => {
  test("a signal-derived, non-cargo exit status with a non-zero executed count still yields warn, and the message stays neutral about the cause", () => {
    const { dir, path } = writeLog(
      "test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s\n",
    );
    try {
      // 130 = a shell's conventional SIGINT-derived status, not cargo's own
      // test-failure status (101) — the classification must not assume the
      // exit status came from cargo specifically.
      const { outcome, message } = runClassify(130, path);
      expect(outcome).toBe("warn");
      expect(message).toContain("130");
      expect(message).toContain("5");
      expect(message).not.toContain("stopped the build");
    } finally {
      cleanup(dir);
    }
  });

  test("a suite reporting zero passed and zero failed (every test ignored) still selects the zero-executed branch", () => {
    const { dir, path } = writeLog(
      "test result: ok. 0 passed; 0 failed; 12 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    try {
      const { outcome, message } = runClassify(CARGO_FAILURE_STATUS, path);
      expect(outcome).toBe("fail");
      expect(message).toContain("stopped the build");
    } finally {
      cleanup(dir);
    }
  });

  test("a log path holding a space is handled without splitting", () => {
    const { dir, path } = writeLog(
      "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
      "captured log with spaces.log",
    );
    try {
      const { outcome, message } = runClassify(0, path);
      expect(outcome).toBe("pass");
      expect(message).toBe("");
    } finally {
      cleanup(dir);
    }
  });
});

/*
 * task0002 (verify-font-bootstrap-classification): fail closed when the
 * executed-test count cannot be derived. AC numbers below refer to
 * feature-docs/verify-font-bootstrap-classification/tasks/task0002.md, a
 * separate acceptance list from task0001's above (both feed the same
 * IMPLEMENTATION.md).
 */

describe("classify_unfetched_verdict: fails closed when the log cannot be read (task0002 AC-1, AC-2, AC-3, AC-4)", () => {
  test("a missing log path with a non-zero exit status yields fail, naming the log path and stating no test count (AC-1, AC-2)", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-missing-"),
    );
    const missingPath = join(dir, "missing.log");
    try {
      const { outcome, message } = runClassify(
        CARGO_FAILURE_STATUS,
        missingPath,
      );
      expect(outcome).toBe("fail");
      expect(message).toContain(missingPath);
      expect(message).not.toContain("stopped the build");
      expect(message).not.toContain(
        "tests executed and the run reported a failure",
      );
      expect(message).not.toContain("tests executed"); // no count field at all, empty or otherwise
    } finally {
      cleanup(dir);
    }
  });

  test("a missing log path with a zero exit status still yields fail, never pass (AC-3)", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-missing-zero-"),
    );
    const missingPath = join(dir, "missing.log");
    try {
      const { outcome } = runClassify(0, missingPath);
      expect(outcome).toBe("fail");
    } finally {
      cleanup(dir);
    }
  });

  test("an unreadable existing log behaves identically to a missing one, for a non-zero exit status (AC-4)", () => {
    if (isRoot()) return; // root bypasses permission bits; case cannot be constructed
    const { dir, path } = writeLog(
      "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    chmodSync(path, 0o000);
    try {
      const { outcome, message } = runClassify(CARGO_FAILURE_STATUS, path);
      expect(outcome).toBe("fail");
      expect(message).toContain(path);
    } finally {
      chmodSync(path, 0o644);
      cleanup(dir);
    }
  });

  test("an unreadable existing log behaves identically to a missing one, for a zero exit status (AC-4)", () => {
    if (isRoot()) return;
    const { dir, path } = writeLog(
      "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    chmodSync(path, 0o000);
    try {
      const { outcome } = runClassify(0, path);
      expect(outcome).toBe("fail");
    } finally {
      chmodSync(path, 0o644);
      cleanup(dir);
    }
  });
});

describe("classify_unfetched_verdict: fails closed when the status argument is not an integer (task0002 AC-5)", () => {
  test("a non-numeric status yields fail naming the uninterpretable status, never pass or warn", () => {
    const { dir, path } = writeLog(
      "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    try {
      const { outcome, message } = runClassify("abc", path);
      expect(outcome).toBe("fail");
      expect(message).toContain("abc");
    } finally {
      cleanup(dir);
    }
  });

  test("a non-integer (fractional) status yields fail naming the uninterpretable status, never pass or warn", () => {
    const { dir, path } = writeLog(
      "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    try {
      const { outcome, message } = runClassify("12.5", path);
      expect(outcome).toBe("fail");
      expect(message).toContain("12.5");
    } finally {
      cleanup(dir);
    }
  });
});

describe("classify_unfetched_verdict: exactly one line, success exit, across every outcome including the new fail-closed ones (task0002 AC-10)", () => {
  test("the fail-closed branch on an unreadable log still returns success and prints exactly one line", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-oneline-"),
    );
    const missingPath = join(dir, "missing.log");
    try {
      const { exitCode, rawStdout, outcome } = runClassify(
        CARGO_FAILURE_STATUS,
        missingPath,
      );
      expect(exitCode).toBe(0);
      expect(rawStdout.split("\n").filter((l) => l.length > 0).length).toBe(1);
      expect(outcome).toBe("fail");
    } finally {
      cleanup(dir);
    }
  });

  test("the fail-closed branch on a non-integer status still returns success and prints exactly one line", () => {
    const { dir, path } = writeLog(
      "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    try {
      const { exitCode, rawStdout, outcome } = runClassify("abc", path);
      expect(exitCode).toBe(0);
      expect(rawStdout.split("\n").filter((l) => l.length > 0).length).toBe(1);
      expect(outcome).toBe("fail");
    } finally {
      cleanup(dir);
    }
  });

  test("the pre-existing pass branch still returns success and prints exactly one line", () => {
    const { dir, path } = writeLog(
      "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s\n",
    );
    try {
      const { exitCode, rawStdout, outcome } = runClassify(0, path);
      expect(exitCode).toBe(0);
      expect(rawStdout.split("\n").filter((l) => l.length > 0).length).toBe(1);
      expect(outcome).toBe("pass");
    } finally {
      cleanup(dir);
    }
  });
});

describe("count_executed_tests: unchanged relocation behaviour (AC-10)", () => {
  test("several test result lines are summed (a library suite plus doctests)", () => {
    const { dir, path } = writeLog(
      "test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n" +
        "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s (doctests)\n",
    );
    try {
      const { stdout } = runCount(path);
      expect(stdout).toBe("16");
    } finally {
      cleanup(dir);
    }
  });

  test("a log with no test result line yields zero", () => {
    const { dir, path } = writeLog(CARGO_BUILD_ERROR_LOG);
    try {
      const { stdout } = runCount(path);
      expect(stdout).toBe("0");
    } finally {
      cleanup(dir);
    }
  });

  test("a suite reporting zero passed and zero failed yields zero", () => {
    const { dir, path } = writeLog(
      "test result: ok. 0 passed; 0 failed; 12 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    try {
      const { stdout } = runCount(path);
      expect(stdout).toBe("0");
    } finally {
      cleanup(dir);
    }
  });
});

describe("count_executed_tests: reports failure and prints no count when the log cannot be read (task0002 AC-6)", () => {
  test("a missing log path reports failure and prints no count", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-count-missing-"),
    );
    const missingPath = join(dir, "missing.log");
    try {
      const { stdout, exitCode } = runCount(missingPath);
      expect(exitCode).not.toBe(0);
      expect(stdout).toBe("");
    } finally {
      cleanup(dir);
    }
  });

  test("an unreadable existing log reports failure and prints no count", () => {
    if (isRoot()) return; // root bypasses permission bits; case cannot be constructed
    const { dir, path } = writeLog(
      "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
    );
    chmodSync(path, 0o000);
    try {
      const { stdout, exitCode } = runCount(path);
      expect(exitCode).not.toBe(0);
      expect(stdout).toBe("");
    } finally {
      chmodSync(path, 0o644);
      cleanup(dir);
    }
  });

  test("a readable log still reports success and prints exactly the integer it prints today", () => {
    const { dir, path } = writeLog(
      "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s\n",
    );
    try {
      const { stdout, exitCode } = runCount(path);
      expect(exitCode).toBe(0);
      expect(stdout).toBe("7");
    } finally {
      cleanup(dir);
    }
  });
});

describe("count_executed_tests: an unusual log path is delivered via standard input, never as a positional argument (task0002 AC-8)", () => {
  test("a relative path beginning with '-' derives the same count as identical content at an ordinary path", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-dash-"),
    );
    const content =
      "test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s\n";
    writeFileSync(join(dir, "-suspicious.log"), content);
    writeFileSync(join(dir, "ordinary.log"), content);
    try {
      const dash = runCountAt("-suspicious.log", dir);
      const ordinary = runCountAt("ordinary.log", dir);
      expect(dash.exitCode).toBe(0);
      expect(dash.stdout).toBe(ordinary.stdout);
      expect(dash.stdout).toBe("9");
    } finally {
      cleanup(dir);
    }
  });

  test("a relative path containing '=' derives the same count as identical content at an ordinary path", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-eq-"),
    );
    const content =
      "test result: ok. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s\n";
    writeFileSync(join(dir, "var=value.log"), content);
    writeFileSync(join(dir, "ordinary.log"), content);
    try {
      const eq = runCountAt("var=value.log", dir);
      const ordinary = runCountAt("ordinary.log", dir);
      expect(eq.exitCode).toBe(0);
      expect(eq.stdout).toBe(ordinary.stdout);
      expect(eq.stdout).toBe("5");
    } finally {
      cleanup(dir);
    }
  });
});

describe("Loading the helper is side-effect free (AC-17)", () => {
  test("sourcing the helper produces no stdout/stderr output and exits 0", () => {
    const result = Bun.spawnSync([
      "bash",
      "-c",
      'source "$1"',
      "_",
      HELPER_PATH,
    ]);
    expect(result.stdout.toString()).toBe("");
    expect(result.stderr.toString()).toBe("");
    expect(result.exitCode).toBe(0);
  });

  test("sourcing the helper creates no file or directory in the working directory", () => {
    const dir = mkdtempSync(
      join(tmpdir(), "verify-font-bootstrap-verdict-cwd-"),
    );
    try {
      const result = Bun.spawnSync(
        ["bash", "-c", 'source "$1"', "_", HELPER_PATH],
        {
          cwd: dir,
        },
      );
      expect(result.exitCode).toBe(0); // the source itself must have succeeded
      expect(readdirSync(dir)).toEqual([]);
    } finally {
      cleanup(dir);
    }
  });

  test("sourcing the helper installs no trap (no exit-time hook)", () => {
    const result = Bun.spawnSync([
      "bash",
      "-c",
      'source "$1" && echo "sourced-ok" && trap -p',
      "_",
      HELPER_PATH,
    ]);
    const stdout = result.stdout.toString();
    expect(stdout).toContain("sourced-ok"); // the source itself must have succeeded
    expect(stdout.replace("sourced-ok\n", "")).toBe("");
  });
});

describe("verify-font-bootstrap.sh: the un-fetched cargo invocation is the literal reproduction command (AC-11)", () => {
  test("the exact command string is present, with no added cargo flag", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).toContain(
      "CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib",
    );
  });

  test("no test-thread-count flag appears anywhere in the script", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).not.toContain("--test-threads");
  });
});

describe("verify-font-bootstrap.sh: warn routes through the script's own reporting and tally (AC-8, AC-9)", () => {
  test("a report_warn helper exists, logs through the script's log() helper, and counts toward the passed tally via report_pass", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    const match = script.match(/report_warn\(\)\s*\{[\s\S]*?\n\}/);
    expect(match).not.toBeNull();
    const body = match![0];
    expect(body).toContain('log "$name: WARN');
    expect(body).toContain("report_pass");
  });

  test("report_warn emits the captured log's tail on stderr, the same diagnostic a failing scenario produces", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    const match = script.match(/report_warn\(\)\s*\{[\s\S]*?\n\}/);
    const body = match![0];
    expect(body).toContain("tail_log");
    expect(body).toContain(">&2");
  });

  test("the un-fetched scenario's verdict block dispatches the helper's warn outcome to report_warn", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    const match = script.match(/scenario_unfetched\(\)\s*\{[\s\S]*?\n\}\n/);
    expect(match).not.toBeNull();
    const body = match![0];
    expect(body).toContain("classify_unfetched_verdict");
    expect(body).toMatch(/warn\)\s*\n\s*report_warn/);
    expect(body).toMatch(/fail\)\s*\n\s*report_fail/);
    expect(body).toMatch(/pass\)\s*\n\s*report_pass/);
  });
});

describe("verify-font-bootstrap.sh: scenario independence is preserved (AC-13)", () => {
  test("the script still sets -uo pipefail and never enables -e", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).toContain("set -uo pipefail");
    expect(script).not.toMatch(/^\s*set\s+-e\b/m);
    expect(script).not.toMatch(/^\s*set\s+-\w*e\w*\s/m);
  });
});

describe("verify-font-bootstrap.sh: the untouched parts of the script (AC-14)", () => {
  test("the fetch-failure scenario's actionable-message assertions are unchanged", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).toContain("build_rs.font_missing: bundled font missing at");
    expect(script).toContain(
      "Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts.",
    );
  });

  test("the already-fetched scenario's acquisition-path recheck is unchanged", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).toContain(
      "acquisition path reported a download after the measured build",
    );
  });

  test("the exit-time cleanup trap and the scenario invocation order are unchanged", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).toContain("trap cleanup EXIT");
    const order = [
      script.indexOf("\nscenario_unfetched\n"),
      script.indexOf("\nscenario_fetch_failure\n"),
      script.indexOf("\nscenario_already_fetched\n"),
    ];
    expect(order.every((i) => i >= 0)).toBe(true);
    expect(order[0]).toBeLessThan(order[1] as number);
    expect(order[1]).toBeLessThan(order[2] as number);
  });

  test("the summary line's shape is unchanged", () => {
    const script = readFileSync(MAIN_SCRIPT_PATH, "utf-8");
    expect(script).toContain(
      'log "summary: $RESULTS_PASS/$RESULTS_TOTAL scenarios passed"',
    );
  });

  test("the prior feature's SPEC.md is byte-for-byte unmodified", () => {
    const raw = readFileSync(PRIOR_SPEC_PATH, "utf-8");
    const hash = createHash("sha256").update(raw).digest("hex");

    // Captured from feature-docs/worktree-font-bootstrap/SPEC.md as it stood
    // before this task (D5: this task must never touch it).
    expect(hash).toBe(
      "934a980ee165f63aff486fcc470eb150ed9251814a4653451b97dc43040912df",
    );
  });

  test("the CI verification step still has no cache step and no opt-out env var reference near it", () => {
    const raw = readFileSync(CI_YML_PATH, "utf-8");
    expect(raw).not.toContain("EMTERM_SKIP_FONT_FETCH");
    expect(raw).not.toContain("fetch-fonts.sh");
  });
});
