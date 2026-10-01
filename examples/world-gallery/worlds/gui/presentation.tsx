/**
 * Ordinary Canvas building blocks for the GUI demo: layout containers, plain
 * decoration boxes, text and verified glyphs from Shure Tech Mono Nerd Font,
 * and theme part rows encoded through the gallery runtime's generated contract.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "@ipp/react";
import { Box, Layout, Style, Text } from "@ipp/react/gui";
import { GALLERY_RUNTIME } from "../../shared/runtime.js";

export type Color = readonly [number, number, number, number];
type Pair = readonly [number, number];

/** Top, right, bottom and left logical insets. */
export type Edges = readonly [number, number, number, number];

export const GUI_ICONS = {
  dashboard: "\ueacd", // nf-cod-dashboard
  signal: "\uf012", // nf-fa-signal
  pulse: "\ueb31", // nf-cod-pulse
  aurora: "\uf2dc", // nf-fa-snowflake_o
  ember: "\uf06d", // nf-fa-fire
  neon: "\uf0e7", // nf-fa-flash
} as const;

/** GuiLayout operations. */
const LEAF = 0;
const ROW = 1;
const COLUMN = 2;
const STACK = 3;
const PADDING = 4;
const ALIGN = 5;

/** The constraint box every layout entity shares. */
export interface BoxModel {
  readonly width?: number;
  readonly height?: number;
  readonly minHeight?: number;
  readonly margin?: Edges;
  readonly padding?: Edges;
  readonly alignX?: number;
  readonly alignY?: number;
  readonly flex?: number;
  readonly clip?: boolean;
}

/** One GuiLayout declaration; unset lengths keep the runtime defaults. */
export function BoxLayout({
  kind,
  width,
  height,
  minHeight,
  margin,
  padding,
  alignX,
  alignY,
  flex,
  clip,
}: BoxModel & { readonly kind: number }) {
  return (
    <Layout
      kind={kind}
      width={width}
      height={height}
      min_height={minHeight}
      margin_top={margin?.[0]}
      margin_right={margin?.[1]}
      margin_bottom={margin?.[2]}
      margin_left={margin?.[3]}
      padding_top={padding?.[0]}
      padding_right={padding?.[1]}
      padding_bottom={padding?.[2]}
      padding_left={padding?.[3]}
      align_x={alignX}
      align_y={alignY}
      flex={flex}
      clip={clip}
    />
  );
}

/** Linear RGBA fill or tint of one Canvas entity. */
export function Tint({
  color,
  opacity,
}: {
  readonly color: Color;
  readonly opacity?: number;
}) {
  return (
    <Style
      red={color[0]}
      green={color[1]}
      blue={color[2]}
      alpha={color[3]}
      opacity={opacity}
    />
  );
}

type ContainerProps = BoxModel & {
  readonly id: string;
  readonly children?: ReactNode;
};

function container(kind: number, { id, children, ...box }: ContainerProps) {
  return (
    <Entity id={id}>
      <BoxLayout kind={kind} {...box} />
      <Children>{children}</Children>
    </Entity>
  );
}

/** Children overlap from the top-left, each placed by its own align lanes. */
export function Stack(props: ContainerProps) {
  return container(STACK, props);
}

/** Children follow left to right; each aligns itself vertically. */
export function Row(props: ContainerProps) {
  return container(ROW, props);
}

/** Children follow top to bottom; each aligns itself horizontally. */
export function Column(props: ContainerProps) {
  return container(COLUMN, props);
}

/** One child placed inside the cell by its alignment, centred by default. */
export function Align(props: ContainerProps) {
  return container(ALIGN, props);
}

/** One child offset by the padding. */
export function Padding(props: ContainerProps) {
  return container(PADDING, props);
}

/** A fixed or flexible gap with no paint. */
export function Spacer({ id, ...box }: BoxModel & { readonly id: string }) {
  return (
    <Entity id={id}>
      <BoxLayout kind={LEAF} {...box} />
    </Entity>
  );
}

/**
 * A plain rounded decoration box. Canvas boxes fill their settled layout
 * rectangle with one colour; `x` and `y` offset the box from the start of
 * its parent Stack by adding to its leading margins, so the Stack extent,
 * layout bounds and painted position agree.
 */
export function Shape({
  id,
  x = 0,
  y = 0,
  margin = [0, 0, 0, 0],
  color,
  radius = 0,
  ...box
}: BoxModel & {
  readonly id: string;
  readonly x?: number;
  readonly y?: number;
  readonly color: Color;
  readonly radius?: number;
}) {
  return (
    <Entity id={id}>
      <BoxLayout
        kind={LEAF}
        {...box}
        margin={[margin[0] + y, margin[1], margin[2], margin[3] + x]}
      />
      <Tint color={color} />
      <Box
        width={box.width}
        height={box.height}
        radius_x={radius}
        radius_y={radius}
      />
    </Entity>
  );
}

