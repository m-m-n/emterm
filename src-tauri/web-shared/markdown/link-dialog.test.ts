/**
 * Tests for LinkConfirmDialog.
 */
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { LinkConfirmDialog } from "./link-dialog.ts";

describe("LinkConfirmDialog", () => {
	let dialog: LinkConfirmDialog;

	beforeEach(() => {
		dialog = new LinkConfirmDialog();
	});

	afterEach(() => {
		dialog.dispose();
		// Clean up any remaining dialogs
		document.querySelectorAll(".link-confirm-dialog-overlay").forEach((el) => {
			el.remove();
		});
	});

	describe("confirm", () => {
		test("should show dialog with URL", async () => {
			const promise = dialog.confirm("https://example.com");

			const dialogEl = document.querySelector(".link-confirm-dialog-overlay");
			expect(dialogEl).not.toBeNull();

			// Check URL is displayed
			const urlEl = document.querySelector(".link-confirm-url");
			expect(urlEl?.textContent).toBe("https://example.com");

			// Close dialog to resolve promise
			dialog.close();
			const result = await promise;
			expect(result).toBe(false);
		});

		test("should escape HTML in URL", async () => {
			const promise = dialog.confirm("https://example.com/<script>alert(1)</script>");

			const urlEl = document.querySelector(".link-confirm-url");
			expect(urlEl?.innerHTML).not.toContain("<script>");
			expect(urlEl?.innerHTML).toContain("&lt;script&gt;");

			dialog.close();
			await promise;
		});

		test("should display URL with special characters safely", async () => {
			// Test various special characters that could be used for XSS
			const promise = dialog.confirm("https://example.com/?q='test'&x=\"value\"");

			const urlEl = document.querySelector(".link-confirm-url");
			// Verify the URL is displayed as text, not interpreted as HTML
			expect(urlEl?.textContent).toBe("https://example.com/?q='test'&x=\"value\"");
			// Verify no script execution or unintended HTML parsing
			expect(urlEl?.querySelectorAll("*").length).toBe(0);

			dialog.close();
			await promise;
		});

		test('should resolve true on Open click', async () => {
			const promise = dialog.confirm("https://example.com");

			const openBtn = document.querySelector(
				".link-confirm-open",
			) as HTMLElement;
			openBtn.click();

			const result = await promise;
			expect(result).toBe(true);
		});

		test('should resolve false on Cancel click', async () => {
			const promise = dialog.confirm("https://example.com");

			const cancelBtn = document.querySelector(
				".link-confirm-cancel",
			) as HTMLElement;
			cancelBtn.click();

			const result = await promise;
			expect(result).toBe(false);
		});

		test("should resolve false on overlay click", async () => {
			const promise = dialog.confirm("https://example.com");

			const overlay = document.querySelector(
				".link-confirm-dialog-overlay",
			) as HTMLElement;
			// Click on overlay directly (not the dialog inside)
			overlay.click();

			const result = await promise;
			expect(result).toBe(false);
		});

		test("should resolve false on Escape key", async () => {
			const promise = dialog.confirm("https://example.com");

			const event = new KeyboardEvent("keydown", {
				key: "Escape",
				bubbles: true,
			});
			document.dispatchEvent(event);

			const result = await promise;
			expect(result).toBe(false);
		});

		test("should resolve true on Enter key", async () => {
			const promise = dialog.confirm("https://example.com");

			const event = new KeyboardEvent("keydown", {
				key: "Enter",
				bubbles: true,
			});
			document.dispatchEvent(event);

			const result = await promise;
			expect(result).toBe(true);
		});
	});

	describe("close", () => {
		test("should remove dialog from DOM", async () => {
			const promise = dialog.confirm("https://example.com");

			expect(
				document.querySelector(".link-confirm-dialog-overlay"),
			).not.toBeNull();

			dialog.close();

			expect(document.querySelector(".link-confirm-dialog-overlay")).toBeNull();

			const result = await promise;
			expect(result).toBe(false);
		});
	});

	describe("isShown", () => {
		test("should return false initially", () => {
			expect(dialog.isShown()).toBe(false);
		});

		test("should return true after confirm is called", async () => {
			const promise = dialog.confirm("https://example.com");

			expect(dialog.isShown()).toBe(true);

			dialog.close();
			await promise;
		});

		test("should return false after close", async () => {
			const promise = dialog.confirm("https://example.com");
			dialog.close();
			await promise;

			expect(dialog.isShown()).toBe(false);
		});
	});

	describe("dispose", () => {
		test("should close dialog", async () => {
			const promise = dialog.confirm("https://example.com");

			dialog.dispose();

			expect(document.querySelector(".link-confirm-dialog-overlay")).toBeNull();

			const result = await promise;
			expect(result).toBe(false);
		});
	});
});
