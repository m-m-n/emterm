/**
 * Tests for the Claude Code hook script `notify-status.sh` and the
 * `hooks.json` manifest that wires it up (task0001; event set reworked by
 * task0009 — `StopFailure` removed as a documented no-op, `PostToolUseFailure`
 * added so approve-then-fail also clears `blocked`).
 *
 * `notify-status.sh` is exercised as a real subprocess, invoked explicitly
 * via `sh <script> <args…>`, so the assertions cover POSIX-shell semantics
 * rather than the developer's login shell (IMPLEMENTATION.md D3, Test
 * Notes). No fakes: this is the shipped artifact itself.
 *
 * See feature-docs/emterm-plugin-runtime-fixes/{SPEC,tasks/task0001,tasks/task0009}.md.
 */

import { describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const REPO_ROOT = join(import.meta.dir, "..", "..", "..", "..");
const SCRIPT_PATH = join(import.meta.dir, "notify-status.sh");
const HOOKS_JSON_PATH = join(REPO_ROOT, "plugins/emterm/hooks/hooks.json");
const AGENT_STATUS_RS_PATH = join(
  REPO_ROOT,
  "src-tauri",
  "src",
  "agent_status.rs",
);

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
 * Decode a Rust `&str` literal body (the text captured between the quotes
 * by `extractRustStrConst` below, escapes intact) into the runtime string
 * it represents. `src-tauri/src/agent_status.rs`'s wire constants use only
 * `\xHH` byte escapes and a literal `\\`; both are handled here, plus the
 * common `\n`/`\t`/`\r`/`\"` escapes for robustness. Any other escape is a
 * source-format surprise this derivation does not understand, so it throws
 * rather than silently guessing (task0004.md AC-7 — fail loudly).
 */
function decodeRustStringLiteral(body: string): string {
  let out = "";
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (ch !== "\\") {
      out += ch;
      continue;
    }
    const next = body[++i];
    switch (next) {
      case "x": {
        const hex = body.slice(i + 1, i + 3);
        i += 2;
        out += String.fromCharCode(Number.parseInt(hex, 16));
        break;
      }
      case "\\":
        out += "\\";
        break;
      case '"':
        out += '"';
        break;
      case "n":
        out += "\n";
        break;
      case "t":
        out += "\t";
        break;
      case "r":
        out += "\r";
        break;
      default:
        throw new Error(
          `decodeRustStringLiteral: unsupported escape '\\${next}' while decoding agent_status.rs constant literal ${JSON.stringify(body)}`,
        );
    }
  }
  return out;
}

/**
 * Extract a `const <name>: &str = "...";` value out of Rust source,
 * tolerating ordinary formatting variation (whitespace around `=`, a
 * trailing `//` comment after the semicolon) but throwing loudly — never
 * falling back to an assumed value — if the constant cannot be found. This
 * is the mechanism that turns a Rust-side wire-format change into a
 * failing test here instead of silent drift (task0004.md F4 /
 * SPEC.md "Wire-format duplication").
 */
function extractRustStrConst(source: string, name: string): string {
  const pattern = new RegExp(
    `const\\s+${name}\\b\\s*:\\s*&str\\s*=\\s*"((?:[^"\\\\]|\\\\.)*)"\\s*;`,
  );
  const match = pattern.exec(source);
  if (!match) {
    throw new Error(
      `extractRustStrConst: could not find 'const ${name}: &str = "...";' in ${AGENT_STATUS_RS_PATH} — the wire-format drift detection this backs (task0004 AC-7) cannot verify the hook's expected sequence`,
    );
  }
  return decodeRustStringLiteral(match[1] as string);
}

/**
 * The four wire-format constants (task0004.md F4), derived from
 * `src-tauri/src/agent_status.rs` at module-load time rather than
 * hardcoded — a missing constant throws immediately and fails every test
 * in this file loudly, rather than silently falling back to a stale
 * literal. This is read-only access to `src-tauri/`; nothing there is
 * written (task0004.md "Out of Scope").
 */
