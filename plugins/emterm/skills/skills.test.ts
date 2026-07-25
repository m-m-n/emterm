/**
 * Static scan of the plugin's SKILL.md files (task0003 — display / mux
 * skills; task0002 — display-skill argument-injection hardening; task0004
 * — rework of the hardening to a Bash-first, shell-safe form; task0006 —
 * round-2 rework closing the `~`-expansion regression, the double-quote
 * near-miss, the display-image self-contradiction, and the mux-send
 * argv-array unreachable instruction).
 *
 * Skill content is prose; there is no runtime to exercise inside a task
 * worktree, so these tests assert the static invariants the task plans'
 * Acceptance Criteria describe: frontmatter shape, the exact `emterm`
 * invocation each body must contain, path hygiene, and (for the four
 * display skills) the argument-injection-safety guardrails. task0004
 * replaced task0002's "no-shell invocation" primary MUST — a path Claude
 * Code cannot reach, since a skill only ever calls `emterm` through the
 * Bash tool — with the quoted-plus-`--` shell form as the primary MUST, so
 * these tests assert the copy-pastable safe form directly rather than
 * loose word-presence checks (task0004.md "Test Notes"). task0006 adds
 * regression guards specific enough to fail on a wording regression
 * (task0006.md "Test coverage") rather than the loose word-presence checks
 * round 1 used. Whether Claude actually auto-invokes the right skill for a
 * given prompt is human judgment and lives in the verify phase (task0003.md
 * "Test Notes").
 */

import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parse as parseYaml } from "yaml";

const SKILLS_DIR = import.meta.dir;
const PLUGIN_DIR = join(SKILLS_DIR, "..");
const MARKETPLACE_ROOT = join(SKILLS_DIR, "..", "..", "..");

/** The four display skills gain argument-injection hardening (task0002.md "Display-skill hardening (FR7)"). */
const DISPLAY_SKILLS = [
  "display-markdown",
  "display-json",
  "display-yaml",
  "display-image",
];

/** Directory slug -> exact `emterm` invocation its body must contain (task0003.md "Per-skill invocation mapping"). */
const SKILLS: Record<string, string> = {
  "display-markdown": "emterm markdown",
  "display-json": "emterm json",
  "display-yaml": "emterm yaml",
  "display-image": "emterm image",
  "mux-read": "emterm mux read",
  "mux-send": "emterm mux send",
  "mux-wait": "emterm mux wait",
};

interface ParsedSkill {
  frontmatter: Record<string, unknown>;
  body: string;
}

/** Split a SKILL.md into its YAML frontmatter block and Markdown body. */
function parseSkillMd(content: string): ParsedSkill {
  const match = content.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);
  if (!match) {
    throw new Error("no YAML frontmatter block found (expected leading ---)");
  }
  const [, frontmatterRaw, body] = match;
  const frontmatter = (parseYaml(frontmatterRaw ?? "") ?? {}) as Record<
    string,
    unknown
  >;
  return { frontmatter, body: body ?? "" };
}

for (const [slug, invocation] of Object.entries(SKILLS)) {
  describe(`plugins/emterm/skills/${slug}/SKILL.md`, () => {
    const path = join(SKILLS_DIR, slug, "SKILL.md");
    const content = readFileSync(path, "utf-8");
    const { frontmatter, body } = parseSkillMd(content);

    test("AC-1: file exists with a non-empty, parseable YAML frontmatter block", () => {
      expect(content.length).toBeGreaterThan(0);
      expect(frontmatter).toBeTruthy();
    });

    test("AC-2: name frontmatter matches the directory name exactly", () => {
      expect(frontmatter.name).toBe(slug);
    });

    test("AC-3: description is a non-empty English single sentence stating the trigger condition", () => {
      expect(typeof frontmatter.description).toBe("string");
      const description = (frontmatter.description as string).trim();
      expect(description.length).toBeGreaterThan(0);
      // A YAML scalar with an embedded newline is not "single sentence" shaped.
      expect(description).not.toContain("\n");
      // English proxy: no CJK script in the description.
      expect(/[぀-ヿ㐀-鿿]/.test(description)).toBe(false);
    });

    test("AC-4: body instructs the model to invoke the exact emterm subcommand", () => {
      expect(body).toContain(invocation);
    });

    test("AC-6: no absolute path or path outside plugins/emterm/ is referenced", () => {
      expect(content).not.toContain("../");
      expect(content).not.toMatch(/\/home\/|\/Users\/|[A-Za-z]:\\/);
      expect(content).not.toContain("${CLAUDE_PLUGIN_ROOT}");
    });
  });
}

