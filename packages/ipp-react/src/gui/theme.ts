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

export type SkinProps = ComponentProps & ComponentFields<"GuiSkin">;

export function Skin(props: SkinProps) {
  return createElement(componentContract.GuiSkin.host, props);
}
