import { createElement } from "react";
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
 * selects animation; the default look of each control kind times whatever
 * they leave out.
 */
export function ThemeMotion(props: ThemeMotionProps) {
  return createElement(componentContract.GuiThemeMotion.host, props);
}

export type SkinProps = ComponentProps & ComponentFields<"GuiSkin">;

export function Skin(props: SkinProps) {
  return createElement(componentContract.GuiSkin.host, props);
}
