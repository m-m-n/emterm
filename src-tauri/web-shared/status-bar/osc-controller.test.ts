import { describe, test, expect, beforeEach } from "bun:test";
import { OscLayerController, stripHtmlTags } from "./osc-controller";
import { StatusBarRenderer } from "./renderer";

describe("stripHtmlTags", () => {
  test("should strip simple HTML tags", () => {
    expect(stripHtmlTags("<b>bold</b>")).toBe("bold");
  });

  test("should strip tags with attributes", () => {
    expect(stripHtmlTags('<a href="http://example.com">link</a>')).toBe("link");
  });

  test("should strip nested tags", () => {
    expect(stripHtmlTags("<div><span>text</span></div>")).toBe("text");
  });

  test("should handle text without tags", () => {
    expect(stripHtmlTags("plain text")).toBe("plain text");
  });

  test("should handle empty string", () => {
    expect(stripHtmlTags("")).toBe("");
  });

  test("should strip self-closing tags", () => {
    expect(stripHtmlTags("before<br/>after")).toBe("beforeafter");
  });

  test("should strip script tags and content", () => {
    expect(stripHtmlTags("<script>alert('xss')</script>safe")).toBe("safe");
  });

  test("should strip style tags and content", () => {
    expect(stripHtmlTags("<style>body{}</style>safe")).toBe("safe");
  });

  test("should handle angle brackets in text (non-HTML)", () => {
    expect(stripHtmlTags("1 < 2 > 0")).toBe("1 < 2 > 0");
  });
});

describe("OscLayerController", () => {
  let container: HTMLElement;
  let renderer: StatusBarRenderer;
  let controller: OscLayerController;

  beforeEach(() => {
    container = document.createElement("div");
    renderer = new StatusBarRenderer(container);
    controller = new OscLayerController(renderer);
  });

  test("should set left content with HTML stripped", () => {
    controller.handleCommand("set", "left", '<b>hello</b> world');
    expect(renderer.getContent("osc", "left")).toBe("hello world");
  });

  test("should set right content with HTML stripped", () => {
    controller.handleCommand("set", "right", "status info");
    expect(renderer.getContent("osc", "right")).toBe("status info");
  });

  test("should clear all content", () => {
    controller.handleCommand("set", "left", "left text");
    controller.handleCommand("set", "right", "right text");
    controller.handleCommand("clear");
    expect(renderer.getContent("osc", "left")).toBe("");
    expect(renderer.getContent("osc", "right")).toBe("");
  });

  test("should clear left only", () => {
    controller.handleCommand("set", "left", "left text");
    controller.handleCommand("set", "right", "right text");
    controller.handleCommand("clear", "left");
    expect(renderer.getContent("osc", "left")).toBe("");
    expect(renderer.getContent("osc", "right")).toBe("right text");
  });

  test("should clear right only", () => {
    controller.handleCommand("set", "left", "left text");
    controller.handleCommand("set", "right", "right text");
    controller.handleCommand("clear", "right");
    expect(renderer.getContent("osc", "left")).toBe("left text");
    expect(renderer.getContent("osc", "right")).toBe("");
  });

  test("should show OSC layer explicitly", () => {
    controller.handleCommand("show");
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(false);
  });

  test("should hide OSC layer explicitly", () => {
    controller.handleCommand("set", "left", "content");
    controller.handleCommand("hide");
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(true);
  });

  test("should auto-show OSC layer when content is set", () => {
    controller.handleCommand("set", "left", "auto-show");
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(false);
  });

  test("should ignore unknown commands", () => {
    // Should not throw
    controller.handleCommand("unknown");
    controller.handleCommand("invalid", "param");
  });

  test("should strip script tags from OSC content", () => {
    controller.handleCommand("set", "left", '<script>alert("xss")</script>safe');
    expect(renderer.getContent("osc", "left")).toBe("safe");
  });
});
