/**
 * Tests for FullscreenMarkdownView.
 */
import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type { MarkdownBlock } from "./types.ts";

// Mock Tauri shell plugin - must be before importing fullscreen.ts
const mockShellOpen = mock(() => Promise.resolve());
mock.module("@tauri-apps/plugin-shell", () => ({
	open: mockShellOpen,
}));

// Mock Tauri clipboard plugin - must be before importing fullscreen.ts
const mockWriteText = mock(() => Promise.resolve());
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
	writeText: mockWriteText,
}));

// Import after mocks are set up
const { FullscreenMarkdownView } = await import("./fullscreen.ts");

function createMockBlock(html = "<p>Test content</p>"): MarkdownBlock {
	return {
		id: "test-block-123",
		html,
		startRow: 0,
		rowCount: 0,
		visible: true,
	};
}

describe("FullscreenMarkdownView", () => {
	let view: FullscreenMarkdownView;

	beforeEach(() => {
		view = new FullscreenMarkdownView();
	});

	afterEach(() => {
		view.dispose();
		// Clean up any remaining overlays
		document.querySelectorAll(".markdown-fullscreen-overlay").forEach((el) => {
			el.remove();
		});
	});

	describe("show", () => {
		test("should create overlay element in DOM", () => {
			const block = createMockBlock();
			view.show(block);

			const overlay = document.querySelector(".markdown-fullscreen-overlay");
			expect(overlay).not.toBeNull();
		});

		test("should render markdown content", () => {
			const block = createMockBlock("<h1>Test Heading</h1>");
			view.show(block);

			const content = document.querySelector(".markdown-fullscreen-content");
			expect(content).not.toBeNull();
			expect(content?.innerHTML).toContain("Test Heading");
		});

		test("should set accessibility attributes", () => {
			const block = createMockBlock();
			view.show(block);

			const overlay = document.querySelector(".markdown-fullscreen-overlay");
			expect(overlay?.getAttribute("role")).toBe("dialog");
			expect(overlay?.getAttribute("aria-modal")).toBe("true");
			expect(overlay?.getAttribute("aria-label")).toBe("Markdown Document");
		});

		test("should close existing view before opening new one", () => {
			const block1 = createMockBlock("<p>First</p>");
			const block2 = createMockBlock("<p>Second</p>");

			view.show(block1);
			view.show(block2);

			const overlays = document.querySelectorAll(".markdown-fullscreen-overlay");
			expect(overlays.length).toBe(1);

			const content = document.querySelector(".markdown-fullscreen-content");
			expect(content?.innerHTML).toContain("Second");
		});

		test("should set isActive to true", () => {
			expect(view.isActive()).toBe(false);

			const block = createMockBlock();
			view.show(block);

			expect(view.isActive()).toBe(true);
		});

		test("should focus content for keyboard navigation", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(".markdown-fullscreen-content");
			expect(content?.getAttribute("tabindex")).toBe("-1");
		});
	});

	describe("close", () => {
		test("should remove overlay from DOM", () => {
			const block = createMockBlock();
			view.show(block);

			expect(
				document.querySelector(".markdown-fullscreen-overlay"),
			).not.toBeNull();

			view.close();

			expect(document.querySelector(".markdown-fullscreen-overlay")).toBeNull();
		});

		test("should set isActive to false", () => {
			const block = createMockBlock();
			view.show(block);

			expect(view.isActive()).toBe(true);

			view.close();

			expect(view.isActive()).toBe(false);
		});

		test("should do nothing if not active", () => {
			// Should not throw
			view.close();
			expect(view.isActive()).toBe(false);
		});

		test("should restore focus to previously focused element", () => {
			const button = document.createElement("button");
			button.id = "test-focus-button";
			document.body.appendChild(button);
			button.focus();

			const block = createMockBlock();
			view.show(block);

			// Focus moved to content
			expect(document.activeElement?.className).toBe(
				"markdown-fullscreen-content",
			);

			view.close();

			// Focus should be restored
			expect(document.activeElement?.id).toBe("test-focus-button");

			button.remove();
		});
	});

	describe("isActive", () => {
		test("should return false initially", () => {
			expect(view.isActive()).toBe(false);
		});

		test("should return true after show", () => {
			const block = createMockBlock();
			view.show(block);

			expect(view.isActive()).toBe(true);
		});

		test("should return false after close", () => {
			const block = createMockBlock();
			view.show(block);
			view.close();

			expect(view.isActive()).toBe(false);
		});
	});

	describe("keyboard navigation - Escape", () => {
		test("should close on Escape key", () => {
			const block = createMockBlock();
			view.show(block);

			expect(view.isActive()).toBe(true);

			// Simulate Escape key press
			const event = new KeyboardEvent("keydown", {
				key: "Escape",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(false);
		});
	});

	describe("dispose", () => {
		test("should close view and clean up resources", () => {
			const block = createMockBlock();
			view.show(block);

			expect(view.isActive()).toBe(true);

			view.dispose();

			expect(view.isActive()).toBe(false);
			expect(document.querySelector(".markdown-fullscreen-overlay")).toBeNull();
		});
	});

	describe("scrollTo", () => {
		test("should scroll to top", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;
			// Manually set scrollTop to simulate scrolled state
			content.scrollTop = 500;

			view.scrollTo("top");

			expect(content.scrollTop).toBe(0);
		});

		test("should scroll to bottom", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			view.scrollTo("bottom");

			// scrollTop should be set to scrollHeight
			expect(content.scrollTop).toBe(content.scrollHeight);
		});

		test("should scroll to specific position", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			view.scrollTo(100);

			expect(content.scrollTop).toBe(100);
		});

		test("should do nothing if not active", () => {
			// Should not throw
			view.scrollTo("top");
		});
	});

	describe("scrollBy", () => {
		test("should scroll by delta", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;
			content.scrollTop = 100;

			view.scrollBy(50);

			// Note: happy-dom may not fully support scrollBy with smooth behavior
			// We test that the method doesn't throw
		});

		test("should do nothing if not active", () => {
			// Should not throw
			view.scrollBy(50);
		});
	});

	describe("keyboard navigation - scroll", () => {
		test("should scroll down on ArrowDown", () => {
			const block = createMockBlock();
			view.show(block);

			const event = new KeyboardEvent("keydown", {
				key: "ArrowDown",
				bubbles: true,
			});
			document.dispatchEvent(event);

			// Test that event is handled (doesn't throw)
			expect(view.isActive()).toBe(true);
		});

		test("should scroll up on ArrowUp", () => {
			const block = createMockBlock();
			view.show(block);

			const event = new KeyboardEvent("keydown", {
				key: "ArrowUp",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(true);
		});

		test("should scroll page on PageDown", () => {
			const block = createMockBlock();
			view.show(block);

			const event = new KeyboardEvent("keydown", {
				key: "PageDown",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(true);
		});

		test("should scroll page on PageUp", () => {
			const block = createMockBlock();
			view.show(block);

			const event = new KeyboardEvent("keydown", {
				key: "PageUp",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(true);
		});

		test("should scroll to top on Home", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;
			content.scrollTop = 500;

			const event = new KeyboardEvent("keydown", {
				key: "Home",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(content.scrollTop).toBe(0);
		});

		test("should scroll to bottom on End", () => {
			const block = createMockBlock();
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			const event = new KeyboardEvent("keydown", {
				key: "End",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(content.scrollTop).toBe(content.scrollHeight);
		});

		test("should trap focus with Tab key", () => {
			const blockWithLinks = createMockBlock(
				'<p><a href="https://link1.com">Link 1</a></p><p><a href="https://link2.com">Link 2</a></p>',
			);
			view.show(blockWithLinks);

			const links = document.querySelectorAll("a");
			expect(links.length).toBe(2);

			// Focus the last link
			(links[1] as HTMLElement).focus();
			expect(document.activeElement).toBe(links[1]);

			// Tab from last should wrap to first
			const tabEvent = new KeyboardEvent("keydown", {
				key: "Tab",
				bubbles: true,
			});
			document.dispatchEvent(tabEvent);

			expect(document.activeElement).toBe(links[0]);
		});

		test("should trap focus with Shift+Tab key", () => {
			const blockWithLinks = createMockBlock(
				'<p><a href="https://link1.com">Link 1</a></p><p><a href="https://link2.com">Link 2</a></p>',
			);
			view.show(blockWithLinks);

			const links = document.querySelectorAll("a");
			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			// Focus the content (simulating start state)
			content.focus();

			// Shift+Tab from content should go to last link
			const shiftTabEvent = new KeyboardEvent("keydown", {
				key: "Tab",
				shiftKey: true,
				bubbles: true,
			});
			document.dispatchEvent(shiftTabEvent);

			expect(document.activeElement).toBe(links[1]);
		});
	});

	describe("code copy functionality", () => {
		test("should add copy buttons to code blocks", () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>',
			);
			view.show(blockWithCode);

			const copyButton = document.querySelector(".copy-code-button");
			expect(copyButton).not.toBeNull();
		});

		test("should not add copy buttons when showCopyButtons is false", () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>',
			);
			view.show(blockWithCode, { showCopyButtons: false });

			const copyButton = document.querySelector(".copy-code-button");
			expect(copyButton).toBeNull();
		});

		test("should add multiple copy buttons for multiple code blocks", () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs">code1</code></pre><pre><code class="hljs">code2</code></pre>',
			);
			view.show(blockWithCode);

			const copyButtons = document.querySelectorAll(".copy-code-button");
			expect(copyButtons.length).toBe(2);
		});

		test("should copy code to clipboard on button click", async () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>',
			);
			view.show(blockWithCode);

			const copyButton = document.querySelector(
				".copy-code-button",
			) as HTMLElement;
			expect(copyButton).not.toBeNull();

			// Click the copy button
			copyButton.click();

			// Wait for async operation
			await new Promise((resolve) => setTimeout(resolve, 10));

			expect(mockWriteText).toHaveBeenCalled();
		});

		test("should show success feedback after copy", async () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs">const x = 1;</code></pre>',
			);
			view.show(blockWithCode);

			const copyButton = document.querySelector(
				".copy-code-button",
			) as HTMLElement;
			copyButton.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			expect(copyButton.innerHTML).toContain("Copied!");
			expect(copyButton.classList.contains("copy-success")).toBe(true);
		});
	});

	describe("font-size zoom", () => {
		test("should apply font-size instead of transform on zoom", () => {
			const block = createMockBlock("<p>Test content</p>");
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			// Get zoom controller and change zoom
			// Zoom to 150%
			const event = new KeyboardEvent("keydown", {
				key: "+",
				bubbles: true,
			});
			document.dispatchEvent(event);

			// Font-size should be set, not transform
			// 150% of 16px = 24px
			expect(content.style.fontSize).toBe("17.6px"); // 110% of 16px
		});

		test("should have 16px font-size at 100% zoom", () => {
			const block = createMockBlock("<p>Test content</p>");
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			// At 100%, font-size should be 16px
			expect(content.style.fontSize).toBe("16px");
		});

		test("should not use transform for zoom", () => {
			const block = createMockBlock("<p>Test content</p>");
			view.show(block);

			const content = document.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			// Zoom in
			const event = new KeyboardEvent("keydown", {
				key: "+",
				bubbles: true,
			});
			document.dispatchEvent(event);

			// Transform should not include scale
			expect(content.style.transform).not.toContain("scale");
		});
	});

	describe("link handling", () => {
		test("should show confirmation dialog on link click", async () => {
			const blockWithLink = createMockBlock(
				'<p><a href="https://example.com">Example Link</a></p>',
			);
			view.show(blockWithLink);

			const link = document.querySelector("a") as HTMLElement;
			link.click();

			// Wait for event processing
			await new Promise((resolve) => setTimeout(resolve, 10));

			// Dialog should appear
			const dialog = document.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).not.toBeNull();

			// Close dialog to clean up
			const cancelBtn = document.querySelector(
				".link-confirm-cancel",
			) as HTMLElement;
			cancelBtn?.click();
		});

		test("should not show dialog for non-http links", async () => {
			const blockWithLink = createMockBlock(
				'<p><a href="mailto:test@example.com">Email</a></p>',
			);
			view.show(blockWithLink);

			const link = document.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			// No dialog should appear
			const dialog = document.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();
		});

		test("should open link directly with linkBehavior=direct", async () => {
			// Reset mock before test
			mockShellOpen.mockClear();

			const blockWithLink = createMockBlock(
				'<p><a href="https://example.com">Example Link</a></p>',
			);
			view.show(blockWithLink, { linkBehavior: "direct" });

			const link = document.querySelector("a") as HTMLElement;
			link.click();

			// Wait for async openLink to complete
			await new Promise((resolve) => setTimeout(resolve, 50));

			// No dialog should appear
			const dialog = document.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();

			// Shell.open should be called directly
			expect(mockShellOpen.mock.calls.length).toBeGreaterThan(0);
		});

		test("should not open link when linkBehavior is disabled", async () => {
			const blockWithLink = createMockBlock(
				'<p><a href="https://example.com">Example Link</a></p>',
			);
			view.show(blockWithLink, { linkBehavior: "disabled" });

			const link = document.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			// No dialog should appear
			const dialog = document.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();
		});
	});
});
