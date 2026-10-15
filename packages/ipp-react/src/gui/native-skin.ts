import { createElement, Fragment, useContext } from "react";
import { AttachmentContext } from "../composition/attached-world.js";
import { componentContract } from "../components.js";
import type { GuiKitControlName } from "../gui-kit/kit.js";
import { GuiSkinContext } from "./skin-context.js";

export interface NativeControlSkinProps {
  /** Inherit the native role by default, choose another role, or disable inheritance. */
  readonly skin?: GuiKitControlName | false;
}

/** Author one shared theme reference; explicit Skin declarations take precedence. */
export function useNativeControl(
  host: string,
  role: GuiKitControlName,
  {
    skin,
    ...props
  }: NativeControlSkinProps & Readonly<Record<string, unknown>>,
) {
  const context = useContext(GuiSkinContext);
  const container = useContext(AttachmentContext);
  const theme =
    skin !== false && context?.container === container
      ? context?.appearance.controls?.[skin ?? role]
      : undefined;
  return createElement(
    Fragment,
    null,
    createElement(host, { ...props, key: "control" }),
    skin !== false &&
      context?.container === container &&
      context &&
      createElement(componentContract.GuiFont.host, {
        key: "font",
        source: context.font,
        font_size: context.fontSize,
        fontFallback: true,
      }),
    theme &&
      createElement(componentContract.GuiSkin.host, {
        key: "skin",
        theme,
        parts: context!.emptyParts,
        skinFallback: true,
      }),
  );
}
