import { describe, test, expect } from "bun:test";
import { parseGitBranch, parseGitStatus, getGitStateColor } from "./git-provider";

describe("parseGitBranch", () => {
  test("should parse branch name from git output", () => {
    expect(parseGitBranch("main\n")).toBe("main");
  });

  test("should parse feature branch", () => {
    expect(parseGitBranch("feature/status-bar\n")).toBe("feature/status-bar");
  });

  test("should handle detached HEAD", () => {
    expect(parseGitBranch("HEAD\n")).toBe("HEAD");
  });

  test("should trim whitespace", () => {
    expect(parseGitBranch("  main  \n")).toBe("main");
  });

  test("should return empty for empty output", () => {
    expect(parseGitBranch("")).toBe("");
  });

  test("should return empty for error output", () => {
    expect(parseGitBranch("fatal: not a git repository")).toBe("");
  });
});

describe("parseGitStatus", () => {
  test("should detect clean state", () => {
    expect(parseGitStatus("")).toBe("clean");
  });

  test("should detect dirty state (modified files)", () => {
    expect(parseGitStatus(" M src/main.ts\n")).toBe("dirty");
  });

  test("should detect dirty state (staged files)", () => {
    expect(parseGitStatus("M  src/main.ts\n")).toBe("dirty");
  });

  test("should detect untracked-only state", () => {
    expect(parseGitStatus("?? new-file.ts\n")).toBe("untracked");
  });

  test("should detect dirty when mixed with untracked", () => {
    expect(parseGitStatus(" M src/main.ts\n?? new.ts\n")).toBe("dirty");
  });
});

describe("getGitStateColor", () => {
  test("should return green-ish for clean", () => {
    const color = getGitStateColor("clean");
    expect(color).toBeTruthy();
  });

  test("should return yellow-ish for dirty", () => {
    const color = getGitStateColor("dirty");
    expect(color).toBeTruthy();
    expect(color).not.toBe(getGitStateColor("clean"));
  });

  test("should return dim color for untracked", () => {
    const color = getGitStateColor("untracked");
    expect(color).toBeTruthy();
  });

  test("should return null for empty (not a git repo)", () => {
    const color = getGitStateColor("");
    expect(color).toBeNull();
  });
});
