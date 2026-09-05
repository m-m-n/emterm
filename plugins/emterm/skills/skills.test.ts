/**
 * Static scan of the plugin's SKILL.md files (task0003 — display / mux
 * skills; task0002 — display-skill argument-injection hardening; task0004
 * — rework of the hardening to a Bash-first, shell-safe form; task0006 —
 * round-2 rework closing the `~`-expansion regression, the double-quote
 * near-miss, the display-image self-contradiction, and the mux-send
 * argv-array unreachable instruction; task0007 — round-4 rework replacing
 * the two constructs whose safety depended on the model applying a rule
 * correctly (mux-send's quoted-delimiter heredoc, the display skills' `~`
 * exception) with forms that have no hole to describe: file redirection
 * for untrusted `--stdin` text, and a single "resolve to absolute, then
 * single-quote the whole path" invariant for `~`).
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
 * round 1 used. task0007 keeps that bar: the heredoc-absence and
 * `"$HOME"'`-absence guards assert on the literal syntax that must never
 * reappear, not on loose word presence (task0007.md "Test Notes"). Whether
 * Claude actually auto-invokes the right skill for a given prompt is human
 * judgment and lives in the verify phase (task0003.md "Test Notes").
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

/**
 * Matches a shell heredoc operator regardless of delimiter quoting style
 * (`<<EOF`, `<< EOF`, `<<'EOF'`, `<<"EOF"`, `<<-EOF`) — task0010, finding
 * cm-heredoc-guard-too-narrow. The previous guard (`content.includes("<<'")`)
 * matched only the two characters that open a QUOTED delimiter, so an
 * unquoted heredoc passed silently despite being strictly more dangerous
 * (its body also expands command substitution, backticks, and parameter
 * expansion, on top of the same delimiter-collision termination risk).
 */
