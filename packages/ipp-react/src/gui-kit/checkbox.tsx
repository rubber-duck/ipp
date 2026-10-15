import { useEntityId } from "../scope/identity.js";
import type { ReactNode } from "react";
import { PresentationGate, Children, Entity } from "../components.js";
import {
  Checkbox as NativeCheckbox,
  type CheckboxProps as NativeCheckboxProps,
} from "../gui/controls.js";
import { Behavior, Font, Layout, Style } from "../gui/components.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_LEAF, layoutFields, type GuiKitLayoutProps } from "./layout.js";
import { textWidth } from "./text.js";
import { useCheckedValue } from "./values/checked-value.js";

export interface CheckboxProps
  extends Omit<
      NativeCheckboxProps,
      "checked" | "children" | "fields" | "onToggle"
    >,
    GuiKitLayoutProps {
  readonly id?: string;
  readonly checked?: boolean;
  readonly defaultChecked?: boolean;
  readonly onCheckedChange?: (checked: boolean) => void;
  readonly disabled?: boolean;
  readonly layer?: number;
  readonly semanticLabel?: string;
  readonly children?: ReactNode;
}

function CheckedField({
  role,
  id: providedId,
  checked,
  defaultChecked = false,
  onCheckedChange,
  disabled = false,
  layer = 0,
  semanticLabel,
  children,
  label,
  ref,
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
}: CheckboxProps & { readonly role: "checkbox" | "switch" }) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const value = useCheckedValue(checked, defaultChecked, onCheckedChange, ref);
  const text =
    label ??
    (typeof children === "string" || typeof children === "number"
      ? String(children)
      : "");
  const custom =
    children !== undefined &&
    typeof children !== "string" &&
    typeof children !== "number";
  const [indicatorWidth, indicatorHeight] =
    kit.controlSize(role) ??
    (role === "switch"
      ? [
          kit.unit(kit.tokens.switchWidth ?? 40),
          kit.unit(kit.tokens.switchHeight ?? 22),
        ]
      : [
          kit.unit(kit.tokens.controlHeight),
          kit.unit(kit.tokens.controlHeight),
        ]);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_LEAF}
        {...layoutFields(
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
            width:
              textWidth(text, kit.fontSize) +
              indicatorWidth +
              (text ? kit.spacing.label : 0),
            height: Math.max(indicatorHeight, text ? kit.fontSize : 0),
          },
        )}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Behavior enabled={!disabled} semantic_label={semanticLabel ?? text} />
      <NativeCheckbox
        {...control}
        label={text}
        checked={value.initial}
        onToggle={value.changed}
        ref={value.ref}
      />
      {custom && <Children>{children}</Children>}
    </Entity>
  );
}

export function Checkbox(props: CheckboxProps) {
  return <CheckedField {...props} role="checkbox" />;
}

/** A checked field with the skin's switch role; the native control identity is unchanged. */
export type SwitchProps = CheckboxProps;

export function Switch(props: SwitchProps) {
  return (
    <CheckedField {...props} role="switch" skin={props.skin ?? "switch"} />
  );
}
