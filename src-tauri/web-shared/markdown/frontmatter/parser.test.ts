/**
 * Unit tests for the front matter parser (task0002).
 *
 * Pure-function tests (no DOM). Each test names the Acceptance Criterion it
 * proves. Covers SPEC.md TS-1/TS-2/TS-3 (parsing half), TS-6 (failure
 * capture), and TS-10 (empty block). AC-6 (bundler compatibility) is verified
 * by running `bun run build:viewer` in-task, not by a unit test.
 */

import { describe, expect, test } from "bun:test";

import { parseFrontMatter } from "./parser.ts";
import { MAX_NODES } from "./tree-builder.ts";
import type {
  FrontMatterParseFailure,
  FrontMatterParseSuccess,
  FrontMatterValue,
} from "./types.ts";

/** Narrow to the success branch or fail the test with a clear message. */
function expectOk(
  result: ReturnType<typeof parseFrontMatter>,
): FrontMatterParseSuccess {
  if (!result.ok) {
    throw new Error(
      `expected a successful parse, got failure: ${result.error}`,
    );
  }
  return result;
}

/** Narrow to the failure branch or fail the test with a clear message. */
function expectFail(
  result: ReturnType<typeof parseFrontMatter>,
): FrontMatterParseFailure {
  if (result.ok) {
    throw new Error(
      `expected a parse failure, got success: ${JSON.stringify(result.value)}`,
    );
  }
  return result;
}

/** Assert the whole value tree contains only plain JS types (no Date etc.). */
function assertPlain(value: FrontMatterValue): void {
  if (value === null) return;
  const t = typeof value;
  if (t === "string" || t === "number" || t === "boolean") return;
  if (Array.isArray(value)) {
    for (const v of value) assertPlain(v);
    return;
  }
  if (t === "object") {
    // A plain object has Object.prototype (or null) as its prototype — a Date
    // or other library type would fail this check.
    const proto = Object.getPrototypeOf(value);
    if (proto !== Object.prototype && proto !== null) {
      throw new Error(
        `non-plain object in tree: ${Object.prototype.toString.call(value)}`,
      );
    }
    for (const v of Object.values(value)) assertPlain(v);
    return;
  }
  throw new Error(`non-plain value in tree: ${t}`);
}

describe("AC-1: valid YAML parses to a plain JS value tree", () => {
  test("nested maps, lists, and scalars (numbers, booleans, null)", () => {
    const yaml = [
      "title: Hello",
      "count: 3",
      "ratio: 2.5",
      "enabled: true",
      "disabled: false",
      "empty: null",
      "tilde: ~",
      "nested:",
      "  host: localhost",
      "  port: 8080",
      "list:",
      "  - a",
      "  - b",
      "  - 1",
    ].join("\n");

    const r = expectOk(parseFrontMatter(yaml, "yaml"));
    const v = r.value as { [k: string]: FrontMatterValue };

    expect(v.title).toBe("Hello");
    expect(v.count).toBe(3);
    expect(v.ratio).toBe(2.5);
    expect(v.enabled).toBe(true);
    expect(v.disabled).toBe(false);
    expect(v.empty).toBe(null);
    expect(v.tilde).toBe(null);
    expect(v.nested).toEqual({ host: "localhost", port: 8080 });
    expect(v.list).toEqual(["a", "b", 1]);
    assertPlain(r.value);
  });
});

describe("AC-2: valid TOML parses to a plain JS value tree (dates normalized)", () => {
  test("tables, arrays, key-values, and a date value contain only plain types", () => {
    const toml = [
      'title = "Hello"',
      "port = 8080",
      "ratio = 2.5",
      "enabled = true",
      'tags = ["a", "b"]',
      "",
      "[server]",
      'host = "localhost"',
      "ports = [80, 443]",
      "",
      "[owner]",
      'name = "Alice"',
      "dob = 1979-05-27",
    ].join("\n");

    const r = expectOk(parseFrontMatter(toml, "toml"));
    const v = r.value as { [k: string]: FrontMatterValue };

    expect(v.title).toBe("Hello");
    expect(v.port).toBe(8080);
    expect(v.ratio).toBe(2.5);
    expect(v.enabled).toBe(true);
    expect(v.tags).toEqual(["a", "b"]);
    expect(v.server).toEqual({ host: "localhost", ports: [80, 443] });

    const owner = v.owner as { [k: string]: FrontMatterValue };
    expect(owner.name).toBe("Alice");
    // The date value is normalized to a plain string, not a Date instance.
    expect(typeof owner.dob).toBe("string");
    expect(owner.dob).toBe("1979-05-27");

    // The whole tree must be free of library-specific value types.
    assertPlain(r.value);
  });

  test("an offset date-time is normalized to a plain ISO string", () => {
    const r = expectOk(parseFrontMatter("when = 2026-07-04T12:30:00Z", "toml"));
    const v = r.value as { [k: string]: FrontMatterValue };
    expect(typeof v.when).toBe("string");
    expect(v.when).toBe("2026-07-04T12:30:00.000Z");
    assertPlain(r.value);
  });
});

