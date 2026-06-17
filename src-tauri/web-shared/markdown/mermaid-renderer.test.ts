/**
 * Tests for MermaidRenderer.
 */
import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

// Mock mermaid module
const mockRender = mock(
	async (id: string, source: string): Promise<{ svg: string }> => ({
		svg: `<svg id="${id}"><text>${source}</text></svg>`,
	}),
);
const mockInitialize = mock((_config: Record<string, unknown>) => {});

mock.module("mermaid", () => ({
	default: {
		initialize: mockInitialize,
		render: mockRender,
	},
}));

const { MermaidRenderer } = await import("./mermaid-renderer.ts");

describe("MermaidRenderer", () => {
	let container: HTMLElement;

	beforeEach(() => {
		container = document.createElement("div");
		document.body.appendChild(container);
		mockRender.mockClear();
		mockInitialize.mockClear();
	});

	afterEach(() => {
		container.remove();
	});

	describe("detection", () => {
		test("should detect mermaid code blocks (TS-07)", () => {
			container.innerHTML =
				'<pre><code class="hljs language-mermaid">graph TD\n  A-->B</code></pre>';
			const blocks = container.querySelectorAll(
				"pre > code.language-mermaid, pre > code.hljs.language-mermaid",
			);
			expect(blocks.length).toBe(1);
		});

		test("should not detect non-mermaid code blocks", () => {
			container.innerHTML =
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>';
			const blocks = container.querySelectorAll(
				"pre > code.language-mermaid, pre > code.hljs.language-mermaid",
			);
			expect(blocks.length).toBe(0);
		});

		test("should detect multiple mermaid blocks", () => {
			container.innerHTML = `
				<pre><code class="hljs language-mermaid">graph TD\n  A-->B</code></pre>
				<pre><code class="hljs language-javascript">const x = 1;</code></pre>
				<pre><code class="hljs language-mermaid">sequenceDiagram\n  A->>B: Hello</code></pre>
			`;
			const blocks = container.querySelectorAll(
				"pre > code.language-mermaid, pre > code.hljs.language-mermaid",
			);
			expect(blocks.length).toBe(2);
		});
	});

	describe("no-op behavior", () => {
		test("should not load mermaid when no mermaid blocks exist (TS-08)", async () => {
			container.innerHTML = "<p>No code blocks here</p>";
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			expect(mockInitialize).not.toHaveBeenCalled();
			expect(mockRender).not.toHaveBeenCalled();
		});

		test("should not load mermaid when only non-mermaid code blocks exist", async () => {
			container.innerHTML =
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>';
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			expect(mockInitialize).not.toHaveBeenCalled();
			expect(mockRender).not.toHaveBeenCalled();
		});
	});

	describe("rendering", () => {
		test("should render mermaid block with toolbar", async () => {
			container.innerHTML =
				'<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>';
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			// Should create mermaid-block
			const mermaidBlock = container.querySelector(".mermaid-block");
			expect(mermaidBlock).not.toBeNull();

			// Diagram should be rendered
			const mermaidDiagram = container.querySelector(".mermaid-diagram");
			expect(mermaidDiagram).not.toBeNull();
			expect(mermaidDiagram?.querySelector("svg")).not.toBeNull();

			// Source should be preserved
			const mermaidSource = container.querySelector(".mermaid-source");
			expect(mermaidSource).not.toBeNull();
			expect(mermaidSource?.querySelector("pre > code")).not.toBeNull();

			// Toolbar with Chart/Code icon buttons and Copy text button
			const toolbar = container.querySelector(".mermaid-toolbar");
			expect(toolbar).not.toBeNull();
			const viewBtns = container.querySelectorAll(".mermaid-view-btn");
			expect(viewBtns.length).toBe(2);
			const copyBtn = toolbar?.querySelector(".copy-code-button");
			expect(copyBtn).not.toBeNull();
		});

		test("should store source as data attribute for copy", async () => {
			container.innerHTML =
				'<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>';
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			const mermaidBlock = container.querySelector(".mermaid-block");
			expect(mermaidBlock?.getAttribute("data-mermaid-source")).toContain("graph TD");
		});

		test("should apply dark theme and strict security (TS-10)", async () => {
			container.innerHTML =
				'<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>';
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			expect(mockInitialize).toHaveBeenCalledWith(
				expect.objectContaining({
					theme: "dark",
					securityLevel: "strict",
					themeVariables: expect.objectContaining({
						darkMode: true,
					}),
				}),
			);
		});

		test("should render multiple mermaid blocks", async () => {
			container.innerHTML = `
				<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>
				<pre><code class="hljs language-mermaid">sequenceDiagram\n  A-&gt;&gt;B: Hello</code></pre>
			`;
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			const blocks = container.querySelectorAll(".mermaid-block");
			expect(blocks.length).toBe(2);
			const diagrams = container.querySelectorAll(".mermaid-diagram");
			expect(diagrams.length).toBe(2);
		});

		test("should initialize mermaid only once for multiple blocks", async () => {
			container.innerHTML = `
				<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>
				<pre><code class="hljs language-mermaid">graph LR\n  C--&gt;D</code></pre>
			`;
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			expect(mockInitialize).toHaveBeenCalledTimes(1);
		});

		test("should toggle between diagram and code views", async () => {
			container.innerHTML =
				'<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>';
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			const block = container.querySelector(".mermaid-block") as HTMLElement;
			const codeBtn = container.querySelector('.mermaid-view-btn[data-mode="code"]') as HTMLElement;
			const diagramContainer = container.querySelector(".mermaid-diagram") as HTMLElement;
			const sourceContainer = container.querySelector(".mermaid-source") as HTMLElement;

			// Default: diagram visible, source hidden
			expect(block.dataset.view).toBe("diagram");
			expect(diagramContainer.style.display).toBe("");
			expect(sourceContainer.style.display).toBe("none");

			// Click code button
			codeBtn.click();

			expect(block.dataset.view).toBe("code");
			expect(diagramContainer.style.display).toBe("none");
			expect(sourceContainer.style.display).toBe("");
		});
	});

	describe("error handling", () => {
		test("should preserve original code block on render error (TS-09)", async () => {
			// Make render fail
			mockRender.mockImplementationOnce(() => {
				throw new Error("Invalid syntax");
			});

			container.innerHTML =
				'<pre><code class="hljs language-mermaid">invalid mermaid syntax</code></pre>';
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			// Original code block should remain (no mermaid-block wrapper)
			const mermaidBlock = container.querySelector(".mermaid-block");
			expect(mermaidBlock).toBeNull();

			const codeBlock = container.querySelector(
				"pre > code.language-mermaid, pre > code.hljs.language-mermaid",
			);
			expect(codeBlock).not.toBeNull();
			expect(codeBlock?.textContent).toContain("invalid mermaid syntax");
		});

		test("should continue rendering other blocks after one fails", async () => {
			// First call fails, second succeeds
			mockRender
				.mockImplementationOnce(() => {
					throw new Error("Invalid syntax");
				})
				.mockImplementationOnce(
					async (id: string, source: string) => ({
						svg: `<svg id="${id}"><text>${source}</text></svg>`,
					}),
				);

			container.innerHTML = `
				<pre><code class="hljs language-mermaid">invalid syntax</code></pre>
				<pre><code class="hljs language-mermaid">graph TD\n  A--&gt;B</code></pre>
			`;
			const renderer = new MermaidRenderer();
			await renderer.renderAll(container);

			// First block should remain as code (no wrapper)
			const mermaidBlocks = container.querySelectorAll(".mermaid-block");
			expect(mermaidBlocks.length).toBe(1);

			// Second block should be rendered with toggle
			const diagrams = container.querySelectorAll(".mermaid-diagram");
			expect(diagrams.length).toBe(1);
		});
	});
});
