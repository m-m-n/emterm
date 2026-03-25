import { describe, test, expect, beforeEach } from "bun:test";
import { StatusBarRenderer } from "./renderer";

// Minimal DOM mock for Bun test environment
function createMockContainer(): HTMLElement {
  const el = document.createElement("div");
  return el;
}

describe("StatusBarRenderer", () => {
  let container: HTMLElement;
  let renderer: StatusBarRenderer;

  beforeEach(() => {
    container = createMockContainer();
    renderer = new StatusBarRenderer(container);
  });

  test("should create 3 layers with left/right sections", () => {
    const layers = container.querySelectorAll(".status-bar-layer");
    expect(layers.length).toBe(3);

    const sections = container.querySelectorAll(".status-bar-section");
    expect(sections.length).toBe(6); // 3 layers * 2 sections each
  });

  test("should have correct layer data attributes", () => {
    expect(container.querySelector('[data-layer="osc"]')).toBeTruthy();
    expect(container.querySelector('[data-layer="app-line1"]')).toBeTruthy();
    expect(container.querySelector('[data-layer="app-line2"]')).toBeTruthy();
  });

  test("should hide OSC layer by default (empty content)", () => {
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(true);
  });

  test("should hide app-line2 layer by default (empty content)", () => {
    const line2 = container.querySelector('[data-layer="app-line2"]') as HTMLElement;
    expect(line2.classList.contains("hidden")).toBe(true);
  });

  test("should keep app-line1 always visible", () => {
    const line1 = container.querySelector('[data-layer="app-line1"]') as HTMLElement;
    expect(line1.classList.contains("hidden")).toBe(false);
  });

  test("should set content and update layer visibility", () => {
    renderer.setContent("osc", "left", "Hello");
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(false);

    const leftSection = oscLayer.querySelector('[data-section="left"]') as HTMLElement;
    expect(leftSection.innerHTML).toBe("Hello");
  });

  test("should hide layer when content is cleared", () => {
    renderer.setContent("osc", "left", "Hello");
    renderer.clearContent("osc");
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(true);
  });

  test("should clear specific section only", () => {
    renderer.setContent("osc", "left", "Left");
    renderer.setContent("osc", "right", "Right");

    renderer.clearContent("osc", "left");

    expect(renderer.getContent("osc", "left")).toBe("");
    expect(renderer.getContent("osc", "right")).toBe("Right");
  });

  test("should show app-line2 when content is set", () => {
    renderer.setContent("app-line2", "left", "Line 2 content");
    const line2 = container.querySelector('[data-layer="app-line2"]') as HTMLElement;
    expect(line2.classList.contains("hidden")).toBe(false);
  });

  test("should apply config with custom colors", () => {
    renderer.applyConfig({
      enabled: true,
      appLine1Left: "",
      appLine1Right: "",
      appLine2Left: "",
      appLine2Right: "",
      timeFormat: "HH:mm:ss",
      fontSize: 12,
      bgColor: "#1a1a1a",
      fgColor: "#e0e0e0",
      opacity: 0.8,
    });

    expect(container.style.getPropertyValue("--status-bar-bg")).toBe("#1a1a1a");
    expect(container.style.getPropertyValue("--status-bar-fg")).toBe("#e0e0e0");
    expect(container.style.getPropertyValue("--status-bar-font-size")).toBe("12pt");
    expect(container.style.getPropertyValue("--status-bar-opacity")).toBe("0.8");
  });

  test("should remove color properties when empty", () => {
    renderer.applyConfig({
      enabled: true,
      appLine1Left: "",
      appLine1Right: "",
      appLine2Left: "",
      appLine2Right: "",
      timeFormat: "HH:mm:ss",
      fontSize: null,
      bgColor: "",
      fgColor: "",
      opacity: 1.0,
    });

    expect(container.style.getPropertyValue("--status-bar-bg")).toBe("");
    expect(container.style.getPropertyValue("--status-bar-fg")).toBe("");
    expect(container.style.getPropertyValue("--status-bar-font-size")).toBe("");
  });

  test("should not update DOM when content is unchanged (differential rendering)", () => {
    renderer.setContent("app-line1", "left", "Same");
    const el = renderer.getSection("app-line1", "left")!;
    const original = el.innerHTML;

    // Set same content again - should not trigger DOM write
    renderer.setContent("app-line1", "left", "Same");
    expect(el.innerHTML).toBe(original);
  });

  test("should explicitly show/hide layers", () => {
    renderer.setLayerVisible("osc", true);
    const oscLayer = container.querySelector('[data-layer="osc"]') as HTMLElement;
    expect(oscLayer.classList.contains("hidden")).toBe(false);

    renderer.setLayerVisible("osc", false);
    expect(oscLayer.classList.contains("hidden")).toBe(true);
  });

  test("should clean up on dispose", () => {
    renderer.dispose();
    expect(container.innerHTML).toBe("");
  });
});
