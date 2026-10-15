import { AttachmentContext } from "../composition/attached-world.js";
import { GuiSkinContext } from "./skin-context.js";
import { createElement, useContext } from "react";
import {
  componentContract,
  type ComponentFields,
  type ComponentProps,
} from "../components.js";

export type ThemeProps = ComponentProps & ComponentFields<"GuiTheme">;

export function Theme(props: ThemeProps) {
  return createElement(componentContract.GuiTheme.host, props);
}

export type ThemeMotionProps = ComponentProps &
  ComponentFields<"GuiThemeMotion">;

/**
 * Transition timing rows beside a `Theme` on the same entity, in a World that
 * selects animation. Missing timing is immediate.
 */
export function ThemeMotion(props: ThemeMotionProps) {
  return createElement(componentContract.GuiThemeMotion.host, props);
}

export type SkinProps = ComponentProps & ComponentFields<"GuiSkin">;

export function Skin(props: SkinProps) {
  const context = useContext(GuiSkinContext);
  const container = useContext(AttachmentContext);
  const inherited = context?.container === container ? context : undefined;
  // A whole explicit skin replaces automatic appearance, including when it is
  // added or removed on an existing control. Raw field writes remain explicit.
  return createElement(componentContract.GuiSkin.host, {
    ...props,
    ...(inherited && !props.fields
      ? { theme: props.theme ?? 0n, parts: props.parts ?? inherited.emptyParts }
      : {}),
  });
}

export type MetricsProps = ComponentProps & ComponentFields<"GuiMetrics">;

/** Shared skin dimensions on a theme, or absolute overrides on a control. */
export function Metrics(props: MetricsProps) {
  return createElement(componentContract.GuiMetrics.host, props);
}
