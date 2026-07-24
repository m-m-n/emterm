#!/usr/bin/env bun
/**
 * Claude Code hook script: forwards Claude Code lifecycle state to
 * `emterm agent-status` via /dev/tty.
 *
 * See feature-docs/emterm-claude-plugin/SPEC.md FR3/FR4. Failure semantics
 * are uniform: every failure path results in exit 0 so Claude Code is never
 * blocked or shown a hook error.
 */

import { closeSync, openSync, writeSync } from "node:fs";

/** States the hook script accepts. `done` is accepted by the allow-list
 * but not currently emitted by any hook in hooks.json (reserved so a
 * future hook wiring can pass it through without regression). */
export const ALLOWED_STATES = ["idle", "working", "blocked", "done"] as const;
export type State = (typeof ALLOWED_STATES)[number];

export function isAllowedState(value: string): value is State {
  return (ALLOWED_STATES as readonly string[]).includes(value);
}

/**
 * Internal deadline for the whole spawn+read step, enforced independently
 * of Claude Code's own 3 s hook timeout (SPEC.md FR4). 1 s of slack is left
 * so this script always wins the race and can return exit 0 on its own
 * terms instead of being killed non-zero by Claude Code.
 */
export const INTERNAL_TIMEOUT_MS = 2000;

/**
 * Grace period between SIGTERM and SIGKILL when the internal deadline
 * fires and the spawned child must be torn down.
 */
const KILL_GRACE_MS = 250;

/** Result of spawning the `emterm agent-status` child process. */
export interface SpawnResult {
  stdout: Uint8Array;
  exitCode: number;
}

/** A writable sink for /dev/tty (or a fake, in tests). */
export interface TtySink {
  write(bytes: Uint8Array): void;
  close(): void;
}

/**
 * Injectable dependencies so the core logic (`run`) never touches a real
 * terminal and never requires `emterm` to be installed on the machine
 * running the tests.
 *
 * `spawn` receives an `AbortSignal` that `run()` aborts once
 * `INTERNAL_TIMEOUT_MS` elapses; real implementations use it to terminate
 * the child process. Fakes may ignore it.
 */
export interface Deps {
  which: (command: string) => string | null;
  spawn: (argv: string[], signal: AbortSignal) => Promise<SpawnResult>;
  openTty: () => TtySink;
}

type SpawnOutcome =
  | { timedOut: true }
  | { timedOut: false; result: SpawnResult };

/**
 * Resolves `{ timedOut: true }` after `ms` and aborts `controller` so the
 * in-flight `spawn` call can tear down its child. The timer is cancelled by
 * the caller via the returned `cancel()` once the race has a winner, so a
 * fast-resolving `spawn` never leaves a dangling 2 s timer behind.
 */
function armInternalTimeout(
  ms: number,
  controller: AbortController,
): { promise: Promise<SpawnOutcome>; cancel: () => void } {
  let timeoutId: ReturnType<typeof setTimeout>;
  const promise = new Promise<SpawnOutcome>((resolve) => {
    timeoutId = setTimeout(() => {
      controller.abort();
      resolve({ timedOut: true });
    }, ms);
  });
  return { promise, cancel: () => clearTimeout(timeoutId) };
}

/**
 * Core hook behavior. Always resolves to 0 by design (SPEC.md FR4: every
 * failure path — bad state, wrong argv cardinality, missing `emterm`,
 * internal-timeout expiry, /dev/tty open failure, non-zero child exit,
 * thrown exception — results in exit 0). The single top-level try/catch is
 * deliberate: it is the one place that guarantees no exception from an
 * injected dependency ever escapes this function.
 */
export async function run(argv: string[], deps: Deps): Promise<number> {
  try {
    // SPEC.md FR4: exactly one positional argument. Checked before the
    // allow-list so extra/missing args are rejected uniformly.
    if (argv.length !== 1) {
      return 0;
    }

    const state = argv[0];
    if (state === undefined || !isAllowedState(state)) {
      return 0;
    }

    const emtermPath = deps.which("emterm");
    if (!emtermPath) {
      return 0;
    }

    const controller = new AbortController();
    const { promise: timeoutPromise, cancel: cancelTimeout } =
      armInternalTimeout(INTERNAL_TIMEOUT_MS, controller);

    const spawnPromise: Promise<SpawnOutcome> = deps
      .spawn(
        [emtermPath, "agent-status", state, "--name", "claude-code"],
        controller.signal,
      )
      .then((result) => ({ timedOut: false, result }) as const);

    let outcome: SpawnOutcome;
    try {
      outcome = await Promise.race([spawnPromise, timeoutPromise]);
    } finally {
      cancelTimeout();
    }

    if (outcome.timedOut) {
      // Deadline hit: the child was signaled via `controller.abort()`
      // inside `armInternalTimeout`. No tty is opened, nothing is written.
      return 0;
    }

    const { result } = outcome;
    if (result.exitCode === 0) {
      const sink = deps.openTty();
      try {
        sink.write(result.stdout);
      } finally {
        sink.close();
      }
    }

    return 0;
  } catch {
    return 0;
  }
}

/** Real dependencies: Bun.which + Bun.spawn + a real /dev/tty file descriptor. */
function realDeps(): Deps {
  return {
    which: (command) => Bun.which(command),
    spawn: async (argv, signal) => {
      const proc = Bun.spawn(argv, { stdout: "pipe", stderr: "ignore" });

      const onAbort = () => {
        proc.kill(); // SIGTERM
        const killTimer = setTimeout(() => {
          if (!proc.killed) {
            proc.kill("SIGKILL");
          }
        }, KILL_GRACE_MS);
        // Don't let the grace-period timer keep the process/tests alive
        // once the child has actually exited.
        void proc.exited.finally(() => clearTimeout(killTimer));
      };
      signal.addEventListener("abort", onAbort, { once: true });

      try {
        const stdout = new Uint8Array(
          await new Response(proc.stdout).arrayBuffer(),
        );
        const exitCode = await proc.exited;
        return { stdout, exitCode };
      } finally {
        signal.removeEventListener("abort", onAbort);
      }
    },
    openTty: () => {
      const fd = openSync("/dev/tty", "w");
      return {
        write: (bytes) => {
          writeSync(fd, bytes);
        },
        close: () => {
          closeSync(fd);
        },
      };
    },
  };
}

if (import.meta.main) {
  const exitCode = await run(Bun.argv.slice(2), realDeps());
  process.exit(exitCode);
}
