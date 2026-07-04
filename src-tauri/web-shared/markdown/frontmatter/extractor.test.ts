/**
 * Unit tests for the front matter extractor (task0001).
 *
 * Pure-function tests (no DOM). Each test names the Acceptance Criterion it
 * proves. Covers SPEC.md TS-1 (extraction half), TS-3, TS-4, TS-5, and the
 * edge cases (BOM / CRLF / empty block / front-matter-only file).
 */

import { describe, expect, test } from "bun:test";

import { extractFrontMatter } from "./extractor.ts";
import type { FrontMatterExtracted, NoFrontMatter } from "./types.ts";

/** Narrow to the "found" branch or fail the test with a clear message. */
function expectExtracted(
  result: ReturnType<typeof extractFrontMatter>,
): FrontMatterExtracted {
  if (!result.found) {
    throw new Error(`expected a front matter extraction, got: none`);
  }
  return result;
}

/** Narrow to the "none" branch or fail the test with a clear message. */
function expectNone(
  result: ReturnType<typeof extractFrontMatter>,
): NoFrontMatter {
  if (result.found) {
    throw new Error(
      `expected no front matter, got a ${result.format} extraction`,
    );
  }
  return result;
}

describe("AC-1: YAML front matter (---)", () => {
  test("extracts content between delimiters and strips the block from the body", () => {
    const source = "---\ntitle: Hello\ncount: 3\n---\n# Body\n";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("title: Hello\ncount: 3\n");
    expect(r.body).toBe("# Body\n");
    expect(r.body).not.toContain("---");
    expect(r.body).not.toContain("title: Hello");
  });

  test("tolerates trailing whitespace on the delimiter lines", () => {
    const source = "---  \nkey: val\n---\t\nbody";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("key: val\n");
    expect(r.body).toBe("body");
  });
});

describe("AC-2: TOML front matter (+++)", () => {
  test("the same shape with +++ yields a TOML extraction", () => {
    const source = '+++\ntitle = "Hello"\n+++\n# Body\n';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("toml");
    expect(r.raw).toBe('title = "Hello"\n');
    expect(r.body).toBe("# Body\n");
  });
});

describe("AC-3: JSON front matter (brace balancing)", () => {
  test("detects a bare JSON object and starts the body after the block", () => {
    const source = '{"title": "Hello", "count": 3}\n# Body\n';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"title": "Hello", "count": 3}');
    expect(r.body).toBe("# Body\n");
  });

  test("nested objects balance to the matching outer brace", () => {
    const source = '{"a": {"b": {"c": 1}}, "d": 2}\nbody';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"a": {"b": {"c": 1}}, "d": 2}');
    expect(r.body).toBe("body");
  });

  test("braces inside string literals do not affect balancing", () => {
    const source = '{"expr": "a{b}c", "close": "}"}\nbody';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"expr": "a{b}c", "close": "}"}');
    expect(r.body).toBe("body");
  });

  test("escaped quotes inside a string literal are honored", () => {
    const source = '{"quote": "she said \\"}{\\" loudly"}\nbody';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"quote": "she said \\"}{\\" loudly"}');
    expect(r.body).toBe("body");
  });

  test("an escaped backslash before a quote does not escape the quote", () => {
    // "path": "C:\\"  — the backslash-backslash is a literal backslash, so the
    // following quote CLOSES the string; the next } closes the object.
    const source = '{"path": "C:\\\\"}\nbody';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"path": "C:\\\\"}');
    expect(r.body).toBe("body");
  });
});

describe("AC-4: no front matter, source unmodified", () => {
  test("body text on the first line", () => {
    const source = "# Just a heading\n\nSome body text.\n";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("a blank first line before a --- block is not front matter", () => {
    const source = "\n---\ntitle: Hello\n---\nbody";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("--- appearing only mid-document is not front matter", () => {
    const source = "intro paragraph\n---\nnot front matter\n";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("*** on the first line is not a front matter delimiter", () => {
    const source = "***\nkey: val\n***\nbody";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("no-front-matter body is reference-identical to the source (FR7 fast path)", () => {
    const source = "plain document with no front matter\n";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
    // The fast path returns the exact same string reference, not a copy.
    expect(Object.is(r.body, source)).toBe(true);
  });
});

describe("AC-5: unterminated blocks are not front matter", () => {
  test("--- with no closing delimiter", () => {
    const source = "---\ntitle: Hello\nnever closed\n";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("+++ with no closing delimiter", () => {
    const source = '+++\ntitle = "Hello"\n';
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("unbalanced opening brace", () => {
    const source = '{"a": {"b": 1}\nbody';
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("a brace closed only inside a string literal stays unbalanced", () => {
    const source = '{"a": "}"\nbody';
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });

  test("an opening delimiter with no following newline is not front matter", () => {
    const source = "---";
    const r = expectNone(extractFrontMatter(source));
    expect(r.body).toBe(source);
  });
});

describe("AC-6: BOM and CRLF handling", () => {
  test("a UTF-8 BOM before --- does not defeat detection", () => {
    const source = "﻿---\ntitle: Hello\n---\n# Body\n";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("title: Hello\n");
    expect(r.body).toBe("# Body\n");
  });

  test("a UTF-8 BOM before { does not defeat JSON detection", () => {
    const source = '﻿{"title": "Hello"}\nbody';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"title": "Hello"}');
    expect(r.body).toBe("body");
  });

  test("CRLF line endings are detected and stripped correctly", () => {
    const source = "---\r\ntitle: Hello\r\n---\r\n# Body\r\n";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("title: Hello\r\n");
    expect(r.body).toBe("# Body\r\n");
  });

  test("CRLF after a JSON block strips both carriage return and newline", () => {
    const source = '{"a": 1}\r\nbody';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"a": 1}');
    expect(r.body).toBe("body");
  });
});

describe("AC-7: empty block", () => {
  test("--- immediately followed by --- yields empty raw content", () => {
    const source = "---\n---\n# Body\n";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("");
    expect(r.body).toBe("# Body\n");
  });

  test("empty JSON object {} is a valid block with braces as raw", () => {
    const source = "{}\nbody";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe("{}");
    expect(r.body).toBe("body");
  });
});

describe("AC-8: front-matter-only document (empty body)", () => {
  test("YAML block with a trailing newline and no body", () => {
    const source = "---\ntitle: Hello\n---\n";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("title: Hello\n");
    expect(r.body).toBe("");
  });

  test("YAML block with no trailing newline after the closing delimiter", () => {
    const source = "---\ntitle: Hello\n---";
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("yaml");
    expect(r.raw).toBe("title: Hello\n");
    expect(r.body).toBe("");
  });

  test("JSON-only document leaves an empty body", () => {
    const source = '{"only": true}';
    const r = expectExtracted(extractFrontMatter(source));

    expect(r.format).toBe("json");
    expect(r.raw).toBe('{"only": true}');
    expect(r.body).toBe("");
  });
});