/** A text leaf. A fixed `width` wraps it at word boundaries. */
export function TextLeaf({
  id,
  text,
  font,
  fontSize,
  color,
  ...box
}: BoxModel & {
  readonly id: string;
  readonly text: string;
  readonly font: string;
  readonly fontSize: number;
  readonly color: Color;
}) {
  return (
    <Entity id={id}>
      <BoxLayout kind={LEAF} {...box} />
      <Tint color={color} />
      <Text text={text} source={font} font_size={fontSize} />
    </Entity>
  );
}

/** A single-line label aligned inside a fixed cell. */
export function Label({
  id,
  text,
  font,
  width,
  height,
  size = 0.22,
  color,
  right = false,
}: {
  readonly id: string;
  readonly text: string;
  readonly font: string;
  readonly width: number;
  readonly height: number;
  readonly size?: number;
  readonly color: Color;
  readonly right?: boolean;
}) {
  return (
    <Align
      id={id}
      width={width}
      height={height}
      alignX={right ? 1 : -1}
      alignY={0}
    >
      <Entity id={`${id}/text`}>
        <Tint color={color} />
        <Text text={text} source={font} font_size={size} />
      </Entity>
    </Align>
  );
}

/**
 * A glyph in a fixed cell. The glyph measures at its intrinsic line box: its
 * advance wide and one line tall. The Nerd Font centres each icon's ink in
 * that box, so the alignment in the cell positions the ink itself, centred
 * by default. The cell is a Padding with no align lanes, so its parent
 * Stack, Row or Column places it at the start; the Align filling the cell
 * places the glyph. Choose `fontSize` by ink width: every icon advance is
 * 0.54 em, and the 1.127 em line must fit the cell height.
 */
export function IconCell({
  id,
  width,
  height,
  alignX = 0,
  glyph,
  font,
  fontSize,
  color,
}: {
  readonly id: string;
  readonly width: number;
  readonly height: number;
  readonly alignX?: number;
  readonly glyph: string;
  readonly font: string;
  readonly fontSize: number;
  readonly color: Color;
}) {
  return (
    <Padding id={id} width={width} height={height}>
      <Align id={`${id}/align`} alignX={alignX} alignY={0}>
        <Entity id={`${id}/glyph`}>
          <Tint color={color} />
          <Text text={glyph} source={font} font_size={fontSize} />
        </Entity>
      </Align>
    </Padding>
  );
}

/** Two-stop gradient in the part's local logical units. */
export interface Gradient {
  readonly kind: "linear" | "radial";
  readonly start: Pair;
  readonly end?: Pair;
  readonly radius?: number;
  readonly color0: Color;
  readonly color1: Color;
}

/** Glow around the part's outer boundary; it paints outside the hit area. */
export interface Glow {
  readonly color?: Color;
  readonly intensity: number;
  readonly radius?: number;
  readonly falloff?: number;
}

/** Skin motion clip sampled when a control enters this appearance. */
export interface Transition {
  readonly motion: string;
  readonly duration: number;
  readonly smoothstep: boolean;
  readonly track: number;
  readonly time: number;
}

export interface PartStyle {
  readonly color?: Color;
  readonly opacity?: number;
  readonly scale?: Pair;
  readonly alignX?: number;
  readonly cornerRadius?: Pair;
  readonly borderWidth?: number;
  readonly borderColor?: Color;
  readonly gradient?: Gradient;
  readonly glow?: Glow;
  readonly transition?: Transition;
}

export type PartName =
  | "background"
  | "fill"
  | "label"
  | "icon"
  | "focusRing"
  | "scrollTrackY"
  | "scrollThumbY";

type InteractionState = "idle" | "hovered" | "pressed" | "disabled";

/** Per-part appearance. States refine the base; checked and unchecked
 * refine every state of a checkbox. */
export type ThemedPart = { readonly base?: PartStyle } & {
  readonly [State in InteractionState | "checked" | "unchecked"]?: PartStyle;
};

export type ThemeParts = { readonly [Part in PartName]?: ThemedPart };

interface PaintKey {
  readonly part: PartName;
  readonly state?: InteractionState;
  readonly variant?: "checked" | "unchecked";
}

interface ThemeRow {
  part: number;
  color?: Color;
  opacity?: number;
  scale?: Pair;
  align_x?: number;
  corner_radius?: Pair;
  border_width?: number;
  border_color?: Color;
  fill_mode?: number;
  gradient_start?: Pair;
  gradient_end?: Pair;
  gradient_color0?: Color;
  gradient_color1?: Color;
  gradient_radius?: number;
  glow_color?: Color;
  glow_intensity?: number;
  glow_radius?: number;
  glow_falloff?: number;
}

