/**
 * Markdown renderer.
 *
 * Renders Markdown content to sanitized HTML using marked and DOMPurify.
 *
 * @module markdown/renderer
 */

import DOMPurify from "dompurify";
import hljs from "highlight.js";
import { marked } from "marked";
import type { MarkdownBlock, MarkdownFormat } from "./types.ts";

/**
 * DOMPurify configuration for XSS protection.
 */
const PURIFY_CONFIG = {
	ALLOWED_TAGS: [
		// Headings
		"h1",
		"h2",
		"h3",
		"h4",
		"h5",
		"h6",
		// Block elements
		"p",
		"br",
		"hr",
		"div",
		"span",
		// Lists
		"ul",
		"ol",
		"li",
		// Formatting
		"blockquote",
		"pre",
		"code",
		// Tables
		"table",
		"thead",
		"tbody",
		"tfoot",
		"tr",
		"th",
		"td",
		// Inline elements
		"a",
		"strong",
		"b",
		"em",
		"i",
		"del",
		"s",
		"mark",
		"sub",
		"sup",
		// Media (limited)
		"img",
		// Task lists (GFM)
		"input",
	],
	ALLOWED_ATTR: [
		// Links
		"href",
		"target",
		"rel",
		// Images
		"src",
		"alt",
		"title",
		"width",
		"height",
		// Code highlighting
		"class",
		// Tables
		"colspan",
		"rowspan",
		// Task list checkboxes
		"type",
		"checked",
		"disabled",
		// IDs for anchors
		"id",
		"name",
	],
	ALLOW_DATA_ATTR: false,
	// Force all links to open in new tab safely
	ADD_ATTR: ["target", "rel"],
	// Explicitly forbid dangerous elements
	FORBID_TAGS: [
		"script",
		"style",
		"iframe",
		"object",
		"embed",
		"form",
		"base",
		"meta",
		"link",
		"noscript",
		"svg",
		"math",
	],
	// Forbid all event handler attributes
	FORBID_ATTR: [
		"onerror",
		"onclick",
		"onload",
		"onmouseover",
		"onfocus",
		"onblur",
		"onchange",
		"onsubmit",
		"onkeydown",
		"onkeyup",
		"onkeypress",
		"formaction",
		"srcdoc",
		"action",
		"background",
		"dynsrc",
		"lowsrc",
	],
	// Sanitize URLs
	ALLOWED_URI_REGEXP:
		/^(?:(?:(?:f|ht)tps?|mailto|tel|callto|cid|xmpp):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
};

/**
 * Configure marked with syntax highlighting.
 */
function configureMarked(format: MarkdownFormat): void {
	marked.setOptions({
		gfm: format === "gfm",
		breaks: format === "gfm",
	});
}

/**
 * Custom renderer for code blocks with syntax highlighting.
 */
const renderer = new marked.Renderer();

renderer.code = ({ text, lang }: { text: string; lang?: string }): string => {
	const language = lang && hljs.getLanguage(lang) ? lang : "plaintext";
	let highlighted: string;

	try {
		highlighted = hljs.highlight(text, { language }).value;
	} catch {
		// Fallback to escaped text if highlighting fails
		highlighted = escapeHtml(text);
	}

	const langClass = language ? ` language-${language}` : "";
	return `<pre><code class="hljs${langClass}">${highlighted}</code></pre>`;
};

/**
 * Escape HTML entities for fallback.
 */
function escapeHtml(text: string): string {
	return text
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#039;");
}

/**
 * Markdown renderer class.
 *
 * Provides methods to render Markdown to HTML with XSS protection.
 */
export class MarkdownRenderer {
	/** Map of block ID to DOM element */
	private blocks = new Map<string, HTMLElement>();

	/** Container element reference for virtual scrolling */
	private container: HTMLElement | null = null;

	/** Cached mermaid module instance (null = not loaded, non-null = initialized) */
	private mermaidModule: typeof import("mermaid").default | null = null;

	/**
	 * Create a new renderer.
	 */
	constructor() {
		// Set custom renderer for syntax highlighting
		marked.use({ renderer });
	}

	/**
	 * Render Markdown text to sanitized HTML.
	 *
	 * @param markdown - Raw Markdown text
	 * @param format - Markdown format to use
	 * @returns Sanitized HTML string
	 */
	render(markdown: string, format: MarkdownFormat): string {
		// Configure marked for the format
		configureMarked(format);

		// Parse Markdown to HTML
		let rawHtml: string;
		try {
			rawHtml = marked.parse(markdown) as string;
		} catch {
			// Fallback to escaped text on parse error
			rawHtml = `<pre>${escapeHtml(markdown)}</pre>`;
		}

		// Sanitize HTML
		const cleanHtml = DOMPurify.sanitize(rawHtml, PURIFY_CONFIG);

		return `<div class="markdown-content">${cleanHtml}</div>`;
	}

	/**
	 * Insert rendered HTML into terminal display.
	 *
	 * @param block - Rendered Markdown block
	 * @param container - Target DOM container
	 * @returns Created HTML element
	 */
	insertBlock(block: MarkdownBlock, container: HTMLElement): HTMLElement {
		const element = document.createElement("div");
		element.className = "markdown-block";
		element.dataset.markdownId = block.id;
		element.dataset.startRow = String(block.startRow);
		element.dataset.rowCount = String(block.rowCount);
		element.innerHTML = block.html;

		// Add target="_blank" and rel="noopener" to all links
		element.querySelectorAll("a").forEach((link) => {
			link.setAttribute("target", "_blank");
			link.setAttribute("rel", "noopener noreferrer");
		});

		// Process mermaid diagrams if present
		this.processMermaidDiagrams(element);

		container.appendChild(element);
		this.blocks.set(block.id, element);
		this.container = container;

		return element;
	}

	/**
	 * Process mermaid diagrams in the element.
	 * Mermaid blocks are identified by code blocks with language "mermaid".
	 */
	private async processMermaidDiagrams(element: HTMLElement): Promise<void> {
		const mermaidBlocks = element.querySelectorAll("code.language-mermaid");
		if (mermaidBlocks.length === 0) return;

		// Lazy load and cache mermaid module
		if (!this.mermaidModule) {
			try {
				const mermaidImport = await import("mermaid");
				this.mermaidModule = mermaidImport.default;
				this.mermaidModule.initialize({
					startOnLoad: false,
					theme: "dark",
					securityLevel: "strict",
				});
			} catch {
				console.warn("Failed to initialize mermaid");
				return;
			}
		}

		// Render each mermaid block
		for (const block of mermaidBlocks) {
			const pre = block.parentElement;
			if (!pre || pre.tagName !== "PRE") continue;

			const code = block.textContent || "";
			const id = `mermaid-${Date.now()}-${Math.random().toString(36).slice(2)}`;

			try {
				const { svg } = await this.mermaidModule.render(id, code);
				const wrapper = document.createElement("div");
				wrapper.className = "mermaid-diagram";
				// Sanitize SVG output with DOMPurify for additional security layer
				wrapper.innerHTML = DOMPurify.sanitize(svg, {
					USE_PROFILES: { svg: true, svgFilters: true },
					ADD_TAGS: ["foreignObject"],
				});
				pre.replaceWith(wrapper);
			} catch {
				// Keep the code block on mermaid error
				console.warn("Failed to render mermaid diagram");
			}
		}
	}

	/**
	 * Remove a Markdown block from display.
	 *
	 * @param id - Block identifier
	 */
	removeBlock(id: string): void {
		const element = this.blocks.get(id);
		if (element) {
			element.remove();
			this.blocks.delete(id);
		}
	}

	/**
	 * Get a Markdown block element by ID.
	 *
	 * @param id - Block identifier
	 * @returns Block element or undefined
	 */
	getBlock(id: string): HTMLElement | undefined {
		return this.blocks.get(id);
	}

	/**
	 * Update block visibility based on scroll position.
	 *
	 * Implements virtual scrolling by detaching off-screen blocks
	 * and reattaching them when they become visible.
	 * Maintains correct order when reattaching elements.
	 *
	 * @param visibleRange - Currently visible row range
	 */
	updateVisibility(visibleRange: { start: number; end: number }): void {
		if (!this.container) return;

		// Collect blocks that need to be reattached with their row positions
		const toReattach: Array<{ element: HTMLElement; row: number }> = [];

		for (const [, element] of this.blocks) {
			const row = parseInt(element.dataset.startRow || "0", 10);
			const height = parseInt(element.dataset.rowCount || "1", 10);

			const isVisible =
				row + height >= visibleRange.start && row <= visibleRange.end;

			if (isVisible && !element.parentElement) {
				// Mark for reattachment
				toReattach.push({ element, row });
			} else if (!isVisible && element.parentElement) {
				// Detach but keep reference
				element.remove();
			}
		}

		// Sort by row position and reattach in order
		toReattach.sort((a, b) => a.row - b.row);

		for (const { element } of toReattach) {
			// Find the correct position to insert
			const row = parseInt(element.dataset.startRow || "0", 10);
			let insertBefore: Element | null = null;

			for (const child of this.container.children) {
				const childRow = parseInt(
					(child as HTMLElement).dataset.startRow || "0",
					10,
				);
				if (childRow > row) {
					insertBefore = child;
					break;
				}
			}

			if (insertBefore) {
				this.container.insertBefore(element, insertBefore);
			} else {
				this.container.appendChild(element);
			}
		}
	}

	/**
	 * Dispose renderer and clean up resources.
	 */
	dispose(): void {
		for (const element of this.blocks.values()) {
			element.remove();
		}
		this.blocks.clear();
		this.container = null;
	}
}
