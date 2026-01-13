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
import type { MarkdownFormat } from "./types.ts";

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
 * Used by MarkdownSessionManager to convert Markdown to HTML for fullscreen display.
 */
export class MarkdownRenderer {
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
	 * Dispose renderer and clean up resources.
	 */
	dispose(): void {
		// No-op: Renderer no longer manages DOM elements
		// Fullscreen view handles its own DOM lifecycle
	}
}
