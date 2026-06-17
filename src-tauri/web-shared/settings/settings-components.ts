/**
 * Settings UI Components
 *
 * Reusable UI control renderers for settings panel.
 * Each renderer creates DOM elements and attaches event listeners.
 */

import { createMd3Select } from "../components/md3-select";

// ============================================================
// Option Types
// ============================================================

export interface NumberInputOptions {
  key: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  hint: string;
  description?: string;
  onInput: (value: number) => void;
  onSave: (value: number) => void;
}

export interface TextInputOptions {
  key: string;
  label: string;
  value: string;
  placeholder: string;
  hint: string;
  description?: string;
  onSave: (value: string) => void;
}

export interface SelectOptions {
  key: string;
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  description?: string;
  onSave: (value: string) => void;
}

export interface ToggleOptions {
  key: string;
  label: string;
  value: boolean;
  description?: string;
  onSave: (value: boolean) => void;
}

export interface SliderOptions {
  key: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  hint: string;
  description?: string;
  onInput: (value: number) => void;
  onSave: (value: number) => void;
}

/** Callback to register event listeners for cleanup */
export type AddListenerFn = (
  element: EventTarget,
  type: string,
  handler: EventListener,
  capture?: boolean,
) => void;

// ============================================================
// Renderers
// ============================================================

export function renderSubsectionHeader(panel: HTMLElement, text: string): void {
  const h3 = document.createElement("h3");
  h3.className = "settings-subsection-header";
  h3.textContent = text;
  panel.appendChild(h3);
}

export function renderNumberInput(
  panel: HTMLElement,
  opts: NumberInputOptions,
  addListener: AddListenerFn,
): void {
  const row = document.createElement("div");
  row.className = "settings-row";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;
  row.appendChild(label);

  if (opts.description) {
    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    row.appendChild(desc);
  }

  const inputGroup = document.createElement("div");
  inputGroup.className = "settings-input-group";

  const input = document.createElement("input");
  input.type = "number";
  input.id = `settings-${opts.key}`;
  input.className = "settings-number-input";
  input.min = String(opts.min);
  input.max = String(opts.max);
  input.step = String(opts.step);
  input.value = String(opts.value);
  if (opts.description) {
    input.setAttribute("aria-describedby", `settings-${opts.key}-desc`);
  }
  inputGroup.appendChild(input);

  if (opts.unit) {
    const unit = document.createElement("span");
    unit.className = "settings-unit";
    unit.textContent = opts.unit;
    inputGroup.appendChild(unit);
  }

  row.appendChild(inputGroup);

  const hint = document.createElement("span");
  hint.className = "settings-hint";
  hint.textContent = opts.hint;
  row.appendChild(hint);

  panel.appendChild(row);

  // Event listeners
  let lastSavedValue = opts.value;

  const inputHandler = () => {
    const v = Number(input.value);
    if (v >= opts.min && v <= opts.max) {
      opts.onInput(v);
    }
  };
  addListener(input, "input", inputHandler);

  const saveHandler = () => {
    let v = Number(input.value);
    if (isNaN(v)) {
      v = lastSavedValue;
    }
    v = Math.max(opts.min, Math.min(opts.max, v));
    input.value = String(v);
    if (v !== lastSavedValue) {
      lastSavedValue = v;
      opts.onSave(v);
    }
  };
  addListener(input, "blur", saveHandler);
  addListener(input, "keydown", (e: Event) => {
    if ((e as KeyboardEvent).key === "Enter") saveHandler();
  });
}

export function renderTextInput(
  panel: HTMLElement,
  opts: TextInputOptions,
  addListener: AddListenerFn,
): void {
  const row = document.createElement("div");
  row.className = "settings-row";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;
  row.appendChild(label);

  if (opts.description) {
    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    row.appendChild(desc);
  }

  const input = document.createElement("input");
  input.type = "text";
  input.id = `settings-${opts.key}`;
  input.className = "settings-text-input";
  input.value = opts.value;
  input.placeholder = opts.placeholder;
  if (opts.description) {
    input.setAttribute("aria-describedby", `settings-${opts.key}-desc`);
  }
  row.appendChild(input);

  const hint = document.createElement("span");
  hint.className = "settings-hint";
  hint.textContent = opts.hint;
  row.appendChild(hint);

  panel.appendChild(row);

  // Save on blur/Enter
  let lastSaved = opts.value;
  const saveHandler = () => {
    const v = input.value;
    if (v !== lastSaved) {
      lastSaved = v;
      opts.onSave(v);
    }
  };
  addListener(input, "blur", saveHandler);
  addListener(input, "keydown", (e: Event) => {
    if ((e as KeyboardEvent).key === "Enter") saveHandler();
  });
}

