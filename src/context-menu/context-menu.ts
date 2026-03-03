/**
 * HTML-based context menu component.
 *
 * Replaces Tauri native Menu.popup() which does not position correctly
 * on Linux (GTK/muda). Uses event.clientX/clientY for precise placement.
 */

export interface ContextMenuItem {
  type: "item" | "separator";
  label?: string;
  enabled?: boolean;
  action?: () => void;
}

export interface ContextMenuOptions {
  event: MouseEvent;
  items: ContextMenuItem[];
  onClose?: () => void;
}

let activeCleanup: (() => void) | null = null;

/**
 * Show a context menu at the mouse event position.
 * Only one menu can be open at a time (singleton).
 * Returns a cleanup function to dismiss the menu.
 */
export function showContextMenu(options: ContextMenuOptions): () => void {
  // Close any existing menu
  if (activeCleanup) {
    activeCleanup();
  }

  const { event, items, onClose } = options;

  const menu = document.createElement("div");
  menu.className = "context-menu";
  menu.setAttribute("role", "menu");
  menu.tabIndex = -1;

  // Build items
  const actionableItems: HTMLElement[] = [];

  for (const item of items) {
    if (item.type === "separator") {
      const sep = document.createElement("div");
      sep.className = "context-menu-separator";
      sep.setAttribute("role", "separator");
      menu.appendChild(sep);
      continue;
    }

    const el = document.createElement("div");
    el.className = "context-menu-item";
    el.setAttribute("role", "menuitem");
    el.textContent = item.label ?? "";

    const enabled = item.enabled !== false;
    if (!enabled) {
      el.setAttribute("aria-disabled", "true");
    }

    el.addEventListener("click", (e) => {
      e.stopPropagation();
      if (!enabled) return;
      dismiss();
      item.action?.();
    });

    menu.appendChild(el);
    if (enabled) {
      actionableItems.push(el);
    }
  }

  // Add to DOM hidden for measurement
  menu.style.visibility = "hidden";
  document.body.appendChild(menu);

  // Position with overflow handling
  const rect = menu.getBoundingClientRect();
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let left = event.clientX + 4;
  let top = event.clientY + 4;

  if (left + rect.width > vw) {
    left = event.clientX - rect.width - 4;
  }
  if (top + rect.height > vh) {
    top = event.clientY - rect.height - 4;
  }

  left = Math.max(0, left);
  top = Math.max(0, top);

  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
  menu.style.visibility = "";

  // Focus management
  let activeIndex = -1;

  const updateActive = (newIndex: number) => {
    if (activeIndex >= 0 && activeIndex < actionableItems.length) {
      actionableItems[activeIndex]!.classList.remove("active");
    }
    activeIndex = newIndex;
    if (activeIndex >= 0 && activeIndex < actionableItems.length) {
      actionableItems[activeIndex]!.classList.add("active");
    }
  };

  const handleKeydown = (e: KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        if (actionableItems.length > 0) {
          updateActive(activeIndex < actionableItems.length - 1 ? activeIndex + 1 : 0);
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        if (actionableItems.length > 0) {
          updateActive(activeIndex > 0 ? activeIndex - 1 : actionableItems.length - 1);
        }
        break;
      case "Enter":
        e.preventDefault();
        if (activeIndex >= 0 && activeIndex < actionableItems.length) {
          dismiss();
          const selectedItem = items.filter(
            (i) => i.type === "item" && i.enabled !== false,
          )[activeIndex];
          selectedItem?.action?.();
        }
        break;
      case "Escape":
        e.preventDefault();
        dismiss();
        break;
    }
  };

  const handleMousedownOutside = (e: MouseEvent) => {
    if (!menu.contains(e.target as Node)) {
      dismiss();
    }
  };

  const handleBlur = () => {
    dismiss();
  };

  // Register listeners
  document.addEventListener("keydown", handleKeydown, { capture: true });
  document.addEventListener("mousedown", handleMousedownOutside, { capture: true });
  window.addEventListener("blur", handleBlur);

  menu.focus();

  let dismissed = false;

  const dismiss = () => {
    if (dismissed) return;
    dismissed = true;

    document.removeEventListener("keydown", handleKeydown, { capture: true });
    document.removeEventListener("mousedown", handleMousedownOutside, { capture: true });
    window.removeEventListener("blur", handleBlur);

    menu.remove();

    if (activeCleanup === dismiss) {
      activeCleanup = null;
    }

    onClose?.();
  };

  activeCleanup = dismiss;
  return dismiss;
}