describe("plugins/emterm/skills/display-image/SKILL.md", () => {
  const content = readFileSync(
    join(SKILLS_DIR, "display-image", "SKILL.md"),
    "utf-8",
  );

  test("AC-5: body documents the optional --protocol kitty|sixel argument", () => {
    expect(content).toContain("--protocol");
    expect(content).toContain("kitty");
    expect(content).toContain("sixel");
  });

  test("AC-5 (task0004): --protocol is placed before the -- end-of-options delimiter", () => {
    expect(content).toMatch(/--protocol[^\n]*--\s+'<path>'/);
  });
});

/**
 * Directory slug -> the exact canonical-example text before the trailing
 * `-- '<path>'` (task0004.md F3, item 1: options first, then `--`, then
 * the single-quoted path). `display-image`'s canonical example carries no
 * `--protocol` as of task0006 F3 — the protocol-specific form is a
 * separate second example (see the task0006 F3 describe block below).
 */
const CANONICAL_EXAMPLE_PREFIX: Record<string, string> = {
  "display-markdown": "emterm markdown",
  "display-json": "emterm json",
  "display-yaml": "emterm yaml",
  "display-image": "emterm image",
};

/** Extract the first fenced code block in a SKILL.md body — the canonical usage example. */
function extractFirstCodeBlock(body: string): string {
  const match = body.match(/```\n([\s\S]*?)```/);
  if (!match) {
    throw new Error("no fenced code block found in SKILL.md body");
  }
  return (match[1] ?? "").trim();
}

for (const slug of DISPLAY_SKILLS) {
  describe(`plugins/emterm/skills/${slug}/SKILL.md safe invocation form (task0004)`, () => {
    const content = readFileSync(join(SKILLS_DIR, slug, "SKILL.md"), "utf-8");
    const { body } = parseSkillMd(content);
    const canonicalExample = extractFirstCodeBlock(body);

    test("AC-1: the canonical example is options-first, then --, then a single-quoted path", () => {
      expect(canonicalExample).toBe(
        `${CANONICAL_EXAMPLE_PREFIX[slug]} -- '<path>'`,
      );
    });

    test("AC-2: the quoted-plus-- form is stated as the primary requirement, and the '\\'' escaping rule is documented", () => {
      expect(content).toContain("Always single-quote the path");
      expect(content).toContain("'\\''");
    });

    test("AC-3: the 'where the command supports it' hedge on -- is gone", () => {
      expect(content).not.toContain("where the command supports it");
    });

    test("AC-4: contains an adversarial example whose safe side is a directly copyable command (-- plus a single-quoted path containing the injection payload)", () => {
      expect(content).toContain("touch PWNED");
      expect(content).toMatch(/--\s+'[^']*touch PWNED[^']*'/);
    });
  });
}

