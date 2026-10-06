/** Caller-authored color keys, composed from ordinary Canvas declarations. */
import type { AssetReference } from "../assets/declarations.js";
import { Children, Entity } from "../components.js";
import { Box, Style, Text } from "../gui/components.js";

/** Straight linear RGBA, with each channel in 0..1 like CanvasStyle. */
export type PlotLegendColor = readonly [number, number, number, number];

export interface PlotLegendEntry {
  /** Stable caller identity; reordering preserves the entry's entities. */
  readonly id: string;
  readonly label: string;
  readonly color: PlotLegendColor;
}

/** Colors are equally spaced over an increasing range, interpolated linearly. */
export interface PlotColorScale {
  readonly min: number;
  readonly max: number;
  readonly colors: readonly PlotLegendColor[];
  /** Endpoint labels; include units here or in the legend title. */
  readonly format?: (value: number) => string;
}

export type PlotLegendContent =
  | { readonly entries: readonly PlotLegendEntry[]; readonly scale?: never }
  | { readonly entries?: never; readonly scale: PlotColorScale };

export interface PlotLegendStyle {
  /** Fixed logical width. Text is clipped inside it; enlarge it for long labels. */
  readonly width?: number;
  readonly fontSize?: number;
  readonly rowHeight?: number;
  readonly padding?: number;
  readonly textColor?: PlotLegendColor;
}

export type PlotLegendProps = PlotLegendContent &
  PlotLegendStyle & {
    readonly id: string;
    /** Font reference in the containing root, or a registered font source. */
    readonly font: string | AssetReference;
    readonly title?: string;
    readonly x?: number;
    readonly y?: number;
    /** Nonnegative layer offset at the legend root; descendants inherit it. */
    readonly layer?: number;
  };

type LegendSizing = PlotLegendContent & PlotLegendStyle & { title?: string };
const SCALE_STRIPS = 32;
const DEFAULT_TEXT: PlotLegendColor = [0.82, 0.88, 0.94, 1];

function positive(value: number): boolean {
  return Number.isFinite(value) && value > 0;
}

function validateColor(color: PlotLegendColor): void {
  if (
    !Array.isArray(color) ||
    color.length !== 4 ||
    !color.every((value) => Number.isFinite(value) && value >= 0 && value <= 1)
  )
    throw new RangeError(
      "Plot legend colors must be four finite channels in 0..1",
    );
}

function validateScale(scale: PlotColorScale): void {
  if (
    !Number.isFinite(scale.min) ||
    !Number.isFinite(scale.max) ||
    !positive(scale.max - scale.min)
  )
    throw new RangeError(
      "Plot color scale must have a finite increasing range",
    );
  if (scale.colors.length < 2)
    throw new RangeError("Plot color scale needs at least two colors");
  for (const color of scale.colors) validateColor(color);
}

function scaleColor(scale: PlotColorScale, value: number): PlotLegendColor {
  if (value <= scale.min) return scale.colors[0]!;
  if (value >= scale.max) return scale.colors[scale.colors.length - 1]!;
  const fraction = Math.max(
    0,
    Math.min(1, (value - scale.min) / (scale.max - scale.min)),
  );
  const position = fraction * (scale.colors.length - 1);
  const index = Math.min(Math.floor(position), scale.colors.length - 2);
  const left = scale.colors[index]!;
  const right = scale.colors[index + 1]!;
  const blend = position - index;
  return [
    left[0] + (right[0] - left[0]) * blend,
    left[1] + (right[1] - left[1]) * blend,
    left[2] + (right[2] - left[2]) * blend,
    left[3] + (right[3] - left[3]) * blend,
  ];
}

/** Use the same scale for producer colors and the legend, without reading data. */
export function plotColorScaleColor(
  scale: PlotColorScale,
  value: number,
): PlotLegendColor {
  validateScale(scale);
  if (!Number.isFinite(value))
    throw new RangeError("Plot color scale value must be finite");
  return scaleColor(scale, value);
}

