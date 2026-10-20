import { useEntityId } from "../scope/identity.js";
import type { ReactNode } from "react";
import { PresentationGate, Children, Entity } from "../components.js";
import {
  Button as NativeButton,
  type ButtonProps as NativeButtonProps,
} from "../gui/controls.js";
import { Behavior, Font, Layout, Style } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";
import {
  LAYOUT_LEAF,
  useLayoutFields,
  type GuiKitLayoutProps,
} from "./layout.js";
import { textWidth } from "./text.js";

/** A complete entity-owning button; native Button remains a component declaration. */
export interface ButtonProps
  extends Omit<NativeButtonProps, "children" | "fields">,
    GuiKitLayoutProps {
  readonly id?: string;
  readonly disabled?: boolean;
  readonly layer?: number;
  readonly semanticLabel?: string;
  readonly variant?: "primary" | "secondary" | "destructive";
  readonly children?: ReactNode;
}

export function Button({
  id: providedId,
  disabled = false,
  layer = 0,
  semanticLabel,
  variant = "primary",
  children,
  label,
  selected = false,
  layout,
  width,
  height,
  minWidth,
  minHeight,
  maxWidth,
  maxHeight,
  flex,
  fitWidth,
  fitHeight,
  alignX,
  alignY,
  padding,
  margin,
  clip,
  ...control
}: ButtonProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const text =
    label ??
    (typeof children === "string" || typeof children === "number"
      ? String(children)
      : "");
  const custom =
    children !== undefined &&
    typeof children !== "string" &&
    typeof children !== "number";
  const theme =
    variant === "primary"
      ? kit.controlTheme("button")
      : kit.theme(variant === "secondary" ? "secondary" : "amber");
  const fields = useLayoutFields(
    {
      layout,
      width,
      height,
      minWidth,
      minHeight,
      maxWidth,
      maxHeight,
      flex,
      fitWidth,
      fitHeight,
      alignX,
      alignY,
      padding,
      margin,
      clip,
    },
    {
      width: textWidth(text, kit.fontSize) + kit.unit(2 * kit.tokens.inset),
      height: kit.unit(kit.tokens.controlHeight),
    },
  );
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Layout kind={LAYOUT_LEAF} {...fields} />
      <Style layer={layer} />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Behavior enabled={!disabled} semantic_label={semanticLabel ?? text} />
      {theme && control.skin !== false && <Skin theme={theme} />}
      <NativeButton {...control} selected={selected} label={text} />
      {custom && <Children>{children}</Children>}
    </Entity>
  );
}
