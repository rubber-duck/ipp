import { createElement } from "react";
import { Skin } from "../gui/theme.js";
import { useGuiKit, type GuiKitControlName } from "./kit.js";

/** Select the enclosing client skin for a raw native control on this entity. */
export function ControlSkin({ role }: { readonly role: GuiKitControlName }) {
  const kit = useGuiKit();
  const theme = kit.controlTheme(role);
  if (!theme) throw new Error(`The selected skin has no ${role} theme`);
  return createElement(Skin, { theme });
}
