/**
 * Tests for shared DOM utility functions.
 *
 * @module shared/dom-utils.test
 */

import { describe, test, expect, afterEach } from "bun:test";
import { isAncestorHidden, isModalOverlayVisible } from "./dom-utils.ts";

describe("isAncestorHidden", () => {
	let root: HTMLElement;

	afterEach(() => {
		root?.remove();
	});

	test("returns false when no ancestor is hidden", () => {
		root = document.createElement("div");
		const child = document.createElement("div");
		root.appendChild(child);
		document.body.appendChild(root);

		expect(isAncestorHidden(child)).toBe(false);
	});

	test("returns true when parent has display:none", () => {
		root = document.createElement("div");
		root.style.display = "none";
		const child = document.createElement("div");
		root.appendChild(child);
		document.body.appendChild(root);

		expect(isAncestorHidden(child)).toBe(true);
	});

	test("returns true when grandparent has display:none", () => {
		root = document.createElement("div");
		root.style.display = "none";
		const middle = document.createElement("div");
		const child = document.createElement("div");
		root.appendChild(middle);
		middle.appendChild(child);
		document.body.appendChild(root);

		expect(isAncestorHidden(child)).toBe(true);
	});

	test("returns false for element with no parent", () => {
		const orphan = document.createElement("div");
		expect(isAncestorHidden(orphan)).toBe(false);
	});
});

describe("isModalOverlayVisible", () => {
	let container: HTMLElement;

	afterEach(() => {
		container?.remove();
	});

	test("returns false when no overlays exist", () => {
		expect(isModalOverlayVisible()).toBe(false);
	});

	test("returns true when image viewer overlay is visible", () => {
		container = document.createElement("div");
		const overlay = document.createElement("div");
		overlay.className = "image-viewer-overlay visible";
		container.appendChild(overlay);
		document.body.appendChild(container);

		expect(isModalOverlayVisible()).toBe(true);
	});

	test("returns true when markdown fullscreen overlay is visible", () => {
		container = document.createElement("div");
		const overlay = document.createElement("div");
		overlay.className = "markdown-fullscreen-overlay visible";
		container.appendChild(overlay);
		document.body.appendChild(container);

		expect(isModalOverlayVisible()).toBe(true);
	});

	test("returns false when overlay is visible but in a hidden tab", () => {
		container = document.createElement("div");
		container.style.display = "none";
		const overlay = document.createElement("div");
		overlay.className = "image-viewer-overlay visible";
		container.appendChild(overlay);
		document.body.appendChild(container);

		expect(isModalOverlayVisible()).toBe(false);
	});

	test("returns false when overlay exists but lacks visible class", () => {
		container = document.createElement("div");
		const overlay = document.createElement("div");
		overlay.className = "image-viewer-overlay";
		container.appendChild(overlay);
		document.body.appendChild(container);

		expect(isModalOverlayVisible()).toBe(false);
	});
});
