/**
 * Tests for OutlinePanel.
 */
import { afterEach, beforeEach, describe, expect, test } from "bun:test";

// Polyfill IntersectionObserver for happy-dom
if (typeof globalThis.IntersectionObserver === "undefined") {
	globalThis.IntersectionObserver = class IntersectionObserver {
		readonly root: Element | null = null;
		readonly rootMargin: string = "";
		readonly thresholds: ReadonlyArray<number> = [];
		constructor(
			_callback: IntersectionObserverCallback,
			_options?: IntersectionObserverInit,
		) {}
		observe(_target: Element): void {}
		unobserve(_target: Element): void {}
		disconnect(): void {}
		takeRecords(): IntersectionObserverEntry[] {
			return [];
		}
	} as unknown as typeof IntersectionObserver;
}

import { OutlinePanel } from "./outline.ts";

describe("OutlinePanel", () => {
	let contentEl: HTMLElement;

	beforeEach(() => {
		contentEl = document.createElement("div");
		contentEl.className = "markdown-fullscreen-content";
		document.body.appendChild(contentEl);
	});

	afterEach(() => {
		contentEl.remove();
	});

	describe("heading extraction", () => {
		test("should extract h1-h3 headings correctly (TS-01)", () => {
			contentEl.innerHTML = `
				<h1>Title</h1>
				<p>Some text</p>
				<h2>Section 1</h2>
				<h3>Subsection 1.1</h3>
				<h2>Section 2</h2>
			`;
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			expect(result).not.toBeNull();
			// Panel should contain outline items
			const items = result!.querySelectorAll(".outline-item");
			expect(items.length).toBe(4); // h1 + h2 + h3 + h2
		});

		test("should ignore h4-h6 headings (TS-04)", () => {
			contentEl.innerHTML = `
				<h1>Title</h1>
				<h4>Sub-subsection</h4>
				<h5>Deep heading</h5>
				<h6>Deepest heading</h6>
			`;
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			expect(result).not.toBeNull();
			const items = result!.querySelectorAll(".outline-item");
			expect(items.length).toBe(1); // Only h1
		});

		test("should return null when no headings exist (TS-03)", () => {
			contentEl.innerHTML = "<p>No headings here</p>";
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			expect(result).toBeNull();
		});

		test("should return null when only h4-h6 headings exist", () => {
			contentEl.innerHTML = `
				<h4>Sub heading</h4>
				<h5>Deep heading</h5>
			`;
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			expect(result).toBeNull();
		});

		test("should assign IDs to headings without IDs (TS-05)", () => {
			contentEl.innerHTML = `
				<h1>Title Without ID</h1>
				<h2 id="existing-id">Has ID</h2>
			`;
			const panel = new OutlinePanel();
			panel.build(contentEl);

			const h1 = contentEl.querySelector("h1");
			expect(h1?.id).toBeTruthy();
			expect(h1?.id).toMatch(/^heading-/);

			// Existing ID should be preserved
			const h2 = contentEl.querySelector("h2");
			expect(h2?.id).toBe("existing-id");
		});
	});

	describe("tree hierarchy", () => {
		test("should build correct tree hierarchy (TS-02)", () => {
			contentEl.innerHTML = `
				<h1>Title</h1>
				<h2>Section</h2>
				<h3>Subsection</h3>
			`;
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);
			expect(result).not.toBeNull();

			const items = result!.querySelectorAll(".outline-item");
			expect(items.length).toBe(3);

			// Check indentation via data attributes or classes
			expect(items[0]?.getAttribute("data-level")).toBe("1");
			expect(items[1]?.getAttribute("data-level")).toBe("2");
			expect(items[2]?.getAttribute("data-level")).toBe("3");
		});
	});

	describe("click navigation", () => {
		test("should set up click handler for smooth scroll (TS-12)", () => {
			contentEl.innerHTML = `
				<h1 id="title">Title</h1>
				<p>Some text</p>
				<h2 id="section1">Section 1</h2>
			`;
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);
			expect(result).not.toBeNull();

			const items = result!.querySelectorAll(".outline-item");
			expect(items.length).toBe(2);

			// Click on first item - should not throw
			(items[0] as HTMLElement).click();
		});
	});

	describe("dispose", () => {
		test("should clean up IntersectionObserver on dispose (TS-06)", () => {
			contentEl.innerHTML = `
				<h1>Title</h1>
				<h2>Section</h2>
			`;
			const panel = new OutlinePanel();
			panel.build(contentEl);

			// Dispose should not throw
			panel.dispose();

			// Building again after dispose should work
			const result = panel.build(contentEl);
			expect(result).not.toBeNull();
			panel.dispose();
		});

		test("should handle dispose when not built", () => {
			const panel = new OutlinePanel();
			// Dispose without build should not throw
			panel.dispose();
		});
	});

	describe("outline panel DOM", () => {
		test("should have correct class name", () => {
			contentEl.innerHTML = "<h1>Title</h1>";
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			expect(result?.className).toBe("markdown-outline-panel");
		});

		test("should include heading text in outline items", () => {
			contentEl.innerHTML = `
				<h1>My Title</h1>
				<h2>My Section</h2>
			`;
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			const items = result!.querySelectorAll(".outline-item");
			expect(items[0]?.textContent).toBe("My Title");
			expect(items[1]?.textContent).toBe("My Section");
		});

		test("should have aria attributes for accessibility", () => {
			contentEl.innerHTML = "<h1>Title</h1>";
			const panel = new OutlinePanel();
			const result = panel.build(contentEl);

			expect(result?.getAttribute("role")).toBe("navigation");
			expect(result?.getAttribute("aria-label")).toBeTruthy();
		});
	});
});
