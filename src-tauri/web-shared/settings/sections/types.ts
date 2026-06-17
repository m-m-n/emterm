import type { AppSettings, FontCategory } from "../types";
import type { AddListenerFn } from "../settings-components";
import type { KeybindEditorContext } from "../keybind-editor";

export interface SectionContext {
  currentSettings: AppSettings;
  addContentListener: AddListenerFn;
  saveSetting: <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => void;
  showFontPicker: (
    category: FontCategory,
    currentValue: string,
    onSelect: (value: string) => void,
  ) => void;
  keybindCtx: KeybindEditorContext;
  reRender: () => void;
}
