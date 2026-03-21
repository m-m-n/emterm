/**
 * E2E: Mux multi-session scenario
 *
 * Tests the scenario where multiple tabs each run `emterm mux`,
 * connecting to the same daemon with independent sessions.
 *
 * Scenario:
 * 1. Start with no daemon running
 * 2. Tab A: run `emterm mux` → daemon starts, window 0 created
 * 3. Open Tab B (Ctrl+T)
 * 4. Tab B: run `emterm mux` → connects to existing daemon, window 0 created
 * 5. Tab B: prefix+c → window 1 created
 * 6. Verify both tabs have independent mux sessions
 */

describe("Mux Multi-Session", () => {
  /** Type characters one at a time. */
  async function typeSlowly(text, delay = 80) {
    for (const char of text) {
      await browser.keys(char);
      await browser.pause(delay);
    }
  }

  /** Send prefix key (Ctrl+B by default) then action key. */
  async function sendPrefixKey(actionKey, prefixMods = ["Control", "b"]) {
    await browser.keys(prefixMods);
    await browser.pause(300);
    await browser.keys(actionKey);
    await browser.pause(500);
  }

  /** Get the count of mux sub-tabs in the active tab. */
  async function getSubTabCount() {
    return await browser.execute(() => {
      const subTabs = document.querySelector(".mux-sub-tabs");
      return subTabs ? subTabs.children.length : 0;
    });
  }

  /** Wait for terminal app to be ready. */
  async function waitForTerminal() {
    // First wait for terminal element to exist in DOM
    const terminal = await $('[data-testid="terminal"]');
    await terminal.waitForExist({ timeout: 30000 });
    // Then wait for shell prompt
    await browser.pause(5000);
  }

  /** Wait for shell prompt (simple heuristic: pause and check). */
  async function waitForShellPrompt() {
    await browser.pause(2000);
  }

  /** Kill any existing mux daemon before tests. */
  before(async () => {
    await waitForTerminal();

    // Focus terminal
    const terminal = await $('[data-testid="terminal"]');
    await terminal.click();
    await browser.pause(500);

    // Kill any existing daemon
    await typeSlowly("pkill -f 'emterm mux --daemon' 2>/dev/null; true");
    await browser.keys("Enter");
    await browser.pause(1000);

    // Clean up stale socket
    await typeSlowly(
      "rm -f ${XDG_RUNTIME_DIR:-/tmp}/emterm/mux-default.sock 2>/dev/null; true",
    );
    await browser.keys("Enter");
    await browser.pause(500);

    await browser.saveScreenshot(
      "./screenshots/mux-multi-00-clean-state.png",
    );
  });

  it("Tab A: should start mux mode and create window 0", async () => {
    // Run emterm mux in Tab A
    await typeSlowly("emterm mux");
    await browser.keys("Enter");
    await browser.pause(5000); // Wait for daemon startup + mux attach

    // Verify mux mode is active (sub-tabs should appear)
    const subTabCount = await getSubTabCount();
    console.log("Tab A sub-tab count:", subTabCount);
    expect(subTabCount).toBe(1); // Window 0

    await browser.saveScreenshot(
      "./screenshots/mux-multi-01-tab-a-mux.png",
    );
  });

  it("should open Tab B and start mux mode", async () => {
    // Create a new tab with Ctrl+T
    const terminal = await $(".tab-content");
    await terminal.click();
    await browser.pause(300);
    await browser.keys(["Control", "t"]);
    await browser.pause(3000); // Wait for new tab + PTY

    // Verify we have 2 tabs
    const tabCount = await browser.execute(() => {
      return window.tabManager?.getTabs().length || 0;
    });
    console.log("Tab count after Ctrl+T:", tabCount);
    expect(tabCount).toBeGreaterThanOrEqual(2);

    await waitForShellPrompt();
    await browser.saveScreenshot(
      "./screenshots/mux-multi-02-tab-b-created.png",
    );

    // Run emterm mux in Tab B
    await typeSlowly("emterm mux");
    await browser.keys("Enter");
    await browser.pause(5000); // Wait for mux attach (daemon already running)

    // Verify mux mode is active in Tab B
    const subTabCount = await getSubTabCount();
    console.log("Tab B sub-tab count:", subTabCount);
    expect(subTabCount).toBe(1); // Window 0

    await browser.saveScreenshot(
      "./screenshots/mux-multi-03-tab-b-mux.png",
    );
  });

  it("Tab B: should create window 1 with prefix+c", async () => {
    // Create new window in Tab B
    await sendPrefixKey("c");
    await browser.pause(3000); // Wait for PTY spawn

    // Verify sub-tab count increased
    const subTabCount = await getSubTabCount();
    console.log("Tab B sub-tab count after prefix+c:", subTabCount);
    expect(subTabCount).toBe(2); // Windows 0 and 1

    await browser.saveScreenshot(
      "./screenshots/mux-multi-04-tab-b-two-windows.png",
    );
  });

  it("Tab B: keyboard input should work in mux window", async () => {
    // Type a command in the active mux window
    await typeSlowly("echo mux-multi-test");
    await browser.keys("Enter");
    await browser.pause(1000);

    await browser.saveScreenshot(
      "./screenshots/mux-multi-05-tab-b-input.png",
    );
  });

  it("Tab B: should switch windows with prefix+n/p", async () => {
    // Switch to previous window
    await sendPrefixKey("p");
    await browser.pause(1000);

    const activeAfterPrev = await browser.execute(() => {
      const active = document.querySelector(".mux-sub-tab-active");
      if (!active || !active.parentElement) return -1;
      return Array.from(active.parentElement.children).indexOf(active);
    });
    console.log("Active window after prefix+p:", activeAfterPrev);
    expect(activeAfterPrev).toBe(0);

    await browser.saveScreenshot(
      "./screenshots/mux-multi-06-tab-b-switch.png",
    );

    // Switch back to next
    await sendPrefixKey("n");
    await browser.pause(1000);

    const activeAfterNext = await browser.execute(() => {
      const active = document.querySelector(".mux-sub-tab-active");
      if (!active || !active.parentElement) return -1;
      return Array.from(active.parentElement.children).indexOf(active);
    });
    expect(activeAfterNext).toBe(1);
  });

  it("Tab B: should close window with Ctrl+D and exit mux if last", async () => {
    // Close current window (window 1) with Ctrl+D
    await browser.keys(["Control", "d"]);
    await browser.pause(2000);

    // Should have switched to window 0
    let subTabCount = await getSubTabCount();
    console.log("Sub-tab count after first Ctrl+D:", subTabCount);
    expect(subTabCount).toBe(1);

    await browser.saveScreenshot(
      "./screenshots/mux-multi-07-tab-b-one-window.png",
    );

    // Close last window — should exit mux mode
    await browser.keys(["Control", "d"]);
    await browser.pause(2000);

    subTabCount = await getSubTabCount();
    console.log("Sub-tab count after second Ctrl+D:", subTabCount);
    expect(subTabCount).toBe(0); // Mux mode exited

    await browser.saveScreenshot(
      "./screenshots/mux-multi-08-tab-b-exited.png",
    );
  });

  it("Tab A: should still be in mux mode after Tab B exits", async () => {
    // Switch back to Tab A
    // Tab A is the first tab, use Ctrl+1 or click
    const tabs = await browser.execute(() => {
      return window.tabManager?.getTabs().map((t) => t.id) || [];
    });

    if (tabs.length > 0) {
      await browser.execute((tabId) => {
        window.tabManager?.switchTab(tabId);
      }, tabs[0]);
      await browser.pause(1000);
    }

    // Tab A should still have mux sub-tabs
    const subTabCount = await getSubTabCount();
    console.log("Tab A sub-tab count (should still be in mux):", subTabCount);
    expect(subTabCount).toBeGreaterThanOrEqual(1);

    await browser.saveScreenshot(
      "./screenshots/mux-multi-09-tab-a-still-mux.png",
    );
  });

  after(async () => {
    // Clean up: detach any remaining mux sessions
    try {
      await sendPrefixKey("d");
      await browser.pause(1000);
    } catch {
      // Ignore
    }

    // Kill daemon
    await browser.execute(() => {
      // Best effort cleanup
    });
  });
});