interface MotionRow {
  part: number;
  source: { kind: number; source: string; variant: number };
  duration: number;
  easing: number;
  track: number;
  time: number;
}

interface RowsInput<Row> {
  readonly nextSlot: number;
  readonly rows: ReadonlyMap<number, Row>;
}

/** The generated row encoders and paint keys of the gallery runtime. */
interface GalleryGuiContract {
  guiPaintPartIndex(key: PaintKey): number;
  readonly GuiTheme: {
    encodeParts(table: RowsInput<ThemeRow>): Uint8Array<ArrayBuffer>;
  };
  readonly GuiThemeMotion: {
    encodeParts(table: RowsInput<MotionRow>): Uint8Array<ArrayBuffer>;
  };
}

const contract: GalleryGuiContract = await import(
  `${GALLERY_RUNTIME}generated.js`
);

const STATES: readonly InteractionState[] = [
  "idle",
  "hovered",
  "pressed",
  "disabled",
];

function appearance(
  part: number,
  style: PartStyle,
  solidOverGradient: boolean,
): ThemeRow {
  const row: ThemeRow = { part };
  if (style.color) row.color = style.color;
  if (style.opacity !== undefined) row.opacity = style.opacity;
  if (style.scale) row.scale = style.scale;
  if (style.alignX !== undefined) row.align_x = style.alignX;
  if (style.cornerRadius) row.corner_radius = style.cornerRadius;
  if (style.borderWidth !== undefined) row.border_width = style.borderWidth;
  if (style.borderColor) row.border_color = style.borderColor;
  const gradient = style.gradient;
  if (gradient) {
    row.fill_mode = gradient.kind === "radial" ? 2 : 1;
    row.gradient_start = gradient.start;
    if (gradient.end) row.gradient_end = gradient.end;
    if (gradient.radius !== undefined) row.gradient_radius = gradient.radius;
    row.gradient_color0 = gradient.color0;
    row.gradient_color1 = gradient.color1;
  } else if (solidOverGradient && style.color) {
    // Fill mode resolves like any other property: a state that only sets a
    // colour selects a solid fill instead of a less specific gradient.
    row.fill_mode = 0;
  }
  const glow = style.glow;
  if (glow) {
    if (glow.color) row.glow_color = glow.color;
    row.glow_intensity = glow.intensity;
    if (glow.radius !== undefined) row.glow_radius = glow.radius;
    if (glow.falloff !== undefined) row.glow_falloff = glow.falloff;
  }
  return row;
}

/** Encoded theme part rows and, when any appearance declares a transition,
 * the matching skin motion rows. Row slots follow the paint key order, so a
 * theme whose appearance changes in place keeps each part's slot. */
export interface EncodedTheme {
  readonly parts: Uint8Array<ArrayBuffer>;
  readonly motion?: Uint8Array<ArrayBuffer>;
}

export function encodeTheme(parts: ThemeParts): EncodedTheme {
  const styled: [number, PartStyle, boolean][] = [];
  for (const [name, themed] of Object.entries(parts) as [
    PartName,
    ThemedPart,
  ][]) {
    const hasGradient = Object.values(themed).some(
      (style) => style?.gradient !== undefined,
    );
    const add = (
      key: PaintKey,
      style: PartStyle | undefined,
      solid: boolean,
    ) => {
      if (style) styled.push([contract.guiPaintPartIndex(key), style, solid]);
    };
    add({ part: name }, themed.base, false);
    for (const state of STATES)
      add({ part: name, state }, themed[state], hasGradient);
    for (const variant of ["checked", "unchecked"] as const)
      for (const state of STATES)
        add({ part: name, state, variant }, themed[variant], hasGradient);
  }
  styled.sort(([left], [right]) => left - right);
  const rows = new Map<number, ThemeRow>();
  const motions = new Map<number, MotionRow>();
  styled.forEach(([part, style, solid], slot) => {
    rows.set(slot, appearance(part, style, solid));
    const transition = style.transition;
    if (transition)
      motions.set(slot, {
        part,
        source: { kind: 10, source: transition.motion, variant: 0 },
        duration: transition.duration,
        easing: transition.smoothstep ? 1 : 0,
        track: transition.track,
        time: transition.time,
      });
  });
  return {
    parts: contract.GuiTheme.encodeParts({ nextSlot: rows.size, rows }),
    ...(motions.size
      ? {
          motion: contract.GuiThemeMotion.encodeParts({
            nextSlot: rows.size,
            rows: motions,
          }),
        }
      : {}),
  };
}
