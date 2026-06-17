/**
 * Tests for MarkdownRenderer.
 */
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { MarkdownRenderer } from "./renderer.ts";

describe("MarkdownRenderer", () => {
	let renderer: MarkdownRenderer;

	beforeEach(() => {
		renderer = new MarkdownRenderer();
	});

	afterEach(() => {
		renderer.dispose();
	});

	describe("render", () => {
		test("should render CommonMark to HTML", () => {
			const html = renderer.render("# Hello World", "commonmark");
			expect(html).toContain("<h1");
			expect(html).toContain("Hello World");
		});

		test("should render GFM to HTML", () => {
			const html = renderer.render("# Hello World\n\n- [x] Task", "gfm");
			expect(html).toContain("<h1");
			expect(html).toContain("Hello World");
		});

		test("should render paragraphs", () => {
			const html = renderer.render("This is a paragraph.", "commonmark");
			expect(html).toContain("<p>");
			expect(html).toContain("This is a paragraph.");
		});

		test("should render code blocks", () => {
			const html = renderer.render("```javascript\nconst x = 1;\n```", "gfm");
			expect(html).toContain("<pre>");
			expect(html).toContain("<code");
		});

		test("should render inline code", () => {
			const html = renderer.render("Use `const` for constants.", "commonmark");
			expect(html).toContain("<code>");
			expect(html).toContain("const");
		});

		test("should render links", () => {
			const html = renderer.render(
				"[Example](https://example.com)",
				"commonmark",
			);
			expect(html).toContain("<a");
			expect(html).toContain("href=");
			expect(html).toContain("https://example.com");
		});

		test("should render lists", () => {
			const html = renderer.render("- Item 1\n- Item 2", "commonmark");
			expect(html).toContain("<ul>");
			expect(html).toContain("<li>");
		});

		test("should render blockquotes", () => {
			const html = renderer.render("> Quote text", "commonmark");
			expect(html).toContain("<blockquote>");
		});

		test("should render tables (GFM)", () => {
			const markdown = `
| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |
`;
			const html = renderer.render(markdown, "gfm");
			expect(html).toContain("<table>");
			expect(html).toContain("<th>");
			expect(html).toContain("<td>");
		});
	});

	describe("security", () => {
		test("should sanitize dangerous HTML - script tags", () => {
			const html = renderer.render(
				"<script>alert('xss')</script>",
				"commonmark",
			);
			expect(html).not.toContain("<script>");
			expect(html).not.toContain("alert");
		});

		test("should remove onclick attributes", () => {
			const html = renderer.render(
				"<div onclick=\"alert('xss')\">Click me</div>",
				"commonmark",
			);
			expect(html).not.toContain("onclick");
		});

		test("should remove onerror attributes", () => {
			const html = renderer.render(
				'<img src="x" onerror="alert(\'xss\')">',
				"commonmark",
			);
			expect(html).not.toContain("onerror");
		});

		test("should remove javascript: URLs in links", () => {
			const html = renderer.render(
				"[Click me](javascript:alert('xss'))",
				"commonmark",
			);
			// DOMPurify should remove or sanitize javascript: URLs
			expect(html).not.toContain("javascript:");
		});

		test("should remove style tags", () => {
			const html = renderer.render(
				"<style>body{display:none}</style>",
				"commonmark",
			);
			expect(html).not.toContain("<style>");
		});

		test("should remove iframe tags", () => {
			const html = renderer.render(
				'<iframe src="https://evil.com"></iframe>',
				"commonmark",
			);
			expect(html).not.toContain("<iframe>");
		});

		test("should preserve safe tags", () => {
			const html = renderer.render("**Bold** and *italic*", "commonmark");
			expect(html).toContain("<strong>");
			expect(html).toContain("<em>");
		});

		test("should preserve safe content", () => {
			const markdown = `
# Heading

This is a paragraph with **bold** text.

- List item 1
- List item 2

[Safe link](https://example.com)
`;
			const html = renderer.render(markdown, "commonmark");
			expect(html).toContain("<h1>");
			expect(html).toContain("<strong>");
			expect(html).toContain("<ul>");
			expect(html).toContain("<a");
		});
	});

	describe("data: URI handling", () => {
		test("should allow data: URI in img src", () => {
			const html = renderer.render(
				'<img src="data:image/png;base64,iVBORw0KGgo=" alt="test">',
				"commonmark",
			);
			expect(html).toContain("data:image/png;base64,iVBORw0KGgo=");
		});

		test("should block data: URI in a href", () => {
			const html = renderer.render(
				'<a href="data:text/html,<script>alert(1)</script>">Click</a>',
				"commonmark",
			);
			expect(html).not.toContain("data:text/html");
		});

		test("should still allow http/https URLs", () => {
			const html = renderer.render(
				'[Example](https://example.com)',
				"commonmark",
			);
			expect(html).toContain("https://example.com");
		});
	});

	describe("local image marking", () => {
		test("should mark local-path images with data-local-src", () => {
			const html = renderer.render(
				'![Alt text](./images/test.png)',
				"commonmark",
			);
			expect(html).toContain('data-local-src="./images/test.png"');
		});

		test("should mark absolute-path images with data-local-src", () => {
			const html = renderer.render(
				'![Alt text](/home/user/images/test.png)',
				"commonmark",
			);
			expect(html).toContain('data-local-src="/home/user/images/test.png"');
		});

		test("should set placeholder src for local images", () => {
			const html = renderer.render(
				'![Alt text](./images/test.png)',
				"commonmark",
			);
			// The img src should be the transparent placeholder, not the original path
			// Use a regex to match only the src attribute (not data-local-src)
			const srcMatch = html.match(/\ssrc="([^"]*)"/);
			expect(srcMatch).not.toBeNull();
			expect(srcMatch![1]).toContain("data:image/gif;base64,");
			expect(srcMatch![1]).not.toBe("./images/test.png");
		});

		test("should not mark http/https images with data-local-src", () => {
			const html = renderer.render(
				'![Alt text](https://example.com/image.png)',
				"commonmark",
			);
			expect(html).not.toContain("data-local-src");
			expect(html).toContain("https://example.com/image.png");
		});

		test("should not mark data: URI images with data-local-src", () => {
			const html = renderer.render(
				'<img src="data:image/png;base64,abc123" alt="test">',
				"commonmark",
			);
			expect(html).not.toContain("data-local-src");
		});
	});

	describe("syntax highlighting", () => {
		test("should highlight JavaScript code", () => {
			const html = renderer.render(
				'```javascript\nconst x = "hello";\n```',
				"gfm",
			);
			// highlight.js adds classes for syntax highlighting
			expect(html).toContain("hljs");
		});

		test("should highlight Python code", () => {
			const html = renderer.render(
				'```python\ndef hello():\n    print("Hello")\n```',
				"gfm",
			);
			expect(html).toContain("hljs");
		});

		test("should handle unknown languages gracefully", () => {
			const html = renderer.render("```unknownlang\nsome code\n```", "gfm");
			// Should still render without crashing
			expect(html).toContain("<pre>");
			expect(html).toContain("<code");
		});
	});
});