describe("AC-3: valid JSON parses to the corresponding value tree", () => {
  test("object with nested array, boolean, and null", () => {
    const json =
      '{"title": "Hello", "count": 3, "nested": {"a": [1, 2, 3]}, "flag": true, "nil": null}';
    const r = expectOk(parseFrontMatter(json, "json"));
    const v = r.value as { [k: string]: FrontMatterValue };

    expect(v.title).toBe("Hello");
    expect(v.count).toBe(3);
    expect(v.nested).toEqual({ a: [1, 2, 3] });
    expect(v.flag).toBe(true);
    expect(v.nil).toBe(null);
    assertPlain(r.value);
  });
});

describe("AC-4: broken content yields a failure with a non-empty message", () => {
  test("broken YAML is captured as a failure, never thrown", () => {
    let r: ReturnType<typeof parseFrontMatter> | undefined;
    expect(() => {
      r = parseFrontMatter("title: [1, 2", "yaml");
    }).not.toThrow();
    const fail = expectFail(r!);
    expect(typeof fail.error).toBe("string");
    expect(fail.error.length).toBeGreaterThan(0);
  });

  test("broken TOML is captured as a failure, never thrown", () => {
    let r: ReturnType<typeof parseFrontMatter> | undefined;
    expect(() => {
      r = parseFrontMatter("this is not = = toml", "toml");
    }).not.toThrow();
    const fail = expectFail(r!);
    expect(typeof fail.error).toBe("string");
    expect(fail.error.length).toBeGreaterThan(0);
  });

  test("broken JSON is captured as a failure, never thrown", () => {
    let r: ReturnType<typeof parseFrontMatter> | undefined;
    expect(() => {
      r = parseFrontMatter('{"title": "Hello",}', "json");
    }).not.toThrow();
    const fail = expectFail(r!);
    expect(typeof fail.error).toBe("string");
    expect(fail.error.length).toBeGreaterThan(0);
  });
});

describe("AC-5: empty content yields a defined, non-throwing success", () => {
  test("empty YAML content is a successful null tree", () => {
    const r = expectOk(parseFrontMatter("", "yaml"));
    expect(r.value).toBe(null);
  });

  test("empty TOML content is a successful null tree", () => {
    const r = expectOk(parseFrontMatter("", "toml"));
    expect(r.value).toBe(null);
  });

  test("empty JSON content is a successful null tree", () => {
    const r = expectOk(parseFrontMatter("", "json"));
    expect(r.value).toBe(null);
  });

  test("whitespace-only content is treated as an empty success", () => {
    const r = expectOk(parseFrontMatter("   \n  \t\n", "yaml"));
    expect(r.value).toBe(null);
  });
});

describe("AC-5 (task0006): cyclic YAML aliases complete with a defined, safe result", () => {
  test("a self-referential alias parses to a defined result without hang or throw", () => {
    // `yaml` resolves this anchor/alias into a genuinely cyclic JS object
    // (v.a.b === v.a). Normalization must not chase the cycle into stack
    // exhaustion; it must terminate with a defined, non-throwing result.
    const src = "a: &x\n  b: *x\n";
    let r: ReturnType<typeof parseFrontMatter> | undefined;
    const start = Date.now();
    expect(() => {
      r = parseFrontMatter(src, "yaml");
    }).not.toThrow();
    expect(Date.now() - start).toBeLessThan(2000);
    // A defined result: success with a safe placeholder, or a failure — never
    // undefined and never a hang.
    expect(r).toBeDefined();
    expect(typeof r!.ok).toBe("boolean");
  });

  test("the cycle is replaced by a safe placeholder, not infinite structure", () => {
    const src = "a: &x\n  b: *x\n";
    const r = parseFrontMatter(src, "yaml");
    const ok = expectOk(r);
    const a = (ok.value as { [k: string]: FrontMatterValue }).a as {
      [k: string]: FrontMatterValue;
    };
    // The back-reference to an ancestor becomes a bounded placeholder string.
    expect(a.b).toBe("[Circular]");
  });

  test("shared but acyclic aliases are preserved, not collapsed to a placeholder", () => {
    // `b`/`c` alias the same array as `a`, but nothing references an ancestor,
    // so the ancestor-only cycle guard must keep every copy intact.
    const src = "a: &x [1, 2, 3]\nb: *x\nc: *x\n";
    const r = parseFrontMatter(src, "yaml");
    const ok = expectOk(r);
    const v = ok.value as { [k: string]: FrontMatterValue };
    expect(v.a).toEqual([1, 2, 3]);
    expect(v.b).toEqual([1, 2, 3]);
    expect(v.c).toEqual([1, 2, 3]);
  });
});

