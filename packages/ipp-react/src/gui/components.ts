import { createElement, type ReactNode } from "react";
import type { DynamicPropertyInput } from "@ipp/client";
import type { AssetReference } from "../assets.js";
import {
  componentContract,
  type ComponentFields,
  type ComponentProps,
} from "../components.js";
import type { GuiVisibleChangeListener } from "./callbacks.js";

export type StyleProps = ComponentProps & ComponentFields<"CanvasStyle">;

export function Style(props: StyleProps) {
  return createElement(componentContract.CanvasStyle.host, props);
}

export type LayoutProps = ComponentProps & ComponentFields<"GuiLayout">;

export function Layout(props: LayoutProps) {
  return createElement(componentContract.GuiLayout.host, props);
}

export type OverlayProps = ComponentProps & ComponentFields<"GuiOverlay">;

/**
 * Lays its entity out of its parent's flow and places it against the parent's
 * box. Its `mode` decides what besides your writes of its `Behavior.visible`
 * opens and closes it: manual 0, light 1, modal 2 or hint 3.
 */
export function Overlay(props: OverlayProps) {
  return createElement(componentContract.GuiOverlay.host, props);
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

/** A paint shader definition with independently editable named inputs. */
export type PaintProps = ComponentProps &
  ComponentFields<"CanvasPaint"> & {
    children?: ReactNode;
    /** Unknown props are the paint's named inputs; fixed fields keep their meaning. */
    [name: string]: DynamicPropertyInput | ReactNode | AssetReference;
  };

/**
 * Fills its entity's own box, a `Box` or the Background of its skin or control,
 * through the custom paint `source` names, with the box's colour as the paint's
 * colour input. Other props are the paint's named float inputs, written,
 * animated and saved like custom-material properties.
 */
export function Paint(props: PaintProps) {
  return createElement(componentContract.CanvasPaint.host, props);
}

export type FontProps = ComponentProps & ComponentFields<"GuiFont">;

export function Font(props: FontProps) {
  return createElement(componentContract.GuiFont.host, props);
}

export type BehaviorProps = ComponentProps &
  ComponentFields<"GuiBehavior"> & {
    /**
     * The `visible` field, first as it is and then as anyone changes it: an
     * overlay's open state, which the runtime writes as its mode decides.
     */
    onVisibleChange?: GuiVisibleChangeListener;
  };

export function Behavior(props: BehaviorProps) {
  return createElement(componentContract.GuiBehavior.host, props);
}

/**
 * Makes the controls below its entity, down to any nested group, its items:
 * `axis` horizontal 0, vertical 1 (default) or both 2; `selection` none 0
 * (default), single 1 or follow 2, kept in the items' `selected` fields.
 */
export type GroupProps = ComponentProps & ComponentFields<"GuiGroup">;

export function Group(props: GroupProps) {
  return createElement(componentContract.GuiGroup.host, props);
}
