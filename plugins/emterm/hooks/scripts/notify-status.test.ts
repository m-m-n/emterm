/**
 * Tests for the Claude Code hook script `notify-status.ts` (task0002).
 *
 * Two groups:
 *  - Runtime behavior of `run()` (the injectable-dependency core), one test
 *    group per Acceptance Criterion (AC-3 .. AC-7) plus the `done` allow-list
 *    coverage from Test Notes.
 *  - Static manifest invariants (AC-8) for marketplace.json / plugin.json /
 *    hooks.json — kept in this file per Test Notes ("share a helper... keep
 *    them in the same test file to avoid another new file").
 *
 * All runtime tests use fakes for `spawn` / `openTty` / `which`; none touch
 * a real terminal or require `emterm` on the test machine.
 */

import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { run, type Deps, type SpawnResult, type TtySink } from "./notify-status.ts";

// ---------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------

/** A TtySink fake that records every chunk written to it. */
function fakeTtySink() {
  const writes: Uint8Array[] = [];
  let closed = false;
  const sink: TtySink = {
    write: (bytes) => {
      writes.push(bytes);
    },
    close: () => {
      closed = true;
    },
  };
  return { sink, writes, isClosed: () => closed };
}

/** Builds a fully-fake Deps set, all calls counted, all overridable. */
function fakeDeps(overrides: Partial<Deps> = {}) {
  const whichCalls: string[] = [];
  const spawnCalls: string[][] = [];
  let openTtyCalls = 0;

  const defaultSpawnResult: SpawnResult = {
    stdout: new Uint8Array(),
    exitCode: 0,
  };

  const deps: Deps = {
    which: (command) => {
      whichCalls.push(command);
      return overrides.which
        ? overrides.which(command)
        : "/usr/local/bin/emterm";
    },
    spawn: async (argv) => {
      spawnCalls.push(argv);
      return overrides.spawn ? await overrides.spawn(argv) : defaultSpawnResult;
    },
    openTty: () => {
      openTtyCalls += 1;
      return overrides.openTty ? overrides.openTty() : fakeTtySink().sink;
    },
  };

  return {
    deps,
    whichCalls,
    spawnCalls,
    getOpenTtyCalls: () => openTtyCalls,
  };
}

// ---------------------------------------------------------------------
// AC-3: state argument outside the allow-list
// ---------------------------------------------------------------------

describe("state validation (AC-3)", () => {
  test.each([[""], ["invalid"], []])(
    "argv=%p: exits 0 and never spawns emterm",
    async (...state) => {
      const { deps, spawnCalls } = fakeDeps();
      const exitCode = await run(state, deps);
      expect(exitCode).toBe(0);
      expect(spawnCalls.length).toBe(0);
    },
  );
});

// ---------------------------------------------------------------------
// AC-4: emterm not found on PATH
// ---------------------------------------------------------------------

describe("PATH resolution (AC-4)", () => {
  test("no emterm on injected PATH: exits 0 and never opens /dev/tty", async () => {
    const { deps, spawnCalls, getOpenTtyCalls } = fakeDeps({
      which: () => null,
    });
    const exitCode = await run(["working"], deps);
    expect(exitCode).toBe(0);
    expect(spawnCalls.length).toBe(0);
    expect(getOpenTtyCalls()).toBe(0);
  });
});

// ---------------------------------------------------------------------
// AC-5: /dev/tty open failure
// ---------------------------------------------------------------------

describe("/dev/tty open failure (AC-5)", () => {
  test("openTty throws: exits 0, no unhandled rejection", async () => {
    const { deps } = fakeDeps({
      openTty: () => {
        throw new Error("ENXIO: no such device or address, open '/dev/tty'");
      },
    });
    const exitCode = await run(["working"], deps);
    expect(exitCode).toBe(0);
  });
});

// ---------------------------------------------------------------------
// AC-6: emterm child exits non-zero
// ---------------------------------------------------------------------

describe("emterm non-zero exit (AC-6)", () => {
  test("spawn resolves with exitCode !== 0: script still exits 0", async () => {
    const { deps } = fakeDeps({
      spawn: async () => ({ stdout: new Uint8Array(), exitCode: 1 }),
    });
    const exitCode = await run(["blocked"], deps);
    expect(exitCode).toBe(0);
  });
});

// ---------------------------------------------------------------------
// AC-7: happy path
// ---------------------------------------------------------------------