/** Total number of values in a normalized tree (root + every descendant). */
function countNodes(value: FrontMatterValue): number {
  if (value === null || typeof value !== "object") return 1;
  let n = 1;
  if (Array.isArray(value)) {
    for (const v of value) n += countNodes(v);
  } else {
    for (const v of Object.values(value)) n += countNodes(v);
  }
  return n;
}

/** A JSON object literal with `count` shallow numeric keys (no aliases). */
function wideJson(count: number): string {
  const parts: string[] = [];
  for (let i = 0; i < count; i++) parts.push(`"k${i}": ${i}`);
  return `{${parts.join(",")}}`;
}

describe("AC-3 (task0007): normalization respects the shared node budget at parse time", () => {
  test("AC-3: a wide/flat object beyond the budget stops early and flags truncation", () => {
    // A shallow but very wide mapping: no aliases, so the yaml/JSON parsers'
    // own guards never fire — only the normalization budget can bound the copy.
    const r = parseFrontMatter(wideJson(MAX_NODES + 500), "json");
    const ok = expectOk(r);

    // The result carries a parse-time truncation signal.
    expect(ok.truncated).toBe(true);

    // No full-size copy: the produced value holds at most budget-many nodes
    // (root + one per copied child), never the whole over-budget input.
    expect(countNodes(ok.value)).toBeLessThanOrEqual(MAX_NODES + 1);
    const obj = ok.value as { [k: string]: FrontMatterValue };
    expect(Object.keys(obj).length).toBeLessThan(MAX_NODES + 500);
    expect(Object.keys(obj).length).toBeLessThanOrEqual(MAX_NODES);
  });

  test("AC-3: nested breadth is bounded too, not just top-level keys", () => {
    // Each top-level key holds a one-key child object; the total copied nodes
    // (keys + children) must still stop at the budget.
    const parts: string[] = [];
    for (let i = 0; i < MAX_NODES; i++) parts.push(`"k${i}": {"c": ${i}}`);
    const r = parseFrontMatter(`{${parts.join(",")}}`, "json");
    const ok = expectOk(r);
    expect(ok.truncated).toBe(true);
    expect(countNodes(ok.value)).toBeLessThanOrEqual(MAX_NODES + 1);
  });

  test("AC-3: a within-budget object is copied whole with no truncation signal", () => {
    const r = parseFrontMatter(wideJson(10), "json");
    const ok = expectOk(r);
    expect(ok.truncated).toBeFalsy();
    const obj = ok.value as { [k: string]: FrontMatterValue };
    expect(Object.keys(obj).length).toBe(10);
    expect(obj.k9).toBe(9);
  });

  test("AC-3: an exactly-at-budget object is complete, not truncated", () => {
    const r = parseFrontMatter(wideJson(MAX_NODES), "json");
    const ok = expectOk(r);
    expect(ok.truncated).toBeFalsy();
    const obj = ok.value as { [k: string]: FrontMatterValue };
    expect(Object.keys(obj).length).toBe(MAX_NODES);
  });

  test("AC-3: the budget and the cycle guard coexist ([Circular] still survives)", () => {
    // The same self-referential alias as the cycle tests above, now flowing
    // through the budgeted normalizer: the ancestor back-reference must still
    // collapse to the placeholder rather than spending budget chasing a cycle.
    const r = parseFrontMatter("a: &x\n  b: *x\n", "yaml");
    const ok = expectOk(r);
    const a = (ok.value as { [k: string]: FrontMatterValue }).a as {
      [k: string]: FrontMatterValue;
    };
    expect(a.b).toBe("[Circular]");
    expect(ok.truncated).toBeFalsy();
  });
});

describe("AC-2 (task0008): the parse layer no longer depends on the view tree-builder", () => {
  test("AC-2: parser.ts sources the node budget from the neutral limits module", async () => {
    const src = await Bun.file(new URL("./parser.ts", import.meta.url)).text();
    // The shared budget is imported from the neutral limits module...
    expect(src).toMatch(/from "\.\/limits\.ts"/);
    // ...and NOT from the view-layer tree-builder (the reversed dependency the
    // finding flagged). The parse layer must not inherit a display-tree import.
    expect(src).not.toMatch(/from "\.\/tree-builder\.ts"/);
  });

  test("AC-2: the budget value is unchanged (2000)", () => {
    expect(MAX_NODES).toBe(2000);
  });
});