for (const slug of DISPLAY_SKILLS) {
  describe(`plugins/emterm/skills/${slug}/SKILL.md quoting hardening (task0006)`, () => {
    const content = readFileSync(join(SKILLS_DIR, slug, "SKILL.md"), "utf-8");

    test("AC-1 (finding cm-tilde-expansion-broken): documents the ~ rule with a concrete safe form that expands only the leading $HOME/~ segment and keeps the untrusted remainder single-quoted", () => {
      expect(content).toMatch(/single quotes suppress `~` expansion/);
      expect(content).toMatch(/"\$HOME"'\/[^']*'/);
    });

    test("AC-2 (finding sc-doublequote-not-taught): states double quotes are insufficient and names $(...), backticks, and ${...}", () => {
      expect(content).toMatch(
        /Double quotes are NOT a safe substitute for single quotes/,
      );
      expect(content).toContain("$(...)");
      expect(content).toContain("backtick");
      expect(content).toContain("${...}");
    });

    test("AC-3 (finding sc-doublequote-not-taught): carries a command-substitution adversarial example contrasting the single-quoted safe form against the double-quoted unsafe form", () => {
      expect(content).toMatch(/--\s+'[^']*\$\(touch PWNED\)[^']*'/);
      expect(content).toMatch(/--\s+"[^"]*\$\(touch PWNED\)[^"]*"/);
    });
  });
}

describe("plugins/emterm/skills/display-image/SKILL.md protocol example (task0006 F3, finding cm-display-image-self-contradiction)", () => {
  const content = readFileSync(
    join(SKILLS_DIR, "display-image", "SKILL.md"),
    "utf-8",
  );
  const { body } = parseSkillMd(content);
  const canonicalExample = extractFirstCodeBlock(body);

  test("AC-4: the canonical example carries no --protocol", () => {
    expect(canonicalExample).toBe("emterm image -- '<path>'");
    expect(canonicalExample).not.toContain("--protocol");
  });

  test("AC-4: a second example shows --protocol sixel placed before the -- delimiter", () => {
    expect(content).toMatch(/emterm image --protocol sixel -- '<path>'/);
  });

  test("AC-5: the prose states the default protocol so it no longer contradicts a --protocol-free canonical example", () => {
    expect(content).toMatch(/default[^.]*Kitty Graphics Protocol/i);
  });
});

describe("plugins/emterm/skills/mux-send/SKILL.md rework (task0006 F4, finding sc-mux-send-unreachable)", () => {
  const content = readFileSync(join(SKILLS_DIR, "mux-send", "SKILL.md"), "utf-8");

  test("AC-6: the primary --stdin form is a quoted-delimiter heredoc, and it appears before the argv-array alternative", () => {
    expect(content).toMatch(/--stdin <<'[A-Z]+'/);
    expect(content).toMatch(/no-shell exec path is available/i);
    const heredocIdx = content.indexOf("quoted-delimiter heredoc");
    const argvIdx = content.indexOf("no-shell exec path is available");
    expect(heredocIdx).toBeGreaterThan(-1);
    expect(argvIdx).toBeGreaterThan(heredocIdx);
  });

  test("AC-6: the argv-array form is no longer presented as the primary requirement", () => {
    expect(content).not.toMatch(
      /assemble the invocation as an argv array.*so the text is never/s,
    );
  });

  test("AC-6: notes the delimiter-collision condition", () => {
    expect(content).toMatch(/must not contain a line consisting solely/);
  });

  test("AC-7: --text guidance matches the display skills' single-quote plus '\\'' rule, and pane-ID validation is retained", () => {
    expect(content).toContain("single-quote the value");
    expect(content).toContain("'\\''");
    expect(content).toContain("^[a-z0-9-]+$");
  });
});

describe("plugins/emterm/skills/mux-read/SKILL.md and mux-wait/SKILL.md (task0006 AC-8: unchanged)", () => {
  test("mux-read still carries the prompt-injection boundary section untouched by task0006", () => {
    const content = readFileSync(
      join(SKILLS_DIR, "mux-read", "SKILL.md"),
      "utf-8",
    );
    expect(content).toContain("Prompt-injection boundary (required)");
    expect(content).toContain("UNTRUSTED DATA");
  });

  test("mux-wait carries no free-form text or path argument", () => {
    const content = readFileSync(
      join(SKILLS_DIR, "mux-wait", "SKILL.md"),
      "utf-8",
    );
    expect(content).not.toContain("Argument-injection safety");
  });
});