const AGENT_STATUS_RS_SOURCE = readFileSync(AGENT_STATUS_RS_PATH, "utf-8");
const OSC_INTRODUCER = extractRustStrConst(
  AGENT_STATUS_RS_SOURCE,
  "OSC_INTRODUCER",
);
const PAYLOAD_PREFIX = extractRustStrConst(
  AGENT_STATUS_RS_SOURCE,
  "PAYLOAD_PREFIX",
);
const WIRE_VERSION = extractRustStrConst(
  AGENT_STATUS_RS_SOURCE,
  "WIRE_VERSION",
);
const ST = extractRustStrConst(AGENT_STATUS_RS_SOURCE, "ST");

describe("wire-format constant extraction from agent_status.rs (AC-7)", () => {
  test("extracts OSC_INTRODUCER, PAYLOAD_PREFIX, WIRE_VERSION, ST matching the documented wire grammar", () => {
    expect(OSC_INTRODUCER).toBe("\x1b]777;");
    expect(PAYLOAD_PREFIX).toBe("emterm;agent-status;");
    expect(WIRE_VERSION).toBe("1");
    expect(ST).toBe("\x1b\\");
  });

  test("fails loudly (throws) rather than falling back, when a constant name is not present in the source", () => {
    expect(() =>
      extractRustStrConst(AGENT_STATUS_RS_SOURCE, "DOES_NOT_EXIST"),
    ).toThrow();
  });
});

/**
 * The canonical wire-format sequence (SPEC.md FR3), byte-identical to what
 * `crate::agent_status::build` in `src-tauri/src/agent_status.rs` emits for
 * name "claude-code" — now derived from the four extracted constants above
 * rather than a hardcoded literal (task0004.md F4), so a Rust-side change
 * to `WIRE_VERSION`, `PAYLOAD_PREFIX`, `OSC_INTRODUCER`, or `ST` shows up
 * as a failing test here instead of silent drift.
 *
 * `name=claude-code` is appended as a literal rather than run through the
 * Rust builder's percent-encoder: `claude-code` is composed entirely of
 * URI-unreserved characters (letters, digits, `-`), so percent-encoding it
 * is the identity transform. This is a deliberate simplification, not an
 * oversight — a name containing reserved characters would need the encoder
 * mirrored here too, but nothing this suite tests sends one.
 */
function canonicalSequence(state: string): string {
  return `${OSC_INTRODUCER}${PAYLOAD_PREFIX}v=${WIRE_VERSION};state=${state};name=claude-code${ST}`;
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

  test("no command substitution (task0010, finding sp-verification-dollarparen-grep: NFR2's third prohibition, alongside eval and backticks, must be mechanically checked here too)", () => {
    expect(source.includes("$(")).toBe(false);
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

  test("declares exactly UserPromptSubmit, PostToolUse, PostToolUseFailure, Stop, Notification; no SubagentStop, no StopFailure", () => {
    // StopFailure is deliberately absent (task0009, finding cm-stopfailure-noop):
    // the hooks documentation states its output and exit code are ignored by
    // Claude Code, and this hook transports state exclusively through the
    // `terminalSequence` field of its stdout JSON, so a StopFailure entry
    // could never report anything.
    const hooksJson = readHooksJson();
    expect(Object.keys(hooksJson.hooks).sort()).toEqual(
      ["Notification", "PostToolUse", "PostToolUseFailure", "Stop", "UserPromptSubmit"].sort(),
    );
    expect(hooksJson.hooks.SubagentStop).toBeUndefined();
    expect(hooksJson.hooks.StopFailure).toBeUndefined();
  });

  test.each([
    ["UserPromptSubmit", "working"],
    ["PostToolUse", "working"],
    ["PostToolUseFailure", "working"],
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
// string extracted from hooks.json, evaluated against the eight
// documented notification-type names (task0010, finding
// cm-matcher-test-missing-type: the previous negative set counted seven
// and omitted `agent_completed`; a matcher loosened to a prefix match
// would set `blocked` when a background agent COMPLETES — the opposite
// of a wait — with nothing here failing to catch it). No hook execution
// needed (Test Notes).
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
    "agent_completed",
  ])("does not match %s", (name) => {
    const matcher = new RegExp(matcherSource as string);
    expect(matcher.test(name)).toBe(false);
  });
});
