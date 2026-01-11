/**
 * Integration tests for Markdown display with TerminalState.
 */
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { TerminalState } from "../terminal/state.ts";
import type { TerminalAction } from "../types/terminal.ts";

describe("Markdown Display Integration", () => {
	let state: TerminalState;

	beforeEach(() => {
		state = new TerminalState(80, 24);
	});

	afterEach(() => {
		state.reset();
	});

	/**
	 * Helper to create an EmtermExtension action.
	 */
	function createEmtermAction(verb: string, params: string[]): TerminalAction {
		return {
			type: "Osc",
			value: {
				action: "EmtermExtension",
				verb,
				params,
			},
		};
	}

	test("should render markdown from OSC sequence", () => {
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

		// End session
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=test-1"]),
		);

		// Check pending blocks
		const blocks = state.takePendingMarkdownBlocks();
		expect(blocks.length).toBe(1);
		expect(blocks[0].id).toBe("test-1");
		expect(blocks[0].html).toContain("<h1");
		expect(blocks[0].html).toContain("Hello World");
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

		const blocks = state.takePendingMarkdownBlocks();
		expect(blocks.length).toBe(1);
		expect(blocks[0].html).toContain("<h1");
		expect(blocks[0].html).toContain("<strong>");
		expect(blocks[0].html).toContain("bold");
	});

	test("should handle multiple concurrent sessions", () => {
		// Start two sessions
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=session-a"]),
		);
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=session-b"]),
		);

		// Send chunks to both
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
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=session-b",
				"seq=0",
				`data=${btoa("# Session B")}`,
			]),
		);

		// End session A first
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=session-a"]),
		);

		let blocks = state.takePendingMarkdownBlocks();
		expect(blocks.length).toBe(1);
		expect(blocks[0].id).toBe("session-a");
		expect(blocks[0].html).toContain("Session A");

		// End session B
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=session-b"]),
		);

		blocks = state.takePendingMarkdownBlocks();
		expect(blocks.length).toBe(1);
		expect(blocks[0].id).toBe("session-b");
		expect(blocks[0].html).toContain("Session B");
	});

	test("should set block startRow from cursor position", () => {
		// Move cursor down
		state.processAction({
			type: "Csi",
			value: { action: "CursorPosition", data: { row: 10, col: 1 } },
		});

		// Begin and complete a markdown session
		state.processAction(
			createEmtermAction("emterm", ["markdown", "begin", "id=pos-test"]),
		);
		state.processAction(
			createEmtermAction("emterm", [
				"markdown",
				"chunk",
				"id=pos-test",
				"seq=0",
				`data=${btoa("Test")}`,
			]),
		);
		state.processAction(
			createEmtermAction("emterm", ["markdown", "end", "id=pos-test"]),
		);

		const blocks = state.takePendingMarkdownBlocks();
		expect(blocks[0].startRow).toBe(9); // 0-indexed, so row 10 becomes 9
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

		const blocks = state.takePendingMarkdownBlocks();
		expect(blocks[0].html).not.toContain("<script>");
		expect(blocks[0].html).not.toContain("alert");
	});
});
