/**
 * Test setup file for Bun tests.
 * Configures happy-dom for DOM environment emulation.
 */

import { Window } from "happy-dom";

const window = new Window();

// happy-dom's Node.prototype.nodeName getter is an abstract stub that
// always returns "" — each concrete subclass (Element, Text, Comment, ...)
// defines its own overriding nodeName getter instead. That is transparent
// to ordinary property access (`node.nodeName` resolves through the normal
// prototype chain to the subclass's getter), but a library that reads
// `Object.getOwnPropertyDescriptor(Node.prototype, "nodeName")` directly —
// bypassing the override — gets the base stub instead, so every node
// reports an empty node name. Real browsers implement nodeName as a single
// dispatching getter directly on Node.prototype, so this divergence is
// invisible there. Patch the stub to walk the prototype chain to the
// nearest actual override, matching what normal property access already
// does, so the test DOM environment matches browser-observable behavior.
{
  const NodeCtor = (window as unknown as { Node: { prototype: object } }).Node;
  const nodeProto = NodeCtor.prototype;
  const baseDescriptor = Object.getOwnPropertyDescriptor(nodeProto, "nodeName");
  if (baseDescriptor?.get) {
    Object.defineProperty(nodeProto, "nodeName", {
      configurable: true,
      enumerable: baseDescriptor.enumerable,
      get(this: object) {
        let proto = Object.getPrototypeOf(this);
        while (proto && proto !== nodeProto) {
          const descriptor = Object.getOwnPropertyDescriptor(proto, "nodeName");
          if (descriptor?.get) {
            return descriptor.get.call(this);
          }
          proto = Object.getPrototypeOf(proto);
        }
        return baseDescriptor.get?.call(this);
      },
    });
  }
}

// Register global DOM objects
globalThis.document = window.document as unknown as Document;
globalThis.window = window as unknown as Window & typeof globalThis;
globalThis.KeyboardEvent =
  window.KeyboardEvent as unknown as typeof KeyboardEvent;
globalThis.HTMLElement = window.HTMLElement as unknown as typeof HTMLElement;
globalThis.getComputedStyle = window.getComputedStyle.bind(
  window,
) as typeof getComputedStyle;
globalThis.ResizeObserver =
  window.ResizeObserver as unknown as typeof ResizeObserver;
globalThis.Event = window.Event as unknown as typeof Event;
// WheelEvent may not be available in happy-dom, use Event as fallback
globalThis.WheelEvent = (window.WheelEvent ??
  window.Event) as unknown as typeof WheelEvent;
// MouseEvent — happy-dom provides it, but it is not exposed on globalThis by default.
globalThis.MouseEvent = window.MouseEvent as unknown as typeof MouseEvent;
// Performance API
globalThis.performance = window.performance as unknown as Performance;

// Polyfill requestAnimationFrame for tests
globalThis.requestAnimationFrame = (callback: FrameRequestCallback): number => {
  return setTimeout(() => callback(Date.now()), 0) as unknown as number;
};
globalThis.cancelAnimationFrame = (id: number): void => {
  clearTimeout(id);
};

// Set default locale to English for deterministic test results
import { initI18n } from "./src-tauri/web-shared/i18n/index.ts";
initI18n("en");
