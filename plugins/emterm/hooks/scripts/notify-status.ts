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
 */
export interface Deps {
  which: (command: string) => string | null;
  spawn: (argv: string[]) => Promise<SpawnResult>;
  openTty: () => TtySink;
}

/**
 * Core hook behavior. Always resolves to 0 by design (SPEC.md FR4: every
 * failure path — bad state, missing `emterm`, /dev/tty open failure,
 * non-zero child exit, thrown exception — results in exit 0). The single
 * top-level try/catch is deliberate: it is the one place that guarantees
 * no exception from an injected dependency ever escapes this function.
 */
export async function run(argv: string[], deps: Deps): Promise<number> {
  try {
    const state = argv[0];
    if (state === undefined || !isAllowedState(state)) {
      return 0;
    }

    const emtermPath = deps.which("emterm");
    if (!emtermPath) {
      return 0;
    }

    const result = await deps.spawn([
      emtermPath,
      "agent-status",
      state,
      "--name",
      "claude-code",
    ]);

    const sink = deps.openTty();
    try {
      sink.write(result.stdout);
    } finally {
      sink.close();
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
    spawn: async (argv) => {
      const proc = Bun.spawn(argv, { stdout: "pipe", stderr: "ignore" });
      const stdout = new Uint8Array(
        await new Response(proc.stdout).arrayBuffer(),
      );
      const exitCode = await proc.exited;
      return { stdout, exitCode };
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