export function renderSelect(
  panel: HTMLElement,
  opts: SelectOptions,
  _addListener: AddListenerFn,
): void {
  const row = document.createElement("div");
  row.className = "settings-row";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;
  row.appendChild(label);

  if (opts.description) {
    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    row.appendChild(desc);
  }

  const select = createMd3Select({
    id: `settings-${opts.key}`,
    options: opts.options,
    value: opts.value,
    ariaDescribedBy: opts.description ? `settings-${opts.key}-desc` : undefined,
    onChange: (value) => opts.onSave(value),
  });
  row.appendChild(select.element);

  panel.appendChild(row);
}

export function renderToggle(
  panel: HTMLElement,
  opts: ToggleOptions,
  addListener: AddListenerFn,
): void {
  const row = document.createElement("div");
  row.className = "settings-row settings-row-toggle";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;

  if (opts.description) {
    const wrapper = document.createElement("div");
    wrapper.className = "settings-toggle-label-group";
    wrapper.appendChild(label);

    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    wrapper.appendChild(desc);

    row.appendChild(wrapper);
  } else {
    row.appendChild(label);
  }

  const button = document.createElement("button");
  button.id = `settings-${opts.key}`;
  button.className = "settings-toggle";
  button.setAttribute("role", "switch");
  button.setAttribute("aria-checked", String(opts.value));
  if (opts.description) {
    button.setAttribute("aria-describedby", `settings-${opts.key}-desc`);
  }
  if (opts.value) button.classList.add("on");

  const track = document.createElement("span");
  track.className = "settings-toggle-track";
  const thumb = document.createElement("span");
  thumb.className = "settings-toggle-thumb";
  track.appendChild(thumb);
  button.appendChild(track);

  row.appendChild(button);
  panel.appendChild(row);

  // Toggle on click
  let currentValue = opts.value;
  addListener(button, "click", () => {
    currentValue = !currentValue;
    button.setAttribute("aria-checked", String(currentValue));
    button.classList.toggle("on", currentValue);
    opts.onSave(currentValue);
  });
}

export function renderSlider(
  panel: HTMLElement,
  opts: SliderOptions,
  addListener: AddListenerFn,
): void {
  const row = document.createElement("div");
  row.className = "settings-row";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;
  row.appendChild(label);

  if (opts.description) {
    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    row.appendChild(desc);
  }

  const sliderGroup = document.createElement("div");
  sliderGroup.className = "settings-slider-group";

  const input = document.createElement("input");
  input.type = "range";
  input.id = `settings-${opts.key}`;
  input.className = "settings-slider";
  input.min = String(opts.min);
  input.max = String(opts.max);
  input.step = String(opts.step);
  input.value = String(opts.value);
  if (opts.description) {
    input.setAttribute("aria-describedby", `settings-${opts.key}-desc`);
  }
  sliderGroup.appendChild(input);

  const valueDisplay = document.createElement("span");
  valueDisplay.className = "settings-slider-value";
  valueDisplay.textContent = String(opts.value);
  sliderGroup.appendChild(valueDisplay);

  row.appendChild(sliderGroup);

  const hint = document.createElement("span");
  hint.className = "settings-hint";
  hint.textContent = opts.hint;
  row.appendChild(hint);

  panel.appendChild(row);

  // Event listeners
  addListener(input, "input", () => {
    const v = Number(input.value);
    valueDisplay.textContent = String(v);
    opts.onInput(v);
  });
  addListener(input, "change", () => {
    const v = Number(input.value);
    opts.onSave(v);
  });
}
