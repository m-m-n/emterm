/**
 * E2E: Mux move-window scenarios
 *
 * Exercises the prefix+m workflow end-to-end against a real daemon:
 *   - Tab badge `[N]` rendering (single and multiple windows)
 *   - Reorder on confirm
 *   - Esc / non-numeric / out-of-range / same-position cancel paths
 */

describe("Mux Move Window", () => {
  /** Type characters one at a time. */
  async function typeSlowly(text, delay = 60) {
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

  /** Return the ordered window names rendered in the active mux tab group. */
  async function getWindowOrder() {
    return await browser.execute(() => {
      const group = document.querySelector(".mux-tab-group");
      if (!group) return [];
      return Array.from(group.querySelectorAll(".mux-window-tab")).map((w) => {
        const t = w.querySelector(".tab-title");
        return t ? (t.textContent || "") : "";
      });
    });
  }

  /** Return the number-badge texts (e.g., ["[1]", "[2]", "[3]"]). */
  async function getWindowBadges() {
    return await browser.execute(() => {
      const group = document.querySelector(".mux-tab-group");
      if (!group) return [];
      return Array.from(
        group.querySelectorAll(".mux-window-tab .mux-window-number"),
      ).map((n) => n.textContent || "");
    });
  }

  /** Find the active window index (0-based) within the tab group. */
  async function getActiveWindowIndex() {
    return await browser.execute(() => {
      const group = document.querySelector(".mux-tab-group");
      if (!group) return -1;
      const children = Array.from(group.querySelectorAll(".mux-window-tab"));
      const idx = children.findIndex((c) =>
        c.classList.contains("mux-window-active"),
      );
      return idx;
    });
  }

  /** True if the move-window dialog overlay is present in the DOM. */
  async function isDialogOpen() {
    return await browser.execute(() => {
      return document.querySelector(".sftp-dialog-overlay") !== null;
    });
  }

  /**
   * Programmatically drive the open dialog: set the input's value, then
   * dispatch a keydown on the overlay with the given key. This avoids any
   * focus / key-routing subtleties between the Tauri canvas and the
   * dialog input.
   */
  async function submitDialog(inputValue, key) {
    await browser.execute(
      (value, k) => {
        const input = document.querySelector(".sftp-dialog-input");
        const overlay = document.querySelector(".sftp-dialog-overlay");
        if (input) {
          input.value = value;
        }
        if (overlay) {
          overlay.dispatchEvent(
            new KeyboardEvent("keydown", { key: k, bubbles: true }),
          );
        }
      },
      inputValue,
      key,
    );
  }

  /** Wait for the dialog to disappear (or timeout), then a short settle. */
  async function waitForDialogClosed(timeoutMs = 3000) {
    try {
      await browser.waitUntil(async () => !(await isDialogOpen()), {
        timeout: timeoutMs,
        interval: 100,
      });
    } catch {
      // fall through; assertion in caller will surface the failure
    }
    await browser.pause(300);
  }

  async function waitForTerminal() {
    // The app renders into #tab-content-area at startup; canvas appears
    // once the terminal tab is initialized.
    const container = await $("#tab-content-area");
    await container.waitForExist({ timeout: 30000 });
    // Wait for the first canvas (terminal tab) to be attached.
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => !!document.querySelector("#tab-content-area canvas"),
        )),
      { timeout: 30000, timeoutMsg: "terminal canvas did not appear" },
    );
    // Give the shell prompt some time to settle.
    await browser.pause(5000);
  }

  before(async () => {
    await waitForTerminal();

    const canvas = await $("#tab-content-area canvas");
    await canvas.click();
    await browser.pause(500);

    // Clean slate: kill any stale daemon and socket.
    await typeSlowly("pkill -f 'emterm mux --daemon' 2>/dev/null; true");
    await browser.keys("Enter");
    await browser.pause(1000);
    await typeSlowly(
      "rm -f ${XDG_RUNTIME_DIR:-/tmp}/emterm/mux-default.sock 2>/dev/null; true",
    );
    await browser.keys("Enter");
    await browser.pause(500);

    await browser.saveScreenshot("./screenshots/mux-move-00-clean.png");
  });

  it("E2E-6: single mux window is rendered with [1] badge", async () => {
    await typeSlowly("emterm mux");
    await browser.keys("Enter");
    await browser.pause(5000);

    const badges = await getWindowBadges();
    console.log("Single-window badges:", badges);
    expect(badges).toEqual(["[1]"]);

    await browser.saveScreenshot("./screenshots/mux-move-01-single.png");
  });

  it("creates two additional windows to build [1][2][3]", async () => {
    await sendPrefixKey("c");
    await browser.pause(3000);
    await sendPrefixKey("c");
    await browser.pause(3000);

    const badges = await getWindowBadges();
    console.log("Three-window badges:", badges);
    expect(badges).toEqual(["[1]", "[2]", "[3]"]);

    await browser.saveScreenshot("./screenshots/mux-move-02-three.png");
  });

  it("E2E-1: prefix+m -> 1 -> Enter moves active to position 1", async () => {
    const orderBefore = await getWindowOrder();
    const activeBefore = await getActiveWindowIndex();
    console.log("Before move:", orderBefore, "active=", activeBefore);
    // We just created two additional windows, so the active is the last one (index 2).
    // We'll move it to position 1 (0-based index 0).

    await sendPrefixKey("m");
    await browser.pause(500);
    expect(await isDialogOpen()).toBe(true);
    await submitDialog("1", "Enter");
    await waitForDialogClosed();
    expect(await isDialogOpen()).toBe(false);

    const orderAfter = await getWindowOrder();
    const activeAfter = await getActiveWindowIndex();
    console.log("After move:", orderAfter, "active=", activeAfter);

    // The moved window (formerly at activeBefore) is now at index 0.
    expect(orderAfter[0]).toBe(orderBefore[activeBefore]);
    // And it is still the active window.
    expect(activeAfter).toBe(0);

    // Badges are still sequential 1..N.
    const badges = await getWindowBadges();
    expect(badges).toEqual(["[1]", "[2]", "[3]"]);

    await browser.saveScreenshot("./screenshots/mux-move-03-e2e1.png");
  });

  it("E2E-2: prefix+m -> Esc leaves order unchanged", async () => {
    const orderBefore = await getWindowOrder();
    const activeBefore = await getActiveWindowIndex();

    await sendPrefixKey("m");
    await browser.pause(500);
    expect(await isDialogOpen()).toBe(true);
    await submitDialog("", "Escape");
    await waitForDialogClosed();
    expect(await isDialogOpen()).toBe(false);

    expect(await getWindowOrder()).toEqual(orderBefore);
    expect(await getActiveWindowIndex()).toBe(activeBefore);

    await browser.saveScreenshot("./screenshots/mux-move-04-e2e2-esc.png");
  });

  it("E2E-3: prefix+m -> 999 -> Enter cancels (out of range)", async () => {
    const orderBefore = await getWindowOrder();

    await sendPrefixKey("m");
    await browser.pause(500);
    expect(await isDialogOpen()).toBe(true);
    await submitDialog("999", "Enter");
    await waitForDialogClosed();
    expect(await isDialogOpen()).toBe(false);

    expect(await getWindowOrder()).toEqual(orderBefore);

    await browser.saveScreenshot("./screenshots/mux-move-05-e2e3-999.png");
  });

  it("E2E-4: prefix+m -> abc -> Enter cancels (non-numeric)", async () => {
    const orderBefore = await getWindowOrder();

    await sendPrefixKey("m");
    await browser.pause(500);
    expect(await isDialogOpen()).toBe(true);
    await submitDialog("abc", "Enter");
    await waitForDialogClosed();
    expect(await isDialogOpen()).toBe(false);

    expect(await getWindowOrder()).toEqual(orderBefore);

    await browser.saveScreenshot("./screenshots/mux-move-06-e2e4-abc.png");
  });

  it("E2E-5: prefix+m -> same position cancels (no-op)", async () => {
    const orderBefore = await getWindowOrder();
    const active = await getActiveWindowIndex();
    const sameNumber = String(active + 1);

    await sendPrefixKey("m");
    await browser.pause(500);
    expect(await isDialogOpen()).toBe(true);
    await submitDialog(sameNumber, "Enter");
    await waitForDialogClosed();
    expect(await isDialogOpen()).toBe(false);

    expect(await getWindowOrder()).toEqual(orderBefore);

    await browser.saveScreenshot("./screenshots/mux-move-07-e2e5-same.png");
  });

  after(async () => {
    // Best-effort detach so we leave the daemon in a clean state.
    try {
      await sendPrefixKey("d");
      await browser.pause(1000);
    } catch {
      // ignore
    }
  });
});