function legendGeometry(props: LegendSizing) {
  const fontSize = props.fontSize ?? 14;
  const rowHeight = props.rowHeight ?? (fontSize * 24) / 14;
  const padding = props.padding ?? 8;
  const width = props.width ?? 180;
  const swatch = fontSize;
  const gap = fontSize / 2;
  if (
    !positive(fontSize) ||
    !positive(rowHeight) ||
    rowHeight < fontSize ||
    !Number.isFinite(padding) ||
    padding < 0 ||
    !positive(width) ||
    width <= 2 * padding + swatch + gap
  )
    throw new RangeError(
      "Plot legend dimensions must leave room for swatches and labels",
    );
  validateColor(props.textColor ?? DEFAULT_TEXT);
  if ((props.entries === undefined) === (props.scale === undefined))
    throw new Error("Plot legend requires entries or a color scale");
  if (props.scale) validateScale(props.scale);
  if (props.entries) {
    const ids = new Set<string>();
    for (const entry of props.entries) {
      if (typeof entry.id !== "string" || !entry.id || ids.has(entry.id))
        throw new Error("Plot legend entries need unique nonempty ids");
      ids.add(entry.id);
      validateColor(entry.color);
    }
  }
  const titleRows = props.title ? 1 : 0;
  const contentHeight =
    2 * padding +
    rowHeight * (titleRows + (props.scale ? 3 : props.entries!.length));
  const height = Math.max(rowHeight, contentHeight);
  if (!positive(height))
    throw new RangeError("Plot legend height must be finite and positive");
  return {
    width,
    height,
    fontSize,
    rowHeight,
    padding,
    swatch,
    gap,
    titleRows,
  };
}

/** Logical Canvas extent, including padding and an optional title row. */
export function plotLegendSize(props: LegendSizing): readonly [number, number] {
  const { width, height } = legendGeometry(props);
  return [width, height];
}

export type PlotLegendSide = "left" | "right" | "top" | "bottom";
export type PlotLegendOrigin =
  | "bottom-left"
  | "bottom-right"
  | "top-left"
  | "top-right";

export interface PlotLegendPlacementOptions {
  /** Ordered numeric bounds, in the same coordinates/units as size and gap. */
  readonly bounds: readonly [number, number, number, number];
  readonly size: readonly [number, number];
  /** Canvas coordinates increase down; scene XY coordinates increase up. */
  readonly yDirection: "down" | "up";
  readonly origin?: PlotLegendOrigin;
  /** Defaults to the horizontal side opposite the origin, vertically centered. */
  readonly side?: PlotLegendSide;
  /** Defaults to one tenth of the legend width, independent of the unit scale. */
  readonly gap?: number;
}

export interface PlotLegendPlacement {
  readonly bounds: readonly [number, number, number, number];
  /** Top-left in the requested coordinate convention; use directly in a Canvas. */
  readonly canvasPosition: readonly [number, number];
  /** Place a FlatSurface's centered local XY extent here in a scene. */
  readonly center: readonly [number, number];
}

/** Place a legend outside a chart in its local XY plane, with an explicit Y axis. */
export function plotLegendPlacement({
  bounds,
  size,
  yDirection,
  origin = "bottom-left",
  side,
  gap = size[0] / 10,
}: PlotLegendPlacementOptions): PlotLegendPlacement {
  if (
    bounds.length !== 4 ||
    !bounds.every(Number.isFinite) ||
    bounds[0] >= bounds[2] ||
    bounds[1] >= bounds[3] ||
    size.length !== 2 ||
    !size.every(positive) ||
    !Number.isFinite(gap) ||
    gap < 0 ||
    !["up", "down"].includes(yDirection) ||
    !["bottom-left", "bottom-right", "top-left", "top-right"].includes(origin)
  )
    throw new RangeError(
      "Plot legend placement needs ordered bounds, positive size and an explicit Y direction",
    );
  const resolvedSide = side ?? (origin.endsWith("left") ? "right" : "left");
  if (!["left", "right", "top", "bottom"].includes(resolvedSide))
    throw new RangeError("Plot legend side must be left, right, top or bottom");
  const [width, height] = size;
  let x = (bounds[0] + bounds[2] - width) / 2;
  let y = (bounds[1] + bounds[3] - height) / 2;
  if (resolvedSide === "right") x = bounds[2] + gap;
  if (resolvedSide === "left") x = bounds[0] - gap - width;
  if (resolvedSide === "top")
    y = yDirection === "down" ? bounds[1] - gap - height : bounds[3] + gap;
  if (resolvedSide === "bottom")
    y = yDirection === "down" ? bounds[3] + gap : bounds[1] - gap - height;
  const result = [x, y, x + width, y + height] as const;
  if (!result.every(Number.isFinite))
    throw new RangeError("Plot legend placement must remain finite");
  return {
    bounds: result,
    canvasPosition: [x, yDirection === "down" ? y : y + height],
    center: [x + width / 2, y + height / 2],
  };
}

