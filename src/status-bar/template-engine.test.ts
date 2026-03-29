import { describe, test, expect, beforeEach } from "bun:test";
import { TemplateEngine } from "./template-engine";
import type { VariableProvider } from "./providers/types";

/** Create a mock provider with a static value. */
function mockProvider(value: string, color?: string): VariableProvider {
  return {
    getValue: () => value,
    getColor: color !== undefined ? () => color : undefined,
    dispose: () => {},
  };
}

describe("TemplateEngine", () => {
  let engine: TemplateEngine;

  beforeEach(() => {
    engine = new TemplateEngine();
  });

  test("should parse template and extract variable names", () => {
    const vars = TemplateEngine.extractVariables("{time} - {cwd}");
    expect(vars).toEqual(["time", "cwd"]);
  });

  test("should handle template with no variables", () => {
    const vars = TemplateEngine.extractVariables("static text");
    expect(vars).toEqual([]);
  });

  test("should handle template with duplicate variables", () => {
    const vars = TemplateEngine.extractVariables("{time} and {time}");
    expect(vars).toEqual(["time", "time"]);
  });

  test("should extract cmd:name variables", () => {
    const vars = TemplateEngine.extractVariables("{cmd:hostname}");
    expect(vars).toEqual(["cmd:hostname"]);
  });

  test("should extract hyphenated cmd:name variables", () => {
    const vars = TemplateEngine.extractVariables("{cmd:load-average}");
    expect(vars).toEqual(["cmd:load-average"]);
  });

  test("should resolve hyphenated cmd:name variables", () => {
    engine.registerProvider("cmd:load-average", mockProvider("0.42"));

    const result = engine.resolve("Load: {cmd:load-average}");
    expect(result).toBe("Load: 0.42");
  });

  test("should resolve template with registered providers", () => {
    engine.registerProvider("time", mockProvider("12:30:00"));
    engine.registerProvider("cwd", mockProvider("myproject"));

    const result = engine.resolve("{time} | {cwd}");
    expect(result).toBe("12:30:00 | myproject");
  });

  test("should resolve unknown variables as empty string", () => {
    engine.registerProvider("time", mockProvider("12:30:00"));

    const result = engine.resolve("{time} {unknown}");
    expect(result).toBe("12:30:00 ");
  });

  test("should resolve cmd:name variables", () => {
    engine.registerProvider("cmd:hostname", mockProvider("myhost"));

    const result = engine.resolve("Host: {cmd:hostname}");
    expect(result).toBe("Host: myhost");
  });

  test("should return static text unchanged", () => {
    const result = engine.resolve("Hello World");
    expect(result).toBe("Hello World");
  });

  test("should return empty string for empty template", () => {
    const result = engine.resolve("");
    expect(result).toBe("");
  });

  test("should handle provider returning empty string", () => {
    engine.registerProvider("git_branch", mockProvider(""));

    const result = engine.resolve("branch: {git_branch}");
    expect(result).toBe("branch: ");
  });

  test("should resolve template with color-enabled provider", () => {
    engine.registerProvider("git_branch", mockProvider("main", "#00ff00"));

    const result = engine.resolveWithColors("{git_branch}");
    expect(result).toContain("main");
    expect(result).toContain("#00ff00");
  });

  test("should not wrap in color span when provider has no color", () => {
    engine.registerProvider("time", mockProvider("12:00:00"));

    const result = engine.resolveWithColors("{time}");
    expect(result).toBe("12:00:00");
    expect(result).not.toContain("span");
  });

  test("should dispose all providers", () => {
    let disposed1 = false;
    let disposed2 = false;
    engine.registerProvider("a", {
      getValue: () => "a",
      dispose: () => { disposed1 = true; },
    });
    engine.registerProvider("b", {
      getValue: () => "b",
      dispose: () => { disposed2 = true; },
    });

    engine.dispose();
    expect(disposed1).toBe(true);
    expect(disposed2).toBe(true);
  });

  test("should unregister provider", () => {
    engine.registerProvider("time", mockProvider("12:00:00"));
    engine.unregisterProvider("time");

    const result = engine.resolve("{time}");
    expect(result).toBe("");
  });
});
