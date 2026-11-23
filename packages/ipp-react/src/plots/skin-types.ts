import type { ComponentFields, CustomMaterialProps } from "../components.js";
import type {
  ChartName,
  PlotAxisPaintProps,
  PlotCalloutStyleProps,
  PlotCalloutPaintProps,
} from "./declarations.js";
import type { PaintProps } from "../gui/components.js";

/** Client-authored chart appearance; geometry, data and picking stay native. */
export interface PlotSkinAppearance {
  readonly paint?: PaintProps;
  /** A planar chart may use its own mark paint in place of the shared paint. */
  readonly chartPaints?: Partial<
    Record<Extract<ChartName, `${string}2d`>, PaintProps>
  >;
  /** Skin defaults for native marks; explicitly authored component fields win. */
  readonly marks?: {
    readonly [Name in ChartName]?: Omit<
      ComponentFields<Name>,
      "series" | "labels"
    >;
  };
  readonly material?: CustomMaterialProps;
  /** Optional finishes for a caller-sized Canvas sheet and a spatial base in local metres. */
  readonly paperPaint?: PaintProps;
  readonly baseMaterial?: CustomMaterialProps;
  readonly axisPaint?: PlotAxisPaintProps;
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
