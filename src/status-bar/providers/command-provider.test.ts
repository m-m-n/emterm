import { describe, test, expect } from "bun:test";
import { CommandProvider } from "./command-provider";

describe("CommandProvider", () => {
  test("should return empty string initially", () => {
    const provider = new CommandProvider(
      "/usr/bin/hostname",
      async () => "myhost",
      5000,
    );
    // Before first execution completes
    expect(provider.getValue()).toBe("");
    provider.dispose();
  });

  test("should update value after execution", async () => {
    const provider = new CommandProvider(
      "/usr/bin/hostname",
      async () => "myhost\n",
      60000, // long interval to avoid re-execution
    );
    // Wait for initial execution
    await new Promise((r) => setTimeout(r, 50));
    expect(provider.getValue()).toBe("myhost");
    provider.dispose();
  });

  test("should return empty string on execution failure", async () => {
    const provider = new CommandProvider(
      "/usr/bin/failing-cmd",
      async () => { throw new Error("exec failed"); },
      60000,
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(provider.getValue()).toBe("");
    provider.dispose();
  });

  test("should trim output", async () => {
    const provider = new CommandProvider(
      "/usr/bin/test",
      async () => "  trimmed  \n",
      60000,
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(provider.getValue()).toBe("trimmed");
    provider.dispose();
  });

  test("should not have color", () => {
    const provider = new CommandProvider(
      "/usr/bin/test",
      async () => "value",
      60000,
    );
    expect(provider.getColor()).toBeNull();
    provider.dispose();
  });

  test("should clean up interval on dispose", () => {
    const provider = new CommandProvider(
      "/usr/bin/test",
      async () => "value",
      1000,
    );
    provider.dispose();
    // Ensure no error after dispose
    expect(provider.getValue()).toBe("");
  });
});
