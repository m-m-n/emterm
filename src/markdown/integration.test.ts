/**
 * Integration tests for Markdown display with TerminalState.
 *
 * Note: Markdown is always displayed in fullscreen mode (like `less` command).
 */
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { TerminalState } from "../terminal/state.ts";
import type { TerminalAction } from "../types/terminal.ts";

describe("Markdown Display Integration", () => {
	let state: TerminalState;
	let container: HTMLElement;

	beforeEach(() => {
		state = new TerminalState(80, 24);
		// Create container for fullscreen view
		container = document.createElement("div");
		container.className = "overlay-root";
		document.body.appendChild(container);
		// Set container on markdown manager
		state.getMarkdownManager().setContainer(container);
	});

	afterEach(() => {
		state.reset();
		// Clean up any fullscreen overlays
		document.querySelectorAll(".markdown-fullscreen-overlay").forEach((el) => {
			el.remove();
		});
		container.remove();
	});

	/**
	 * Helper to create an EmtermExtension action.
	 */
	function createEmtermAction(verb: string, params: string[]): TerminalAction {
		return {
			type: "Osc",
			value: {
				action: "EmtermExtension",
				data: { verb, params },
			},
		};
	}

	test("should render markdown in fullscreen from OSC sequence", () => {
		// Begin session
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"begin",
				"id=test-1",
				"format=gfm",
			]),
		);

		// Send chunk with Base64 encoded "# Hello World"
		const content = btoa("# Hello World");
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=test-1",
				"seq=0",
				`data=${content}`,
			]),
		);

		// End session - this should show fullscreen overlay
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=test-1"]),
		);

		// Check fullscreen overlay is shown (inside container)
		const overlay = container.querySelector(".markdown-fullscreen-overlay");
		expect(overlay).not.toBeNull();

		const content_el = overlay?.querySelector(".markdown-fullscreen-content");
		expect(content_el).not.toBeNull();
		expect(content_el?.innerHTML).toContain("Hello World");
	});

	test("should handle chunked transfer", () => {
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=chunked-test"]),
		);

		// Send multiple chunks
		const chunk1 = btoa("# Title\n\n");
		const chunk2 = btoa("This is ");
		const chunk3 = btoa("**bold** text.");

		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=chunked-test",
				"seq=0",
				`data=${chunk1}`,
			]),
		);
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=chunked-test",
				"seq=1",
				`data=${chunk2}`,
			]),
		);
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=chunked-test",
				"seq=2",
				`data=${chunk3}`,
			]),
		);

		// End session
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=chunked-test"]),
		);

		// Check fullscreen content (inside container)
		const content = container.querySelector(".markdown-fullscreen-content");
		expect(content).not.toBeNull();
		expect(content?.innerHTML).toContain("<h1");
		expect(content?.innerHTML).toContain("<strong>");
		expect(content?.innerHTML).toContain("bold");
	});

	test("should handle multiple sequential sessions", () => {
		// First session
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=session-a"]),
		);
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=session-a",
				"seq=0",
				`data=${btoa("# Session A")}`,
			]),
		);
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=session-a"]),
		);

		// Verify first session shown (inside container)
		let content = container.querySelector(".markdown-fullscreen-content");
		expect(content?.innerHTML).toContain("Session A");

		// Close first overlay
		container.querySelector(".markdown-fullscreen-overlay")?.remove();

		// Second session
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=session-b"]),
		);
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=session-b",
				"seq=0",
				`data=${btoa("# Session B")}`,
			]),
		);
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=session-b"]),
		);

		// Verify second session shown (inside container)
		content = container.querySelector(".markdown-fullscreen-content");
		expect(content?.innerHTML).toContain("Session B");
	});

	test("should ignore non-emterm commands", () => {
		state.processAction(
			createEmtermAction("other-app", ["markdown", "begin", "id=test"]),
		);

		const manager = state.getMarkdownManager();
		expect(manager.sessionCount).toBe(0);
	});

	test("should ignore non-markdown commands", () => {
		state.processAction(
			createEmtermAction("emterm", ["image", "begin", "id=test"]),
		);

		const manager = state.getMarkdownManager();
		expect(manager.sessionCount).toBe(0);
	});

	test("should clear markdown state on reset", () => {
		// Create a session
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=reset-test"]),
		);

		// Verify session exists
		let manager = state.getMarkdownManager();
		expect(manager.sessionCount).toBe(1);

		// Reset
		state.reset();

		// Verify session is cleared
		manager = state.getMarkdownManager();
		expect(manager.sessionCount).toBe(0);
	});

	test("should sanitize XSS in rendered content", () => {
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=xss-test"]),
		);
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=xss-test",
				"seq=0",
				`data=${btoa("<script>alert('xss')</script>")}`,
			]),
		);
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=xss-test"]),
		);

		// Check fullscreen content (inside container)
		const content = container.querySelector(".markdown-fullscreen-content");
		expect(content?.innerHTML).not.toContain("<script>");
		expect(content?.innerHTML).not.toContain("alert");
	});
});
