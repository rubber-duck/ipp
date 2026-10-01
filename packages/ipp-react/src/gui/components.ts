import { createElement } from "react";
import {
  componentContract,
  type ComponentFields,
  type ComponentProps,
} from "../components.js";

export type StyleProps = ComponentProps & ComponentFields<"CanvasStyle">;

export function Style(props: StyleProps) {
  return createElement(componentContract.CanvasStyle.host, props);
}

export type LayoutProps = ComponentProps & ComponentFields<"GuiLayout">;

export function Layout(props: LayoutProps) {
  return createElement(componentContract.GuiLayout.host, props);
}

export type TextProps = ComponentProps & ComponentFields<"CanvasText">;

export function Text(props: TextProps) {
  return createElement(componentContract.CanvasText.host, props);
}

export type GlyphRunProps = ComponentProps & ComponentFields<"CanvasGlyphRun">;

export function GlyphRun(props: GlyphRunProps) {
  return createElement(componentContract.CanvasGlyphRun.host, props);
}

export type DrawingProps = ComponentProps & ComponentFields<"CanvasDrawing">;

export function Drawing(props: DrawingProps) {
  return createElement(componentContract.CanvasDrawing.host, props);
}

export type ImageProps = ComponentProps & ComponentFields<"CanvasBitmap">;

export function Image(props: ImageProps) {
  return createElement(componentContract.CanvasBitmap.host, props);
}

export type BoxProps = ComponentProps & ComponentFields<"CanvasBox">;

export function Box(props: BoxProps) {
  return createElement(componentContract.CanvasBox.host, props);
}

export type FontProps = ComponentProps & ComponentFields<"GuiFont">;

export function Font(props: FontProps) {
  return createElement(componentContract.GuiFont.host, props);
}

export type BehaviorProps = ComponentProps & ComponentFields<"GuiBehavior">;

export function Behavior(props: BehaviorProps) {
  return createElement(componentContract.GuiBehavior.host, props);
}
