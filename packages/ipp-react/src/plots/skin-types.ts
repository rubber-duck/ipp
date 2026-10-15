import type { ComponentFields, CustomMaterialProps } from "../components.js";
import type {
  PlotCalloutStyleProps,
  PlotCalloutPaintProps,
} from "./declarations.js";
import type { PaintProps } from "../gui/components.js";

/** Client-authored chart appearance; geometry, data and picking stay native. */
export interface PlotSkinAppearance {
  readonly paint?: PaintProps;
  readonly material?: CustomMaterialProps;
  readonly callout?: PlotCalloutStyleProps;
  readonly calloutPaint?: PlotCalloutPaintProps;
  readonly frame2d?: Partial<ComponentFields<"PlotFrame2d">>;
  readonly frame3d?: Partial<ComponentFields<"PlotFrame3d">>;
  /** One connected Canvas sheet; legend sizing stays caller-authored. */
  readonly legend?: {
    readonly paint?: PaintProps;
    readonly swatchPaint?: PaintProps;
    readonly textColor?: readonly [number, number, number, number];
  };
}
