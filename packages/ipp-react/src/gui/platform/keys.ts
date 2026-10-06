import type { GuiPhysicalKey } from "@ipp/client";

/**
 * The GUI key of a DOM `KeyboardEvent.key`, or null for keys the GUI leaves to
 * the browser. F10 is a GUI key only with Shift, as a context request.
 */
export function keyboardKeyToGuiKey(
  key: string,
  shift = false,
): GuiPhysicalKey | "backspace" | "delete" | null {
  if (key === "Backspace") return "backspace";
  if (key === "Delete") return "delete";
  if (key === "F10" && !shift) return null;
  return key === "Tab" && shift ? "backTab" : (keys[key] ?? null);
}

const keys: Readonly<Record<string, GuiPhysicalKey>> = {
  Tab: "tab",
  Enter: "enter",
  " ": "space",
  Escape: "escape",
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  Home: "home",
  End: "end",
  ContextMenu: "contextMenu",
  F10: "f10",
};
