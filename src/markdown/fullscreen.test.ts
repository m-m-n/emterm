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
const { FullscreenMarkdownView, resolvePath } = await import("./fullscreen.ts");

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
	let container: HTMLElement;

	beforeEach(() => {
		view = new FullscreenMarkdownView();
		// Create container for container-based rendering
		container = document.createElement("div");
		container.className = "overlay-root";
		document.body.appendChild(container);
	});

	afterEach(() => {
		view.dispose();
		// Clean up any remaining overlays
		document.querySelectorAll(".markdown-fullscreen-overlay").forEach((el) => {
			el.remove();
		});
		container.remove();
	});

	describe("show with container parameter", () => {
		test("should append overlay to provided container", () => {
			const block = createMockBlock();
			view.show(block, container);

			// Overlay should be inside the container
			const overlay = container.querySelector(".markdown-fullscreen-overlay");
			expect(overlay).not.toBeNull();

			// Should NOT be a direct child of document.body
			const bodyOverlay = document.body.querySelector(":scope > .markdown-fullscreen-overlay");
			expect(bodyOverlay).toBeNull();
		});

		test("should create overlay element in container", () => {
			const block = createMockBlock();
			view.show(block, container);

			const overlay = container.querySelector(".markdown-fullscreen-overlay");
			expect(overlay).not.toBeNull();
		});

		test("should render markdown content", () => {
			const block = createMockBlock("<h1>Test Heading</h1>");
			view.show(block, container);

			const content = container.querySelector(".markdown-fullscreen-content");
			expect(content).not.toBeNull();
			expect(content?.innerHTML).toContain("Test Heading");
		});

		test("should set accessibility attributes", () => {
			const block = createMockBlock();
			view.show(block, container);

			const overlay = container.querySelector(".markdown-fullscreen-overlay");
			expect(overlay?.getAttribute("role")).toBe("dialog");
			expect(overlay?.getAttribute("aria-modal")).toBe("true");
			expect(overlay?.getAttribute("aria-label")).toBe("Markdown Document");
		});

		test("should close existing view before opening new one", () => {
			const block1 = createMockBlock("<p>First</p>");
			const block2 = createMockBlock("<p>Second</p>");

			view.show(block1, container);
			view.show(block2, container);

			const overlays = container.querySelectorAll(".markdown-fullscreen-overlay");
			expect(overlays.length).toBe(1);

			const content = container.querySelector(".markdown-fullscreen-content");
			expect(content?.innerHTML).toContain("Second");
		});

		test("should set isActive to true", () => {
			expect(view.isActive()).toBe(false);

			const block = createMockBlock();
			view.show(block, container);

			expect(view.isActive()).toBe(true);
		});

		test("should focus content for keyboard navigation", () => {
			const block = createMockBlock();
			view.show(block, container);

			const content = container.querySelector(".markdown-fullscreen-content");
			expect(content?.getAttribute("tabindex")).toBe("-1");
		});
	});

	describe("close", () => {
		test("should remove overlay from container", () => {
			const block = createMockBlock();
			view.show(block, container);

			expect(
				container.querySelector(".markdown-fullscreen-overlay"),
			).not.toBeNull();

			view.close();

			expect(container.querySelector(".markdown-fullscreen-overlay")).toBeNull();
		});

		test("should set isActive to false", () => {
			const block = createMockBlock();
			view.show(block, container);

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
			view.show(block, container);

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
			view.show(block, container);

			expect(view.isActive()).toBe(true);
		});

		test("should return false after close", () => {
			const block = createMockBlock();
			view.show(block, container);
			view.close();

			expect(view.isActive()).toBe(false);
		});
	});

	describe("keyboard navigation - Escape", () => {
		test("should close on Escape key", () => {
			const block = createMockBlock();
			view.show(block, container);

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
			view.show(block, container);

			expect(view.isActive()).toBe(true);

			view.dispose();

			expect(view.isActive()).toBe(false);
			expect(container.querySelector(".markdown-fullscreen-overlay")).toBeNull();
		});
	});

	describe("scrollTo", () => {
		test("should scroll to top", () => {
			const block = createMockBlock();
			view.show(block, container);

			const content = container.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;
			// Manually set scrollTop to simulate scrolled state
			content.scrollTop = 500;

			view.scrollTo("top");

			expect(content.scrollTop).toBe(0);
		});

		test("should scroll to bottom", () => {
			const block = createMockBlock();
			view.show(block, container);

			const content = container.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			view.scrollTo("bottom");

			// scrollTop should be set to scrollHeight
			expect(content.scrollTop).toBe(content.scrollHeight);
		});

		test("should scroll to specific position", () => {
			const block = createMockBlock();
			view.show(block, container);

			const content = container.querySelector(
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
			view.show(block, container);

			const content = container.querySelector(
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
			view.show(block, container);

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
			view.show(block, container);

			const event = new KeyboardEvent("keydown", {
				key: "ArrowUp",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(true);
		});

		test("should scroll page on PageDown", () => {
			const block = createMockBlock();
			view.show(block, container);

			const event = new KeyboardEvent("keydown", {
				key: "PageDown",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(true);
		});

		test("should scroll page on PageUp", () => {
			const block = createMockBlock();
			view.show(block, container);

			const event = new KeyboardEvent("keydown", {
				key: "PageUp",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(view.isActive()).toBe(true);
		});

		test("should scroll to top on Home", () => {
			const block = createMockBlock();
			view.show(block, container);

			const content = container.querySelector(
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
			view.show(block, container);

			const content = container.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			const event = new KeyboardEvent("keydown", {
				key: "End",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(content.scrollTop).toBe(content.scrollHeight);
		});

		test("should scroll down on Space key", () => {
			const block = createMockBlock();
			view.show(block, container);

			const event = new KeyboardEvent("keydown", {
				key: " ",
				bubbles: true,
			});
			document.dispatchEvent(event);

			// View should remain active (Space doesn't close)
			expect(view.isActive()).toBe(true);
		});

		test("should scroll up on Shift+Space key", () => {
			const block = createMockBlock();
			view.show(block, container);

			const event = new KeyboardEvent("keydown", {
				key: " ",
				shiftKey: true,
				bubbles: true,
			});
			document.dispatchEvent(event);

			// View should remain active (Shift+Space doesn't close)
			expect(view.isActive()).toBe(true);
		});

		test("should trap focus with Tab key", () => {
			const blockWithLinks = createMockBlock(
				'<p><a href="https://link1.com">Link 1</a></p><p><a href="https://link2.com">Link 2</a></p>',
			);
			view.show(blockWithLinks, container);

			const links = container.querySelectorAll("a");
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
			view.show(blockWithLinks, container);

			const links = container.querySelectorAll("a");
			const content = container.querySelector(
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
			view.show(blockWithCode, container);

			const copyButton = container.querySelector(".copy-code-button");
			expect(copyButton).not.toBeNull();
		});

		test("should not add copy buttons when showCopyButtons is false", () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>',
			);
			view.show(blockWithCode, container, { showCopyButtons: false });

			const copyButton = container.querySelector(".copy-code-button");
			expect(copyButton).toBeNull();
		});

		test("should add multiple copy buttons for multiple code blocks", () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs">code1</code></pre><pre><code class="hljs">code2</code></pre>',
			);
			view.show(blockWithCode, container);

			const copyButtons = container.querySelectorAll(".copy-code-button");
			expect(copyButtons.length).toBe(2);
		});

		test("should copy code to clipboard on button click", async () => {
			const blockWithCode = createMockBlock(
				'<pre><code class="hljs language-javascript">const x = 1;</code></pre>',
			);
			view.show(blockWithCode, container);

			const copyButton = container.querySelector(
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
			view.show(blockWithCode, container);

			const copyButton = container.querySelector(
				".copy-code-button",
			) as HTMLElement;
			copyButton.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			expect(copyButton.innerHTML).toContain("Copied!");
			expect(copyButton.classList.contains("copy-success")).toBe(true);
		});
	});

	describe("CSS zoom", () => {
		test("should apply CSS zoom on zoom change", () => {
			const block = createMockBlock("<p>Test content</p>");
			view.show(block, container);

			const content = container.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			// Get zoom controller and change zoom
			// Zoom in (+10%)
			const event = new KeyboardEvent("keydown", {
				key: "+",
				bubbles: true,
			});
			document.dispatchEvent(event);

			// CSS zoom should be set (110% = 1.1)
			expect(content.style.zoom).toBe("1.1");
		});

		test("should have zoom=1 at 100%", () => {
			const block = createMockBlock("<p>Test content</p>");
			view.show(block, container);

			const content = container.querySelector(
				".markdown-fullscreen-content",
			) as HTMLElement;

			// At 100%, zoom should be 1
			expect(content.style.zoom).toBe("1");
		});

		test("should not use transform for zoom", () => {
			const block = createMockBlock("<p>Test content</p>");
			view.show(block, container);

			const content = container.querySelector(
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
			view.show(blockWithLink, container);

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			// Wait for event processing
			await new Promise((resolve) => setTimeout(resolve, 10));

			// Dialog should appear (in container now)
			const dialog = container.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).not.toBeNull();

			// Close dialog to clean up
			const cancelBtn = container.querySelector(
				".link-confirm-cancel",
			) as HTMLElement;
			cancelBtn?.click();
		});

		test("should not show dialog for non-http links", async () => {
			const blockWithLink = createMockBlock(
				'<p><a href="mailto:test@example.com">Email</a></p>',
			);
			view.show(blockWithLink, container);

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			// No dialog should appear
			const dialog = container.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();
		});

		test("should open link directly with linkBehavior=direct", async () => {
			// Reset mock before test
			mockShellOpen.mockClear();

			const blockWithLink = createMockBlock(
				'<p><a href="https://example.com">Example Link</a></p>',
			);
			view.show(blockWithLink, container, { linkBehavior: "direct" });

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			// Wait for async openLink to complete
			await new Promise((resolve) => setTimeout(resolve, 50));

			// No dialog should appear
			const dialog = container.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();

			// Shell.open should be called directly
			expect(mockShellOpen.mock.calls.length).toBeGreaterThan(0);
		});

		test("should not open link when linkBehavior is disabled", async () => {
			const blockWithLink = createMockBlock(
				'<p><a href="https://example.com">Example Link</a></p>',
			);
			view.show(blockWithLink, container, { linkBehavior: "disabled" });

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			// No dialog should appear
			const dialog = container.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();
		});
	});

	describe("markdown link navigation", () => {
		test("should write navigate command for .md links", async () => {
			const ptyWrite = mock((_data: string) => {});
			const blockWithMdLink = createMockBlock(
				'<p><a href="./other.md">Other doc</a></p>',
			);
			view.show(blockWithMdLink, container, undefined, ptyWrite, "/home/user/docs");

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			expect(ptyWrite).toHaveBeenCalledWith("navigate /home/user/docs/other.md\n");
		});

		test("should resolve relative .md paths against basedir", async () => {
			const ptyWrite = mock((_data: string) => {});
			const blockWithMdLink = createMockBlock(
				'<p><a href="../readme.md">Readme</a></p>',
			);
			view.show(blockWithMdLink, container, undefined, ptyWrite, "/home/user/docs/sub");

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			expect(ptyWrite).toHaveBeenCalledWith("navigate /home/user/docs/readme.md\n");
		});

		test("should handle absolute .md paths", async () => {
			const ptyWrite = mock((_data: string) => {});
			const blockWithMdLink = createMockBlock(
				'<p><a href="/absolute/path/doc.md">Doc</a></p>',
			);
			view.show(blockWithMdLink, container, undefined, ptyWrite, "/home/user");

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			expect(ptyWrite).toHaveBeenCalledWith("navigate /absolute/path/doc.md\n");
		});

		test("should not navigate .md links without PTY write callback", async () => {
			const blockWithMdLink = createMockBlock(
				'<p><a href="./other.md">Other doc</a></p>',
			);
			// No ptyWrite callback
			view.show(blockWithMdLink, container);

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			// Should not throw or show dialog
			const dialog = container.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).toBeNull();
		});

		test("should still show dialog for http links when PTY write is set", async () => {
			const ptyWrite = mock((_data: string) => {});
			const blockWithHttpLink = createMockBlock(
				'<p><a href="https://example.com">Example</a></p>',
			);
			view.show(blockWithHttpLink, container, undefined, ptyWrite, "/home/user");

			const link = container.querySelector("a") as HTMLElement;
			link.click();

			await new Promise((resolve) => setTimeout(resolve, 10));

			// Should show confirmation dialog, not navigate
			const dialog = container.querySelector(".link-confirm-dialog-overlay");
			expect(dialog).not.toBeNull();

			// Close dialog
			const cancelBtn = container.querySelector(".link-confirm-cancel") as HTMLElement;
			cancelBtn?.click();
		});
	});

	describe("quit on close", () => {
		test("should write quit command to PTY on close", () => {
			const ptyWrite = mock((_data: string) => {});
			const block = createMockBlock();
			view.show(block, container, undefined, ptyWrite, "/home/user");

			view.close();

			expect(ptyWrite).toHaveBeenCalledWith("quit\n");
		});

		test("should not write quit without PTY write callback", () => {
			const block = createMockBlock();
			view.show(block, container);

			// Should not throw
			view.close();
			expect(view.isActive()).toBe(false);
		});

		test("should write quit on Escape key", () => {
			const ptyWrite = mock((_data: string) => {});
			const block = createMockBlock();
			view.show(block, container, undefined, ptyWrite, "/home/user");

			const event = new KeyboardEvent("keydown", {
				key: "Escape",
				bubbles: true,
			});
			document.dispatchEvent(event);

			expect(ptyWrite).toHaveBeenCalledWith("quit\n");
			expect(view.isActive()).toBe(false);
		});
	});

	describe("IntersectionObserver for local images", () => {
		test("should set up observer for images with data-local-src", () => {
			const ptyWrite = mock((_data: string) => {});
			const blockWithImage = createMockBlock(
				'<p><img src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7" data-local-src="./images/test.png" alt="test"></p>',
			);
			view.show(blockWithImage, container, undefined, ptyWrite, "/home/user/docs");

			// In test env, IntersectionObserver may not fire automatically
			// Verify the img has data-local-src attribute
			const img = container.querySelector("img[data-local-src]");
			expect(img).not.toBeNull();
		});

		test("should not set up observer without PTY write callback", () => {
			const blockWithImage = createMockBlock(
				'<p><img src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7" data-local-src="./images/test.png" alt="test"></p>',
			);
			view.show(blockWithImage, container);

			// No observer should be set up, image should remain as placeholder
			const img = container.querySelector("img[data-local-src]") as HTMLImageElement;
			expect(img).not.toBeNull();
			expect(img.getAttribute("data-request-id")).toBeNull();
		});
	});

	describe("resolvePath", () => {
		test("should resolve relative path against basedir", () => {
			expect(resolvePath("/home/user/docs", "./file.md")).toBe("/home/user/docs/file.md");
		});

		test("should resolve parent directory references", () => {
			expect(resolvePath("/home/user/docs/sub", "../file.md")).toBe("/home/user/docs/file.md");
		});

		test("should resolve deeply nested parent references", () => {
			expect(resolvePath("/home/user/docs/a/b", "../../file.md")).toBe("/home/user/docs/file.md");
		});

		test("should return absolute paths as-is", () => {
			expect(resolvePath("/home/user", "/absolute/path.md")).toBe("/absolute/path.md");
		});

		test("should handle basedir with trailing slash", () => {
			expect(resolvePath("/home/user/docs/", "file.md")).toBe("/home/user/docs/file.md");
		});

		test("should handle simple relative path (no ./ prefix)", () => {
			expect(resolvePath("/home/user/docs", "file.md")).toBe("/home/user/docs/file.md");
		});

		test("should handle paths with spaces", () => {
			expect(resolvePath("/home/user/my docs", "file name.md")).toBe("/home/user/my docs/file name.md");
		});

		test("should not go above root", () => {
			expect(resolvePath("/", "../file.md")).toBe("/file.md");
		});
	});
});
