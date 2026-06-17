import { describe, test, expect } from "bun:test";
import { extractBasename } from "./cwd-provider";

describe("extractBasename", () => {
  test("should extract basename from Unix path", () => {
    expect(extractBasename("/home/user/projects/myapp")).toBe("myapp");
  });

  test("should extract basename from Windows path", () => {
    expect(extractBasename("C:\\Users\\user\\projects\\myapp")).toBe("myapp");
  });

  test("should handle root path", () => {
    expect(extractBasename("/")).toBe("/");
  });

  test("should handle Windows drive root", () => {
    expect(extractBasename("C:\\")).toBe("C:\\");
  });

  test("should handle single directory name", () => {
    expect(extractBasename("myproject")).toBe("myproject");
  });

  test("should handle empty string", () => {
    expect(extractBasename("")).toBe("");
  });

  test("should handle trailing slash", () => {
    expect(extractBasename("/home/user/projects/")).toBe("projects");
  });

  test("should handle file:// URI (OSC 7 format)", () => {
    expect(extractBasename("file:///home/user/projects/myapp")).toBe("myapp");
  });

  test("should handle file:// URI with hostname", () => {
    expect(extractBasename("file://hostname/home/user/myapp")).toBe("myapp");
  });

  test("should decode percent-encoded characters", () => {
    expect(extractBasename("file:///home/user/my%20project")).toBe("my project");
  });
});