/**
 * A Canvas color key. Compose it in the chart's Canvas or on an ordinary Surface
 * with its own CanvasWorld. This component creates no Worlds, assets or observers.
 */
export function PlotLegend(props: PlotLegendProps) {
  const { id, font, title, entries, scale, x = 0, y = 0, layer = 0 } = props;
  const g = legendGeometry(props);
  if (!id) throw new Error("Plot legend needs a nonempty id");
  if (
    !Number.isFinite(x) ||
    !Number.isFinite(y) ||
    !Number.isInteger(layer) ||
    layer < 0 ||
    layer > 0xffffffff
  )
    throw new RangeError(
      "Plot legend position must be finite and layer a nonnegative u32",
    );
  if (entries?.length === 0 && !title) return null;
  const textColor = props.textColor ?? DEFAULT_TEXT;
  const start = g.padding + g.titleRows * g.rowHeight;
  const text = (
    name: string,
    label: string,
    left: number,
    top: number,
    width: number,
  ) => (
    <Entity key={name} id={name}>
      <Style
        x={left}
        y={top}
        red={textColor[0]}
        green={textColor[1]}
        blue={textColor[2]}
        alpha={textColor[3]}
        clipped
        clip_min_x={0}
        clip_min_y={0}
        clip_max_x={width}
        clip_max_y={g.rowHeight}
      />
      <Text text={label} source={font} font_size={g.fontSize} />
    </Entity>
  );
  const box = (
    name: string,
    color: PlotLegendColor,
    left: number,
    top: number,
    width: number,
    height: number,
  ) => (
    <Entity key={name} id={name}>
      <Style
        x={left}
        y={top}
        red={color[0]}
        green={color[1]}
        blue={color[2]}
        alpha={color[3]}
      />
      <Box width={width} height={height} />
    </Entity>
  );
  const innerWidth = g.width - 2 * g.padding;
  const rampHeight = 3 * g.rowHeight;
  const stripHeight = rampHeight / SCALE_STRIPS;
  // Paint opaque bands down to the ramp's bottom: every internal antialiased
  // edge blends over its neighboring color, even when bands are subpixel.
  // Translucent palettes keep separate strips without doubled alpha coverage.
  const opaqueScale = scale?.colors.every((color) => color[3] === 1) ?? false;
  return (
    <Entity id={id}>
      <Style
        x={x}
        y={y}
        layer={layer}
        clipped
        clip_min_x={0}
        clip_min_y={0}
        clip_max_x={g.width}
        clip_max_y={g.height}
      />
      <Children>
        {title && text(`${id}/title`, title, g.padding, g.padding, innerWidth)}
        {entries?.map((entry, index) => {
          const entryId = `${id}/entry/${encodeURIComponent(entry.id)}`;
          const top = start + index * g.rowHeight;
          return (
            <Entity key={entry.id} id={entryId}>
              <Children>
                {box(
                  `${entryId}/swatch`,
                  entry.color,
                  g.padding,
                  top + (g.rowHeight - g.swatch) / 2,
                  g.swatch,
                  g.swatch,
                )}
                {text(
                  `${entryId}/label`,
                  entry.label,
                  g.padding + g.swatch + g.gap,
                  top,
                  innerWidth - g.swatch - g.gap,
                )}
              </Children>
            </Entity>
          );
        })}
        {scale && (
          <>
            {Array.from({ length: SCALE_STRIPS }, (_, index) => {
              const value =
                index === SCALE_STRIPS - 1
                  ? scale.min
                  : scale.max -
                    ((scale.max - scale.min) * index) / (SCALE_STRIPS - 1);
              return box(
                `${id}/scale/${index}`,
                scaleColor(scale, value),
                g.padding,
                start + stripHeight * index,
                g.swatch,
                opaqueScale ? rampHeight - stripHeight * index : stripHeight,
              );
            })}
            {text(
              `${id}/min`,
              scale.format?.(scale.min) ?? String(scale.min),
              g.padding + g.swatch + g.gap,
              start + 2 * g.rowHeight,
              innerWidth - g.swatch - g.gap,
            )}
            {text(
              `${id}/max`,
              scale.format?.(scale.max) ?? String(scale.max),
              g.padding + g.swatch + g.gap,
              start,
              innerWidth - g.swatch - g.gap,
            )}
          </>
        )}
      </Children>
    </Entity>
  );
}
