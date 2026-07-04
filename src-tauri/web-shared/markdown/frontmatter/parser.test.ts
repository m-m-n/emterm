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
