/**
 * Tests for the Claude Code hook script `notify-status.sh` and the
 * `hooks.json` manifest that wires it up (task0001).
 *
 * `notify-status.sh` is exercised as a real subprocess, invoked explicitly
 * via `sh <script> <args…>`, so the assertions cover POSIX-shell semantics
 * rather than the developer's login shell (IMPLEMENTATION.md D3, Test
 * Notes). No fakes: this is the shipped artifact itself.
 *
 * See feature-docs/emterm-plugin-runtime-fixes/{SPEC,tasks/task0001}.md.
 */

import { describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const REPO_ROOT = join(import.meta.dir, "..", "..", "..", "..");
const SCRIPT_PATH = join(import.meta.dir, "notify-status.sh");
const HOOKS_JSON_PATH = join(REPO_ROOT, "plugins/emterm/hooks/hooks.json");

/** Runs `sh <script> <args>`, returning stdout, stderr, and exit code. */
function runScript(
  args: string[],
  cwd?: string,
): { stdout: string; stderr: string; exitCode: number } {
  const result = Bun.spawnSync(["sh", SCRIPT_PATH, ...args], {
    stdout: "pipe",
    stderr: "pipe",
    cwd,
  });
  return {
    stdout: result.stdout.toString(),
    stderr: result.stderr.toString(),
    exitCode: result.exitCode,
  };
}

/**
 * The canonical wire-format sequence (SPEC.md FR3), byte-identical to what
 * `crate::agent_status::build` in `src-tauri/src/agent_status.rs` emits for
 * name "claude-code" (TS-9's cross-check against the Rust canonical
 * builder; the literal below is pinned by SPEC.md, not re-derived from
 * source at test time, since this task never touches `src-tauri/`).
 */
function canonicalSequence(state: string): string {
  return `\x1b]777;emterm;agent-status;v=1;state=${state};name=claude-code\x1b\\`;
}

const ALLOWED_STATES = ["idle", "working", "blocked", "done"] as const;

// ---------------------------------------------------------------------
// AC-1: executable bit + shebang
// ---------------------------------------------------------------------

describe("script file (AC-1)", () => {
  test("has a #!/bin/sh shebang and the owner-execute bit set", () => {
    const source = readFileSync(SCRIPT_PATH, "utf-8");
    expect(source.split("\n")[0]).toBe("#!/bin/sh");

    const mode = statSync(SCRIPT_PATH).mode;
    expect((mode & 0o100) !== 0).toBe(true);
    // The committed mode (100755) is verified independently via
    // `git ls-files -s plugins/emterm/hooks/scripts/notify-status.sh`
    // once the file is staged — a filesystem stat can't see the git index.
  });
});

// ---------------------------------------------------------------------
// AC-2 / TS-1 / TS-9: valid states emit the canonical sequence as JSON.
// ---------------------------------------------------------------------

describe("valid states (AC-2)", () => {
  test.each(ALLOWED_STATES)(
    "state=%s: stdout is one JSON line with exactly key terminalSequence, decoding to the canonical sequence",
    (state) => {
      const { stdout, stderr, exitCode } = runScript([state]);
      expect(exitCode).toBe(0);
      expect(stderr).toBe("");

      const lines = stdout.split("\n").filter((line) => line.length > 0);
      expect(lines.length).toBe(1);

      const parsed = JSON.parse(lines[0] as string) as Record<string, unknown>;
      expect(Object.keys(parsed)).toEqual(["terminalSequence"]);
      expect(parsed.terminalSequence).toBe(canonicalSequence(state));
    },
  );
});

// ---------------------------------------------------------------------
// AC-3: state argument outside the allow-list
// ---------------------------------------------------------------------

describe("state validation (AC-3)", () => {
  test.each(["", "invalid", "WORKING"])(
    "argv=%p: empty stdout, empty stderr, exit 0",
    (state) => {
      const { stdout, stderr, exitCode } = runScript([state]);
      expect(exitCode).toBe(0);
      expect(stdout).toBe("");
      expect(stderr).toBe("");
    },
  );
});

// ---------------------------------------------------------------------
// AC-4: zero positional arguments
// ---------------------------------------------------------------------

describe("argv cardinality: zero args (AC-4)", () => {
  test("empty stdout, empty stderr, exit 0", () => {
    const { stdout, stderr, exitCode } = runScript([]);
    expect(exitCode).toBe(0);
    expect(stdout).toBe("");
    expect(stderr).toBe("");
  });
});

// ---------------------------------------------------------------------
// AC-5: two or more positional arguments
// ---------------------------------------------------------------------

describe("argv cardinality: two or more args (AC-5)", () => {
  test("'working extra': empty stdout, empty stderr, exit 0", () => {
    const { stdout, stderr, exitCode } = runScript(["working", "extra"]);
    expect(exitCode).toBe(0);
    expect(stdout).toBe("");
    expect(stderr).toBe("");
  });
});

// ---------------------------------------------------------------------
// AC-6: shell metacharacters in the argument never reach evaluation.
// Test Notes: assert both the absence of output AND the absence of the
// side-effect file, so a future refactor that starts evaluating the
// argument fails loudly.
// ---------------------------------------------------------------------

describe("shell metacharacter injection (AC-6)", () => {
  test("'working; touch PWNED': empty stdout, exit 0, no PWNED file created", () => {
    const dir = mkdtempSync(join(tmpdir(), "emterm-notify-status-test-"));
    try {
      const { stdout, stderr, exitCode } = runScript(
        ["working; touch PWNED"],
        dir,
      );
      expect(exitCode).toBe(0);
      expect(stdout).toBe("");
      expect(stderr).toBe("");
      expect(existsSync(join(dir, "PWNED"))).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("'$(id)': empty stdout, empty stderr, exit 0", () => {
    const { stdout, stderr, exitCode } = runScript(["$(id)"]);
    expect(exitCode).toBe(0);
    expect(stdout).toBe("");
    expect(stderr).toBe("");
  });
});

// ---------------------------------------------------------------------
// NFR3: dash and bash agree on the emitted line for a valid state.
// ---------------------------------------------------------------------

describe("shell portability (NFR3)", () => {
  test("bash and sh (dash) produce identical stdout for state=working", () => {
    const viaSh = runScript(["working"]);
    const viaBash = Bun.spawnSync(["bash", SCRIPT_PATH, "working"], {
      stdout: "pipe",
      stderr: "pipe",
    });
    expect(viaBash.exitCode).toBe(0);
    expect(viaBash.stdout.toString()).toBe(viaSh.stdout);
  });
});

// ---------------------------------------------------------------------
// AC-9: source hygiene + notify-status.ts deletion
// ---------------------------------------------------------------------

describe("notify-status.sh source hygiene (AC-9)", () => {
  const source = readFileSync(SCRIPT_PATH, "utf-8");

  test("notify-status.ts no longer exists", () => {
    expect(existsSync(join(import.meta.dir, "notify-status.ts"))).toBe(false);
  });

  test("no /dev/tty reference", () => {
    expect(source.includes("/dev/tty")).toBe(false);
  });

  test("no bun reference", () => {
    expect(source.toLowerCase().includes("bun")).toBe(false);
  });

  test("no eval", () => {
    expect(source.includes("eval")).toBe(false);
  });

  test("no backticks", () => {
    expect(source.includes("`")).toBe(false);
  });

  test("no invocation of the emterm binary (only the required OSC payload literal 'emterm;agent-status' survives)", () => {
    // FR3 requires the printf format string to literally contain the OSC
    // payload "emterm;agent-status;...", which is not a subprocess
    // invocation (D1: emterm is never invoked). Strip that one legitimate
    // occurrence and assert nothing else references "emterm" — e.g. no
    // `which emterm`, `command -v emterm`, or `emterm agent-status` spawn.
    const withoutPayloadLiteral = source.split("emterm;agent-status").join("");
    expect(withoutPayloadLiteral.includes("emterm")).toBe(false);
  });
});

// ---------------------------------------------------------------------
// AC-7: hooks.json shape — exec form, no state in `command`, timeout 3,
// ${CLAUDE_PLUGIN_ROOT}-prefixed, no SubagentStop.
// ---------------------------------------------------------------------

interface HookEntry {
  type: string;
  command: string;
  args?: string[];
  timeout: number;
}

interface HookGroup {
  matcher?: string;
  hooks: HookEntry[];
}

interface HooksJson {
  description?: string;
  hooks: Record<string, HookGroup[]>;
}

function readHooksJson(): HooksJson {
  return JSON.parse(readFileSync(HOOKS_JSON_PATH, "utf-8")) as HooksJson;
}

describe("hooks.json shape (AC-7)", () => {
  test("parses as JSON", () => {
    expect(() => readHooksJson()).not.toThrow();
  });

  test("declares exactly UserPromptSubmit, Stop, Notification; no SubagentStop", () => {
    const hooksJson = readHooksJson();
    expect(Object.keys(hooksJson.hooks).sort()).toEqual(
      ["Notification", "Stop", "UserPromptSubmit"].sort(),
    );
    expect(hooksJson.hooks.SubagentStop).toBeUndefined();
  });

  test.each([
    ["UserPromptSubmit", "working"],
    ["Stop", "idle"],
    ["Notification", "blocked"],
  ])(
    "%s hook: exec form, command has no state appended, args=[%s], timeout 3",
    (event, state) => {
      const hooksJson = readHooksJson();
      const entry = hooksJson.hooks[event]?.[0]?.hooks?.[0];
      expect(entry).toBeDefined();
      expect(entry?.type).toBe("command");
      expect(entry?.command).toBe(
        "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.sh",
      );
      expect(entry?.args).toEqual([state]);
      expect(entry?.timeout).toBe(3);
    },
  );

  test("every command is ${CLAUDE_PLUGIN_ROOT}-prefixed, never absolute, and the file has no ..", () => {
    const hooksJson = readHooksJson();
    for (const groups of Object.values(hooksJson.hooks)) {
      for (const group of groups) {
        for (const hook of group.hooks) {
          expect(hook.command.startsWith("${CLAUDE_PLUGIN_ROOT}")).toBe(true);
          expect(hook.command.startsWith("/")).toBe(false);
        }
      }
    }
    const raw = readFileSync(HOOKS_JSON_PATH, "utf-8");
    expect(raw.includes("..")).toBe(false);
  });
});

// ---------------------------------------------------------------------
// AC-8: Notification matcher — pure-function test over the matcher
// string extracted from hooks.json, evaluated against the seven
// notification-type names. No hook execution needed (Test Notes).
// ---------------------------------------------------------------------

describe("Notification matcher (AC-8)", () => {
  const hooksJson = readHooksJson();
  const matcherSource = hooksJson.hooks.Notification?.[0]?.matcher;

  test("matcher is defined", () => {
    expect(matcherSource).toBeDefined();
  });

  test.each(["permission_prompt", "elicitation_dialog", "agent_needs_input"])(
    "matches %s",
    (name) => {
      const matcher = new RegExp(matcherSource as string);
      expect(matcher.test(name)).toBe(true);
    },
  );

  test.each([
    "idle_prompt",
    "auth_success",
    "elicitation_complete",
    "elicitation_response",
  ])("does not match %s", (name) => {
    const matcher = new RegExp(matcherSource as string);
    expect(matcher.test(name)).toBe(false);
  });
});
