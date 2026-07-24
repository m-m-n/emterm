/**
 * Static scan of the plugin's SKILL.md files (task0003 — display / mux
 * skills).
 *
 * Skill content is prose; there is no runtime to exercise inside a task
 * worktree, so these tests assert the static invariants task0003.md's
 * Acceptance Criteria describe: frontmatter shape, the exact `emterm`
 * invocation each body must contain, and path hygiene. Whether Claude
 * actually auto-invokes the right skill for a given prompt is human
 * judgment and lives in the verify phase (task0003.md "Test Notes").
 */

import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parse as parseYaml } from "yaml";

const SKILLS_DIR = import.meta.dir;

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
