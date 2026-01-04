/**
 * Security tests for Markdown rendering.
 *
 * Comprehensive XSS prevention tests.
 */
import { describe, test, expect, beforeEach, afterEach } from "bun:test";
import { MarkdownRenderer } from "./renderer.ts";

describe("Markdown Security", () => {
  let renderer: MarkdownRenderer;

  beforeEach(() => {
    renderer = new MarkdownRenderer();
  });

  afterEach(() => {
    renderer.dispose();
  });

  describe("XSS prevention - script injection", () => {
    test("should block XSS via script tag", () => {
      const html = renderer.render(
        "<script>alert('xss')</script>",
        "commonmark",
      );
      expect(html).not.toContain("<script");
      expect(html).not.toContain("alert(");
    });

    test("should block XSS via script tag with attributes", () => {
      const html = renderer.render(
        '<script type="text/javascript">evil()</script>',
        "commonmark",
      );
      expect(html).not.toContain("<script");
    });

    test("should block XSS via noscript tag", () => {
      const html = renderer.render(
        "<noscript><div>content</div></noscript>",
        "commonmark",
      );
      // noscript should be allowed but content should be sanitized
      expect(html).not.toContain("<noscript");
    });
  });

  describe("XSS prevention - event handlers", () => {
    test("should block XSS via onclick", () => {
      const html = renderer.render(
        '<div onclick="alert(1)">click</div>',
        "commonmark",
      );
      expect(html).not.toContain("onclick");
    });

    test("should block XSS via onerror", () => {
      const html = renderer.render(
        '<img src="x" onerror="alert(1)">',
        "commonmark",
      );
      expect(html).not.toContain("onerror");
    });

    test("should block XSS via onload", () => {
      const html = renderer.render(
        '<img src="x" onload="alert(1)">',
        "commonmark",
      );
      expect(html).not.toContain("onload");
    });

    test("should block XSS via onmouseover", () => {
      const html = renderer.render(
        '<div onmouseover="alert(1)">hover</div>',
        "commonmark",
      );
      expect(html).not.toContain("onmouseover");
    });

    test("should block XSS via onfocus", () => {
      const html = renderer.render('<input onfocus="alert(1)">', "commonmark");
      expect(html).not.toContain("onfocus");
    });

    test("should block all event handlers", () => {
      const events = [
        "onabort",
        "onblur",
        "onchange",
        "ondblclick",
        "onerror",
        "onfocus",
        "oninput",
        "onkeydown",
        "onkeypress",
        "onkeyup",
        "onload",
        "onmousedown",
        "onmousemove",
        "onmouseout",
        "onmouseover",
        "onmouseup",
        "onreset",
        "onscroll",
        "onselect",
        "onsubmit",
        "onunload",
      ];

      for (const event of events) {
        const html = renderer.render(
          `<div ${event}="alert(1)">test</div>`,
          "commonmark",
        );
        expect(html).not.toContain(event);
      }
    });
  });

  describe("XSS prevention - dangerous URLs", () => {
    test("should block XSS via javascript: URL in href", () => {
      const html = renderer.render(
        "[click](javascript:alert(1))",
        "commonmark",
      );
      expect(html).not.toContain("javascript:");
    });

    test("should block XSS via javascript: URL with encoding", () => {
      const html = renderer.render(
        "[click](java&#x73;cript:alert(1))",
        "commonmark",
      );
      expect(html).not.toContain("javascript:");
    });

    test("should block XSS via data: URL with scripts", () => {
      const html = renderer.render(
        "[click](data:text/html,<script>alert(1)</script>)",
        "commonmark",
      );
      // data: URLs should be blocked or the script content should be sanitized
      if (html.includes("data:")) {
        expect(html).not.toContain("<script");
      }
    });

    test("should block XSS via vbscript: URL", () => {
      const html = renderer.render("[click](vbscript:msgbox(1))", "commonmark");
      expect(html).not.toContain("vbscript:");
    });

    test("should allow safe URLs", () => {
      const safeUrls = [
        "https://example.com",
        "http://example.com",
        "mailto:test@example.com",
        "/relative/path",
        "#anchor",
      ];

      for (const url of safeUrls) {
        const html = renderer.render(`[link](${url})`, "commonmark");
        expect(html).toContain("href=");
      }
    });
  });

  describe("XSS prevention - dangerous tags", () => {
    test("should block iframe tags", () => {
      const html = renderer.render(
        '<iframe src="https://evil.com"></iframe>',
        "commonmark",
      );
      expect(html).not.toContain("<iframe");
    });

    test("should block object tags", () => {
      const html = renderer.render(
        '<object data="evil.swf"></object>',
        "commonmark",
      );
      expect(html).not.toContain("<object");
    });

    test("should block embed tags", () => {
      const html = renderer.render('<embed src="evil.swf">', "commonmark");
      expect(html).not.toContain("<embed");
    });

    test("should block form tags", () => {
      const html = renderer.render(
        '<form action="evil.com"></form>',
        "commonmark",
      );
      expect(html).not.toContain("<form");
    });

    test("should block style tags", () => {
      const html = renderer.render(
        "<style>* { display: none }</style>",
        "commonmark",
      );
      expect(html).not.toContain("<style");
    });

    test("should block svg with script", () => {
      const html = renderer.render(
        "<svg><script>alert(1)</script></svg>",
        "commonmark",
      );
      expect(html).not.toContain("<script");
    });

    test("should block base tags", () => {
      const html = renderer.render(
        '<base href="https://evil.com">',
        "commonmark",
      );
      expect(html).not.toContain("<base");
    });

    test("should block meta tags", () => {
      const html = renderer.render(
        '<meta http-equiv="refresh" content="0;url=evil">',
        "commonmark",
      );
      expect(html).not.toContain("<meta");
    });

    test("should block link tags", () => {
      const html = renderer.render(
        '<link rel="stylesheet" href="evil.css">',
        "commonmark",
      );
      expect(html).not.toContain("<link");
    });
  });

  describe("XSS prevention - dangerous attributes", () => {
    test("should block style attributes with expression", () => {
      // IE CSS expression attack
      const html = renderer.render(
        '<div style="background:expression(alert(1))">test</div>',
        "commonmark",
      );
      expect(html).not.toContain("expression");
    });

    test("should block formaction attribute", () => {
      const html = renderer.render(
        '<button formaction="javascript:alert(1)">click</button>',
        "commonmark",
      );
      expect(html).not.toContain("formaction");
    });

    test("should block srcdoc attribute", () => {
      const html = renderer.render(
        '<iframe srcdoc="<script>alert(1)</script>"></iframe>',
        "commonmark",
      );
      expect(html).not.toContain("srcdoc");
    });
  });

  describe("safe content preservation", () => {
    test("should allow safe heading tags", () => {
      const headings = ["h1", "h2", "h3", "h4", "h5", "h6"];
      for (let i = 1; i <= 6; i++) {
        const html = renderer.render(
          `${"#".repeat(i)} Heading ${i}`,
          "commonmark",
        );
        expect(html).toContain(`<h${i}`);
      }
    });

    test("should allow safe formatting", () => {
      const html = renderer.render("**bold** *italic* ~~strike~~", "gfm");
      expect(html).toContain("<strong>");
      expect(html).toContain("<em>");
      expect(html).toContain("<del>");
    });

    test("should allow safe lists", () => {
      const html = renderer.render(
        "- item 1\n- item 2\n\n1. first\n2. second",
        "commonmark",
      );
      expect(html).toContain("<ul>");
      expect(html).toContain("<ol>");
      expect(html).toContain("<li>");
    });

    test("should allow safe tables", () => {
      const md = "| A | B |\n|---|---|\n| 1 | 2 |";
      const html = renderer.render(md, "gfm");
      expect(html).toContain("<table>");
      expect(html).toContain("<thead>");
      expect(html).toContain("<tbody>");
      expect(html).toContain("<tr>");
      expect(html).toContain("<th>");
      expect(html).toContain("<td>");
    });

    test("should allow safe code blocks", () => {
      const html = renderer.render("```\ncode\n```", "commonmark");
      expect(html).toContain("<pre>");
      expect(html).toContain("<code");
    });

    test("should allow safe blockquotes", () => {
      const html = renderer.render("> quoted text", "commonmark");
      expect(html).toContain("<blockquote>");
    });

    test("should allow safe horizontal rules", () => {
      const html = renderer.render("---", "commonmark");
      expect(html).toContain("<hr");
    });
  });
});