describe("plugins/emterm/README.md (task0002)", () => {
  const content = readFileSync(join(PLUGIN_DIR, "README.md"), "utf-8");

  test("AC-1: does not list bun as a prerequisite", () => {
    expect(content.toLowerCase()).not.toContain("bun");
  });

  test("AC-2: states the Claude Code minimum version for the agent-status hook", () => {
    expect(content).toContain("2.1.141");
  });

  test("AC-3: states emterm on PATH is needed for the display and mux skills but not the agent-status hook", () => {
    expect(content).toContain("display and mux skills");
    expect(content).toContain("does not invoke it");
  });

  test("AC-4: no longer contains the /dev/tty reachability limitation", () => {
    expect(content).not.toContain("/dev/tty");
  });

  test("AC-4: no longer contains the up-to-3-seconds-per-prompt limitation", () => {
    expect(content).not.toMatch(/3\s*s(econds)?\s*per\s*(Claude Code\s*)?prompt/i);
  });

  test("AC-5: still contains the mux-agent-status-api drain-wiring limitation", () => {
    expect(content).toContain("mux-agent-status-api");
    expect(content).toContain("drain");
  });
});

describe("plugins/emterm/README.md hook wiring (task0005 AC-6)", () => {
  const readmeContent = readFileSync(join(PLUGIN_DIR, "README.md"), "utf-8");
  const hooksJson = JSON.parse(
    readFileSync(join(PLUGIN_DIR, "hooks", "hooks.json"), "utf-8"),
  ) as { hooks: Record<string, unknown> };

  /**
   * Extract a `## <heading>` section's body, up to the next `## ` heading or
   * EOF. A trailing sentinel heading is appended so the non-greedy capture
   * always has a `\n## ` to stop at — with the `m` flag, a bare `$`
   * alternative would also match the blank line directly under the heading
   * (end-of-line, not end-of-string), truncating the capture to "".
   */
  function extractSection(content: string, heading: string): string {
    const terminated = `${content}\n## `;
    const pattern = new RegExp(`^## ${heading}\\n([\\s\\S]*?)\\n## `, "m");
    const match = terminated.match(pattern);
    if (!match) {
      throw new Error(`no "## ${heading}" section found in README.md`);
    }
    return match[1] ?? "";
  }

  const wiredSection = extractSection(readmeContent, "What gets wired");
  // Event names in the wiring line are written as `` `EventName` → `state` ``.
  const eventNamesInReadme = [
    ...wiredSection.matchAll(/`([A-Za-z]+)`\s*→/g),
  ].map((m) => m[1]);
  const eventNamesInHooksJson = Object.keys(hooksJson.hooks);

  test("README names exactly the hooks.json event keys, so an event added to or removed from hooks.json without a README edit fails this test", () => {
    expect(eventNamesInHooksJson.length).toBeGreaterThan(0);
    expect([...eventNamesInReadme].sort()).toEqual(
      [...eventNamesInHooksJson].sort(),
    );
  });
});

describe("plugin/marketplace version regression guard (task0002 AC-9)", () => {
  test("plugin.json still reports version 0.1.0", () => {
    const pluginJson = JSON.parse(
      readFileSync(join(PLUGIN_DIR, ".claude-plugin", "plugin.json"), "utf-8"),
    ) as { version: string };
    expect(pluginJson.version).toBe("0.1.0");
  });

  test("marketplace.json still reports the emterm plugin entry as version 0.1.0", () => {
    const marketplaceJson = JSON.parse(
      readFileSync(
        join(MARKETPLACE_ROOT, ".claude-plugin", "marketplace.json"),
        "utf-8",
      ),
    ) as { plugins: Array<{ name: string; version: string }> };
    const entry = marketplaceJson.plugins.find((p) => p.name === "emterm");
    expect(entry?.version).toBe("0.1.0");
  });
});
