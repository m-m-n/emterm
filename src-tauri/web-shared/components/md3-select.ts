/**
 * MD3 Custom Select (Exposed Dropdown Menu)
 *
 * Replaces native <select> elements with an MD3-styled custom dropdown.
 * Supports keyboard navigation, ARIA attributes, and scroll containment.
 */

export interface Md3SelectOption {
  value: string;
  label: string;
}

export interface Md3SelectConfig {
  id?: string;
  options: Md3SelectOption[];
  value: string;
  className?: string;
  ariaDescribedBy?: string;
  onChange: (value: string) => void;
}

/**
 * Creates an MD3 custom select element.
 * Returns the root element. Call `updateOptions()` to change available options,
 * or `setValue()` to programmatically change the selected value.
 */
export function createMd3Select(config: Md3SelectConfig): {
  element: HTMLElement;
  setValue: (value: string) => void;
  updateOptions: (options: Md3SelectOption[], value?: string) => void;
  getValue: () => string;
} {
  let currentValue = config.value;
  let currentOptions = [...config.options];

  // Root container
  const root = document.createElement("div");
  root.className = `md3-select ${config.className ?? ""}`.trim();
  if (config.id) root.id = config.id;

  // Trigger button
  const trigger = document.createElement("button");
  trigger.type = "button";
  trigger.className = "md3-select-trigger";
  trigger.setAttribute("role", "combobox");
  trigger.setAttribute("aria-haspopup", "listbox");
  trigger.setAttribute("aria-expanded", "false");
  if (config.ariaDescribedBy) {
    trigger.setAttribute("aria-describedby", config.ariaDescribedBy);
  }

  const labelSpan = document.createElement("span");
  labelSpan.className = "md3-select-label";
  trigger.appendChild(labelSpan);

  const chevron = document.createElement("span");
  chevron.className = "md3-select-chevron";
  chevron.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg>`;
  trigger.appendChild(chevron);

  root.appendChild(trigger);

  // Dropdown menu
  const menu = document.createElement("div");
  menu.className = "md3-select-menu";
  menu.setAttribute("role", "listbox");
  menu.tabIndex = -1;
  root.appendChild(menu);

  let isOpen = false;
  let activeIndex = -1;
  let menuItems: HTMLElement[] = [];

  const getSelectedLabel = (): string => {
    const opt = currentOptions.find((o) => o.value === currentValue);
    return opt?.label ?? "";
  };

  const updateLabel = () => {
    labelSpan.textContent = getSelectedLabel();
  };

  const buildMenuItems = () => {
    menu.innerHTML = "";
    menuItems = [];

    for (let i = 0; i < currentOptions.length; i++) {
      const opt = currentOptions[i]!;
      const item = document.createElement("div");
      item.className = "md3-select-item";
      item.setAttribute("role", "option");
      item.setAttribute("aria-selected", opt.value === currentValue ? "true" : "false");
      item.dataset.value = opt.value;
      item.dataset.index = String(i);
      item.textContent = opt.label;

      if (opt.value === currentValue) {
        item.classList.add("selected");
      }

      item.addEventListener("click", (e) => {
        e.stopPropagation();
        selectOption(i);
      });

      item.addEventListener("mouseenter", () => {
        updateActive(i);
      });

      menu.appendChild(item);
      menuItems.push(item);
    }
  };

  const updateActive = (newIndex: number) => {
    if (activeIndex >= 0 && activeIndex < menuItems.length) {
      menuItems[activeIndex]!.classList.remove("active");
    }
    activeIndex = newIndex;
    if (activeIndex >= 0 && activeIndex < menuItems.length) {
      menuItems[activeIndex]!.classList.add("active");
      menuItems[activeIndex]!.scrollIntoView({ block: "nearest" });
    }
  };

  const selectOption = (index: number) => {
    if (index < 0 || index >= currentOptions.length) return;
    const opt = currentOptions[index]!;
    currentValue = opt.value;
    updateLabel();
    closeMenu();
    config.onChange(currentValue);

    // Update selected state
    for (const item of menuItems) {
      const isSelected = item.dataset.value === currentValue;
      item.classList.toggle("selected", isSelected);
      item.setAttribute("aria-selected", isSelected ? "true" : "false");
    }
  };

  const openMenu = () => {
    if (isOpen) return;
    isOpen = true;

    buildMenuItems();
    root.classList.add("open");
    trigger.setAttribute("aria-expanded", "true");

    // Position menu
    const triggerRect = trigger.getBoundingClientRect();
    const vh = window.innerHeight;
    const spaceBelow = vh - triggerRect.bottom;
    const spaceAbove = triggerRect.top;

    // Show hidden for measurement
    menu.style.visibility = "hidden";
    menu.style.display = "block";
    menu.style.width = `${triggerRect.width}px`;
    const menuHeight = menu.scrollHeight;

    if (spaceBelow >= menuHeight || spaceBelow >= spaceAbove) {
      // Show below
      menu.style.top = `${triggerRect.height + 4}px`;
      menu.style.bottom = "";
      menu.style.maxHeight = `${Math.min(spaceBelow - 8, 320)}px`;
    } else {
      // Show above
      menu.style.top = "";
      menu.style.bottom = `${triggerRect.height + 4}px`;
      menu.style.maxHeight = `${Math.min(spaceAbove - 8, 320)}px`;
    }
    menu.style.visibility = "";

    // Set active to current selection
    const selectedIndex = currentOptions.findIndex((o) => o.value === currentValue);
    activeIndex = -1;
    if (selectedIndex >= 0) {
      updateActive(selectedIndex);
    }

    document.addEventListener("mousedown", handleOutsideClick, { capture: true });
    document.addEventListener("keydown", handleKeydown, { capture: true });
    window.addEventListener("blur", closeMenu);
  };

  const closeMenu = () => {
    if (!isOpen) return;
    isOpen = false;

    root.classList.remove("open");
    trigger.setAttribute("aria-expanded", "false");
    menu.style.display = "";

    document.removeEventListener("mousedown", handleOutsideClick, { capture: true });
    document.removeEventListener("keydown", handleKeydown, { capture: true });
    window.removeEventListener("blur", closeMenu);

    trigger.focus();
  };

  const handleOutsideClick = (e: MouseEvent) => {
    if (!root.contains(e.target as Node)) {
      closeMenu();
    }
  };

  const handleKeydown = (e: KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        e.stopPropagation();
        if (menuItems.length > 0) {
          updateActive(activeIndex < menuItems.length - 1 ? activeIndex + 1 : 0);
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        e.stopPropagation();
        if (menuItems.length > 0) {
          updateActive(activeIndex > 0 ? activeIndex - 1 : menuItems.length - 1);
        }
        break;
      case "Home":
        e.preventDefault();
        e.stopPropagation();
        if (menuItems.length > 0) updateActive(0);
        break;
      case "End":
        e.preventDefault();
        e.stopPropagation();
        if (menuItems.length > 0) updateActive(menuItems.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        e.stopPropagation();
        if (activeIndex >= 0) {
          selectOption(activeIndex);
        }
        break;
      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        closeMenu();
        break;
      case "Tab":
        closeMenu();
        break;
    }
  };

  // Trigger click handler
  trigger.addEventListener("click", (e) => {
    e.stopPropagation();
    if (isOpen) {
      closeMenu();
    } else {
      openMenu();
    }
  });

  // Trigger keyboard handler (when menu is closed)
  trigger.addEventListener("keydown", (e) => {
    if (isOpen) return; // Handled by document listener
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === " " || e.key === "Enter") {
      e.preventDefault();
      openMenu();
    }
  });

  // Initial state
  updateLabel();

  return {
    element: root,
    setValue: (value: string) => {
      currentValue = value;
      updateLabel();
    },
    updateOptions: (options: Md3SelectOption[], value?: string) => {
      currentOptions = [...options];
      if (value !== undefined) {
        currentValue = value;
      }
      updateLabel();
      if (isOpen) {
        buildMenuItems();
      }
    },
    getValue: () => currentValue,
  };
}