describe("happy path (AC-7)", () => {
  test("captured stdout bytes are written verbatim to the tty sink", async () => {
    const expectedBytes = new TextEncoder().encode(
      "\x1b]9999;agent-status;working\x07",
    );
    const { sink, writes, isClosed } = fakeTtySink();
    const { deps, spawnCalls } = fakeDeps({
      spawn: async () => ({ stdout: expectedBytes, exitCode: 0 }),
      openTty: () => sink,
    });

    const exitCode = await run(["working"], deps);

    expect(exitCode).toBe(0);
    expect(spawnCalls.length).toBe(1);
    expect(writes.length).toBe(1);
    expect(writes[0]).toEqual(expectedBytes);
    expect(isClosed()).toBe(true);
  });
});

// ---------------------------------------------------------------------
// Test Notes: `done` is accepted by the allow-list (future-proofing)
// ---------------------------------------------------------------------

describe("done state (Test Notes)", () => {
  test("done reaches the spawn call", async () => {
    const { deps, spawnCalls } = fakeDeps();
    const exitCode = await run(["done"], deps);
    expect(exitCode).toBe(0);
    expect(spawnCalls.length).toBe(1);
    expect(spawnCalls[0]).toContain("done");
  });
});

// ---------------------------------------------------------------------
// AC-1: hooks.json declares exactly the three hooks the plan specifies
// ---------------------------------------------------------------------

const REPO_ROOT = join(import.meta.dir, "..", "..", "..", "..");

function readJson(relativePath: string): unknown {
  const raw = readFileSync(join(REPO_ROOT, relativePath), "utf-8");
  return JSON.parse(raw);
}

interface HookEntry {
  type: string;
  command: string;
  timeout: number;
}

interface HooksJson {
  hooks: Record<string, Array<{ hooks: HookEntry[] }>>;
}

describe("hooks.json shape (AC-1)", () => {
  const hooksJson = readJson(
    "plugins/emterm/hooks/hooks.json",
  ) as HooksJson;

  test("declares exactly UserPromptSubmit, Stop, Notification", () => {
    expect(Object.keys(hooksJson.hooks).sort()).toEqual(
      ["Notification", "Stop", "UserPromptSubmit"].sort(),
    );
  });

  test("no SubagentStop entry", () => {
    expect(hooksJson.hooks.SubagentStop).toBeUndefined();
  });

  test.each([
    ["UserPromptSubmit", "working"],
    ["Stop", "idle"],
    ["Notification", "blocked"],
  ])("%s hook: type command, timeout 3, forwards state %s", (event, state) => {
    const entry = hooksJson.hooks[event]?.[0]?.hooks?.[0];
    expect(entry).toBeDefined();
    expect(entry?.type).toBe("command");
    expect(entry?.timeout).toBe(3);
    expect(entry?.command).toBe(
      `\${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts ${state}`,
    );
  });
});

// ---------------------------------------------------------------------
// AC-8: static manifest invariants
// ---------------------------------------------------------------------

describe("static manifest invariants (AC-8)", () => {
  test("marketplace.json, plugin.json, hooks.json all parse as JSON", () => {
    expect(() => readJson(".claude-plugin/marketplace.json")).not.toThrow();
    expect(() =>
      readJson("plugins/emterm/.claude-plugin/plugin.json"),
    ).not.toThrow();
    expect(() => readJson("plugins/emterm/hooks/hooks.json")).not.toThrow();
  });

  test("marketplace <-> plugin name and version match", () => {
    const marketplace = readJson(".claude-plugin/marketplace.json") as {
      plugins: Array<{ name: string; version: string }>;
    };
    const plugin = readJson("plugins/emterm/.claude-plugin/plugin.json") as {
      name: string;
      version: string;
    };
    const entry = marketplace.plugins[0];
    expect(entry).toBeDefined();
    expect(entry?.name).toBe(plugin.name);
    expect(entry?.version).toBe(plugin.version);
  });

  test("hooks.json contains no absolute paths and no ..", () => {
    const raw = readFileSync(
      join(REPO_ROOT, "plugins/emterm/hooks/hooks.json"),
      "utf-8",
    );
    // Every command must be anchored at ${CLAUDE_PLUGIN_ROOT}; forbid any
    // command string starting with "/" and forbid ".." path escapes.
    const commands = (JSON.parse(raw) as HooksJson).hooks;
    for (const entries of Object.values(commands)) {
      for (const entry of entries) {
        for (const hook of entry.hooks) {
          expect(hook.command.startsWith("${CLAUDE_PLUGIN_ROOT}")).toBe(true);
        }
      }
    }
    expect(raw.includes("..")).toBe(false);
  });
});
