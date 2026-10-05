/**
 * The panel World's units and the pieces the dashboard shares: the gallery
 * runtime's generated contract with the design language's tokens and built-in
 * looks, the canvas density that keeps the Surface's physical size, and the
 * plain layout entities the kit does not provide.
 *
 * The panel is authored in the design language's own units: body text 16
 * units on a 4-unit grid, as `GUI_SKIN_TOKENS` states its lengths. The canvas
 * density maps those units to the Surface's metres, so the kit draws at its
 * design size (`GuiKit fontSize={16}`) and the code reads like the language.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "@ipp/react";
import { Behavior, Box, Layout, Style, Text } from "@ipp/react/gui";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import * as HostContract from "@ipp/host-contract";

/** Linear RGBA. */
export type Color = readonly [number, number, number, number];

/** Built-in look rows and transition timing, as the contract exports them. */
interface GalleryLook {
  readonly em: number;
  readonly parts: readonly ({ readonly part: number } & Record<
    string,
    unknown
  >)[];
  readonly motion: readonly {
    part: number;
    duration: number;
    easing: number;
  }[];
}

type RowsTable<Row> = {
  readonly nextSlot: number;
  readonly rows: ReadonlyMap<number, Row>;
};

/** What the dashboard uses of the gallery runtime's generated contract. */
interface GalleryContract extends GuiKitContract {
  readonly GUI_SKIN_LOOKS: GuiKitContract["GUI_SKIN_LOOKS"] & {
    readonly button: GalleryLook;
    readonly amber: GalleryLook;
    readonly switch: GalleryLook;
    readonly scroll: GalleryLook;
  };
  readonly GuiThemeMotion: {
    encodeParts(table: RowsTable<unknown>): Uint8Array<ArrayBuffer>;
  };
}

/** The gallery runtime's generated contract module. */
export const CONTRACT = HostContract as unknown as GalleryContract;

/** The design language's colours and lengths. */
export const TOKENS = CONTRACT.GUI_SKIN_TOKENS;

/** Body text, the size every look and kit length is designed at. */
export const BODY = TOKENS.textBody;

/** The panel's physical size: the Surface the projector frames, in metres. */
export const SURFACE_WIDTH = 7.4;
export const SURFACE_HEIGHT = 4.8;

/** Shared curvature radius for the dashboard and its scene-geometry shield. */
export const SURFACE_RADIUS = 8;

/** Ordinary layers retain painter/input priority on coincident physical planes. */
export const REST_LAYER_SPACING = 0;

/**
 * Canvas units per Surface metre. Body text is then 16 / 140 = 0.114 m, about
 * 13 pixels at the authored camera on a typical desktop canvas, and the panel
 * holds the scanner's 1036 by 672-unit window.
 */
export const UNITS_PER_METRE = 140;

/** The canvas extent in units: the Surface at the canvas density. */
export const CANVAS_WIDTH = 1036;
export const CANVAS_HEIGHT = 672;

/** The right column's tabbed panel: NODES, CONTROLS and COLOUR. */
export const WORKBENCH_WIDTH = 288;
export const WORKBENCH_HEIGHT = 448;

/** `[x, y, width, height]` in canvas units. */
export type Rect = readonly [number, number, number, number];

/** Top, right, bottom and left insets. */
export type Edges = readonly [number, number, number, number];

/** GuiLayout operations. */
export const LEAF = 0;
export const ROW = 1;
export const COLUMN = 2;
export const STACK = 3;

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

/** Linear RGBA tint and opacity of one Canvas entity and its subtree. */
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
      {children !== undefined && <Children>{children}</Children>}
    </Entity>
  );
}

/** Children overlap, each placed by its own alignment. */
export function Stack(props: ContainerProps) {
  return container(STACK, props);
}

/** Children follow left to right. */
export function Row(props: ContainerProps) {
  return container(ROW, props);
}

/** Children follow top to bottom. */
export function Column(props: ContainerProps) {
  return container(COLUMN, props);
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
 * A panel's content: a column filling its container, so separators and rows
 * inside span the frame. Everything a panel holds shares its plane; only
 * the kit's overlays float above the canvas's base.
 */
export function PanelBody({
  id,
  children,
}: {
  readonly id: string;
  readonly children?: ReactNode;
}) {
  return (
    <Entity id={id}>
      <BoxLayout kind={COLUMN} flex={1} />
      <Children>{children}</Children>
    </Entity>
  );
}

/**
 * A column of content that can be hidden while its entities, and the state
 * and animation they hold, stay: hidden content paints nothing and its
 * controls take no input, but it keeps its layout space.
 */
export function Section({
  id,
  hidden = false,
  children,
  ...box
}: BoxModel & {
  readonly id: string;
  readonly hidden?: boolean;
  readonly children?: ReactNode;
}) {
  return (
    <Entity id={id}>
      <BoxLayout kind={COLUMN} {...box} />
      <Behavior visible={!hidden} />
      <Children>{children}</Children>
    </Entity>
  );
}

/** A plain filled box over its layout rectangle. */
export function Fill({
  id,
  color,
  ...box
}: BoxModel & { readonly id: string; readonly color: Color }) {
  return (
    <Entity id={id}>
      <BoxLayout kind={LEAF} {...box} />
      <Tint color={color} />
      <Box width={box.width} height={box.height} />
    </Entity>
  );
}

/** A text leaf in one colour. A fixed `width` wraps it at word boundaries. */
export function TextLeaf({
  id,
  text,
  font,
  size,
  color,
  ...box
}: BoxModel & {
  readonly id: string;
  readonly text: string;
  readonly font: string;
  readonly size: number;
  readonly color: Color;
}) {
  return (
    <Entity id={id}>
      <BoxLayout kind={LEAF} {...box} />
      <Tint color={color} />
      <Text text={text} source={font} font_size={size} />
    </Entity>
  );
}
