/**
 * Static scan of the plugin's SKILL.md files (task0003 — display / mux
 * skills; task0002 — display-skill argument-injection hardening).
 *
 * Skill content is prose; there is no runtime to exercise inside a task
 * worktree, so these tests assert the static invariants the task plans'
 * Acceptance Criteria describe: frontmatter shape, the exact `emterm`
 * invocation each body must contain, path hygiene, and (for the four
 * display skills) the argument-injection-safety guardrails required by
 * task0002.md. Whether Claude actually auto-invokes the right skill for a
 * given prompt is human judgment and lives in the verify phase
 * (task0003.md "Test Notes").
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
});

for (const slug of DISPLAY_SKILLS) {
  describe(`plugins/emterm/skills/${slug}/SKILL.md argument-injection hardening (task0002)`, () => {
    const content = readFileSync(join(SKILLS_DIR, slug, "SKILL.md"), "utf-8");

    test("AC-6: contains a required-safety section requiring a single argv element, never a shell-interpolated string", () => {
      expect(content).toContain("single argv element");
      expect(content).toContain("never");
      expect(content).toContain("interpolated");
    });

    test("AC-6: states the shell-quoting fallback for when a shell is unavoidable", () => {
      expect(content.toLowerCase()).toContain("shell-quoted");
    });

    test("AC-7: contains at least one adversarial example using a path with shell metacharacters", () => {
      // Distinctive token rather than a whole sentence, per task0002.md
      // "Test Notes" — robust to wording changes around it.
      expect(content).toContain("touch PWNED");
    });
  });
}

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