const HEREDOC_OPERATOR = /<<-?\s*['"]?[A-Za-z_]/;

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

    test("AC-5 (task0007, finding sc-tilde-outside-quotes-invariant; restated task0010, finding sc-invariant-contradicts-escape): documents the ~ rule as 'resolve to an absolute path first, then single-quote the whole path', and states the invariant with the '\\'' splice as part of it rather than as an exception", () => {
      expect(content).toMatch(/single quotes suppress `~` expansion/);
      expect(content).toMatch(
        /resolve\s+it\s+to\s+an\s+absolute\s+path\s+yourself\s+before\s+quoting/,
      );
      // task0010 F3: the old absolute framing ("no byte derived from an
      // untrusted path ever appears outside the single quotes") directly
      // contradicted the splice bullet two bullets later, which places the
      // fixed `'\''` bytes outside the quotes. The invariant now folds the
      // splice into itself instead of excepting it out.
      expect(content).toMatch(
        /every\s+byte\s+of\s+the\s+path\s+is\s+either\s+inside\s+(?:that|a)\s+single-quoted\s+span\s+or\s+is\s+part\s+of\s+the\s+fixed\s+four-character\s+(?:splice|`'\\''`\s+splice)/,
      );
      expect(content).toMatch(
        /nothing\s+else\s+path-derived\s+ever\s+appears\s+outside\s+the\s+(?:single\s+)?quotes/,
      );
    });

    test("AC-5 (task0010, finding sc-invariant-contradicts-escape): the ~ bullet and the '\\'' splice bullet are adjacent, with the double-quote bullet outside that pair", () => {
      const tildeIdx = content.search(
        /resolve\s+it\s+to\s+an\s+absolute\s+path\s+yourself\s+before\s+quoting/,
      );
      const spliceIdx = content.search(
        /insert\s+`'\\''`\s+\(end-quote, escaped literal quote, reopen-quote\)/,
      );
      const doubleQuoteIdx = content.search(
        /Double quotes are NOT a safe substitute for single quotes/,
      );
      expect(tildeIdx).toBeGreaterThan(-1);
      expect(spliceIdx).toBeGreaterThan(-1);
      expect(doubleQuoteIdx).toBeGreaterThan(-1);
      expect(spliceIdx).toBeGreaterThan(tildeIdx);
      expect(doubleQuoteIdx).toBeGreaterThan(spliceIdx);
    });

    test("AC-6 (task0007, finding sc-tilde-outside-quotes-invariant): the old \"$HOME\"'/...' exception form is gone — nothing sits outside the single quotes", () => {
      expect(content).not.toMatch(/"\$HOME"'/);
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

describe("plugins/emterm/skills/mux-send/SKILL.md rework (task0007, findings sc-heredoc-delimiter-collision + cm-heredoc-trailing-newline-executes)", () => {
  const content = readFileSync(
    join(SKILLS_DIR, "mux-send", "SKILL.md"),
    "utf-8",
  );

  test("AC-1: the primary --stdin form for untrusted text is file redirection, and no heredoc form is present", () => {
    expect(content).toContain("emterm mux send --pane <id> --stdin < '<file>'");
    // The regression guard that matters most (task0007.md 'Test Notes'),
    // widened in task0010 (finding cm-heredoc-guard-too-narrow): the
    // original guard rejected only the two characters that open a QUOTED
    // delimiter (`<<'`), so a bare `<<EOF` heredoc — strictly more
    // dangerous, since command substitution/backticks/parameter expansion
    // also run inside its body — would have passed silently. This regex
    // rejects the heredoc operator regardless of whether the delimiter is
    // quoted or bare.
    expect(content).not.toMatch(HEREDOC_OPERATOR);
  });

  test("AC-2: states nothing derived from the text enters the command line in that form, and that a trailing newline in the file is an Enter in the destination pane", () => {
    expect(content).toMatch(
      /nothing\s+derived from the text ever enters the command line/,
    );
    expect(content).toMatch(
      /trailing newline\s+in the file is an Enter in the destination pane/,
    );
  });

  test("AC-3: --text guidance matches the display skills' single-quote plus '\\'' rule, --text is restricted to trusted strings, pane-ID validation is retained, and the argv-array note is a conditional alternative (not the primary form)", () => {
    expect(content).toMatch(/single-quote the value/i);
    expect(content).toContain("'\\''");
    expect(content).toContain("^[a-z0-9-]+$");
    expect(content).toMatch(/trusted strings only/);
    const stdinIdx = content.indexOf("Required for untrusted text");
    const argvIdx = content.indexOf("no-shell exec path is available");
    expect(stdinIdx).toBeGreaterThan(-1);
    expect(argvIdx).toBeGreaterThan(stdinIdx);
  });

  test("AC-4: every 'Safe' label is true of the file-redirection form, the file states the destination pane executes what it receives, and that sending untrusted text to a shell pane needs user consent", () => {
    expect(content).not.toMatch(HEREDOC_OPERATOR);
    expect(content).toMatch(
      /destination pane executes what it receives if it is running a shell/,
    );
    expect(content).toMatch(/needs\s+the user's consent/);
    // Every adversarial-example "Safe" claim must be paired with the new
    // primary form, not the retired heredoc.
    const safeMatches = [...content.matchAll(/Safe:/g)];
    expect(safeMatches.length).toBeGreaterThanOrEqual(3);
  });
});

describe("plugins/emterm/skills/mux-send/SKILL.md staging-file + redirect-path hardening (task0010, findings sc-stagefile-unspecified, sc-redirect-path-unruled, sc-consent-vs-invoke-as-is, sp-text-example-unbalanced)", () => {
  const content = readFileSync(
    join(SKILLS_DIR, "mux-send", "SKILL.md"),
    "utf-8",
  );

  test("AC-1: the staging file MUST be created with the Write tool, and Bash-based creation (heredoc, printf, echo, interpolated-variable redirect) is forbidden for untrusted text", () => {
    expect(content).toMatch(/MUST be created with the Write tool/);
    expect(content).toMatch(/forbidden for untrusted text/);
    expect(content).toContain("heredoc");
    expect(content).toContain("printf");
    expect(content).toContain("echo");
    expect(content).toMatch(/interpolated shell variable/);
  });

  test("AC-2: the 'nothing enters the command line' claim is qualified to hold only when the file was written without a shell", () => {
    expect(content).toMatch(
      /holds only because the file was written without a shell/,
    );
  });

  test("AC-3: the heredoc-rejecting adversarial example covers file creation as well as --stdin supply", () => {
    expect(content).toMatch(
      /heredoc\s+through\s+the\s+Bash\s+tool\s+instead\s+of\s+the\s+Write\s+tool/,
    );
    expect(content).toMatch(
      /whether to supply `--stdin` directly or to create the file/,
    );
  });

  test("AC-4: the redirect target is a stated requirement (model-chosen, absolute, temp directory, no ~, no untrusted-derived bytes), with the display skills' path rules given for a caller-supplied path", () => {
    expect(content).toMatch(
      /redirect target[^.]*is a requirement, not a free choice/,
    );
    expect(content).toMatch(/under a temp directory/);
    expect(content).toMatch(/containing no `~`/);
    expect(content).toMatch(
      /resolve a leading `~`\s+to an absolute path yourself first/,
    );
    expect(content).toMatch(/splice it as `'\\''`/);
  });

  test("AC-6: the closing instruction is conditional on user consent for untrusted text to a shell pane, and states what happens on refusal", () => {
    expect(content).toMatch(/show the user the exact bytes/);
    expect(content).toMatch(
      /explicit\s+approval\s+before\s+invoking\s+the\s+command/,
    );
    expect(content).toMatch(/If the user declines, do not invoke/);
    expect(content).toMatch(
      /Otherwise[^.]*invoke the command as-is and report the result back to the user/,
    );
  });

  test("AC-9: the --text embedded-quote example shows the complete quoted value, including the outer quotes, not the splice substring alone", () => {
    expect(content).toContain("'it'\\''s'");
    expect(content).toContain("emterm mux send --pane <id> --text 'it'\\''s'");
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
    expect(content).not.toMatch(
      /3\s*s(econds)?\s*per\s*(Claude Code\s*)?prompt/i,
    );
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

describe("feature-docs/emterm-plugin-runtime-fixes/{SPEC,VERIFICATION}.md byte hygiene (task0008 AC-8, finding sp-fr3-raw-esc-byte)", () => {
  const FEATURE_DOCS_DIR = join(
    MARKETPLACE_ROOT,
    "feature-docs",
    "emterm-plugin-runtime-fixes",
  );
  // Mirrors `grep -P '[\x00-\x08\x0b-\x1f]'`: only tab (0x09) and newline
  // (0x0a) are excluded from the matched range; every other C0 control
  // byte (including CR, 0x0d) is flagged. A raw ESC (0x1B) byte written
  // into FR3's escaping clause previously made a round-3 review record
  // unparseable YAML for exactly this reason.
  const RAW_CONTROL_BYTE = /[\x00-\x08\x0b-\x1f]/;

  for (const filename of ["SPEC.md", "VERIFICATION.md"]) {
    test(`${filename} contains no raw control bytes`, () => {
      const content = readFileSync(join(FEATURE_DOCS_DIR, filename), "utf-8");
      expect(RAW_CONTROL_BYTE.test(content)).toBe(false);
    });
  }
});

describe("plugin/marketplace version regression guard (task0002 AC-9)", () => {
  test("plugin.json still reports version 0.1.1", () => {
    const pluginJson = JSON.parse(
      readFileSync(join(PLUGIN_DIR, ".claude-plugin", "plugin.json"), "utf-8"),
    ) as { version: string };
    expect(pluginJson.version).toBe("0.1.1");
  });

  test("marketplace.json still reports the emterm plugin entry as version 0.1.1", () => {
    const marketplaceJson = JSON.parse(
      readFileSync(
        join(MARKETPLACE_ROOT, ".claude-plugin", "marketplace.json"),
        "utf-8",
      ),
    ) as { plugins: Array<{ name: string; version: string }> };
    const entry = marketplaceJson.plugins.find((p) => p.name === "emterm");
    expect(entry?.version).toBe("0.1.1");
  });
});
