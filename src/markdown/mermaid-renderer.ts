/**
 * Mermaid diagram renderer for fullscreen Markdown view.
 *
 * Lazy-loads mermaid.js and renders mermaid code blocks as SVG diagrams.
 * Only loads the library when mermaid blocks are detected.
 *
 * @module markdown/mermaid-renderer
 */

/** Mermaid API interface for dynamic import */
interface MermaidAPI {
	initialize: (config: Record<string, unknown>) => void;
	render: (id: string, source: string) => Promise<{ svg: string }>;
}

/**
 * Renders mermaid code blocks as SVG diagrams.
 *
 * Uses lazy loading to avoid loading mermaid.js when no mermaid blocks exist.
 */
export class MermaidRenderer {
	/** Cached mermaid instance */
	private mermaid: MermaidAPI | null = null;

	/** Counter for generating unique render IDs */
	private renderCounter = 0;

	/**
	 * Render all mermaid code blocks in the given container.
	 *
	 * Scans for `pre > code.language-mermaid` elements, lazy-loads mermaid.js
	 * if any are found, and replaces them with SVG diagrams.
	 *
	 * @param container - DOM element containing rendered Markdown
	 */
	async renderAll(container: HTMLElement): Promise<void> {
		const codeBlocks = this.findMermaidBlocks(container);
		if (codeBlocks.length === 0) return;

		await this.ensureInitialized();

		for (const codeElement of codeBlocks) {
			await this.renderBlock(codeElement);
		}
	}

	/**
	 * Find all mermaid code blocks in the container.
	 */
	private findMermaidBlocks(container: HTMLElement): HTMLElement[] {
		const selector = "pre > code.language-mermaid, pre > code.hljs.language-mermaid";
		return Array.from(container.querySelectorAll<HTMLElement>(selector));
	}

	/**
	 * Lazy-load and initialize mermaid.js.
	 */
	private async ensureInitialized(): Promise<void> {
		if (this.mermaid) return;

		const mermaidModule = await import("mermaid");
		this.mermaid = mermaidModule.default;

		this.mermaid.initialize({
			startOnLoad: false,
			theme: "dark",
			securityLevel: "strict",
		});
	}

	/**
	 * Render a single mermaid code block to SVG.
	 *
	 * On success, replaces the code block's parent `<pre>` with an SVG container.
	 * On failure, leaves the original code block unchanged.
	 */
	private async renderBlock(codeElement: HTMLElement): Promise<void> {
		if (!this.mermaid) return;

		const pre = codeElement.parentElement;
		if (!pre) return;

		const source = codeElement.textContent || "";
		const id = `mermaid-${++this.renderCounter}`;

		try {
			const { svg } = await this.mermaid.render(id, source);

			const wrapper = document.createElement("div");
			wrapper.className = "mermaid-diagram";
			wrapper.innerHTML = svg;

			pre.parentNode?.replaceChild(wrapper, pre);
		} catch (err) {
			console.warn("[WARN][FRONTEND] MermaidRenderer: failed to render block", err);
		}
	}
}
