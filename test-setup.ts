/**
 * Test setup file for Bun tests.
 * Configures happy-dom for DOM environment emulation.
 */

import { Window } from "happy-dom";

const window = new Window();

// Register global DOM objects
globalThis.document = window.document as unknown as Document;
globalThis.window = window as unknown as Window & typeof globalThis;
globalThis.KeyboardEvent = window.KeyboardEvent as unknown as typeof KeyboardEvent;
globalThis.HTMLElement = window.HTMLElement as unknown as typeof HTMLElement;
globalThis.getComputedStyle = window.getComputedStyle.bind(window) as typeof getComputedStyle;
globalThis.ResizeObserver = window.ResizeObserver as unknown as typeof ResizeObserver;
