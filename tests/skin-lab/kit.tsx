/**
 * Placement helpers for specimens. Every helper declares ordinary Canvas and
 * GUI entities; the canvas root is a Stack, so each placed entity sits at its
 * logical rectangle through its leading margins. Composites such as panels,
 * window controls and data grids come from the GUI kit, not from here.
 */
import type { ReactNode } from "react";
import { Children, Entity, type AssetReference } from "@ipp/react";
import {
  Behavior,
  Box,
  Button,
  Font,
  Layout,
  Style,
  Text,
  type GuiControlRef,
} from "@ipp/react/gui";
import type { GuiKitLayout } from "@ipp/react/gui-kit";
import { srgb, type Point, type Rect } from "./specimen.js";
import { type Color, page, text as textColor } from "./themes/palette.js";

/** Layout operations. */
export const LEAF = 0;
export const ROW = 1;
export const COLUMN = 2;
export const STACK = 3;

/**
 * Shure Tech Mono Nerd Font metrics in em: every glyph advance, the line
 * box, and where the capitals sit in it as measured in captures (a01).
 */
export const FONT_ADVANCE = 0.54;
export const FONT_LINE = 1.127;
export const CAPS_TOP = 0.153;
export const CAPS_HEIGHT = 0.7;

/** The sheet background behind the reference rows: the language's page. */
export const SHEET_PAGE = page;

/**
 * The sheet's caption colour under each state column: document annotation,
 * not part of the skin.
 */
export const SHEET_CAPTION = srgb("#a9c7d8");

/** An entity laid out at `rect`; `content` becomes its child entities. */
export function At({
  id,
  rect,
  children,
  content,
}: {
  readonly id: string;
  readonly rect: Rect;
  readonly children?: ReactNode;
  readonly content?: ReactNode;
}) {
  const [x, y, width, height] = rect;
  return (
    <Entity id={id}>
      <Layout
        kind={0}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      {children}
      {content !== undefined && <Children>{content}</Children>}
    </Entity>
  );
}

/** A plain filled rectangle, such as the sheet background behind a row. */
export function Fill({
  id,
  rect,
  color,
}: {
  readonly id: string;
  readonly rect: Rect;
  readonly color: Color;
}) {
  return (
    <At id={id} rect={rect}>
      <Style red={color[0]} green={color[1]} blue={color[2]} alpha={color[3]} />
      <Box width={rect[2]} height={rect[3]} />
    </At>
  );
}

/**
 * A single text line whose line box starts at `at`. A centred label is
 * centred in the `width` that starts at `at`.
 */
export function Label({
  id,
  at,
  text,
  font,
  size,
  width = 400,
  centred = false,
  color = textColor,
}: {
  readonly id: string;
  readonly at: Point;
  readonly text: string;
  readonly font: AssetReference;
  readonly size: number;
  readonly width?: number;
  readonly centred?: boolean;
  readonly color?: Color;
}) {
  const leaf = (
    <>
      <Style red={color[0]} green={color[1]} blue={color[2]} alpha={color[3]} />
      <Text text={text} source={font} font_size={size} />
    </>
  );
  if (!centred)
    return (
      <Entity id={id}>
        <Layout
          kind={0}
          width={width}
          margin_left={at[0]}
          margin_top={at[1]}
          align_x={-1}
          align_y={-1}
        />
        {leaf}
      </Entity>
    );
  // A Padding cell placed at the start of the Stack; the Align filling it
  // centres the text, because an Align's own lanes place its child.
  return (
    <Entity id={id}>
      <Layout
        kind={4}
        width={width}
        height={size * 2}
        margin_left={at[0]}
        margin_top={at[1]}
        align_x={-1}
        align_y={-1}
      />
      <Children>
        <Entity id={`${id}/align`}>
          <Layout kind={5} align_x={0} align_y={-1} />
          <Children>
            <Entity id={`${id}/text`}>{leaf}</Entity>
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

/** The centre of a rectangle, for pointer pins. */
export function centre(rect: Rect): Point {
  return [rect[0] + rect[2] / 2, rect[1] + rect[3] / 2];
}

/** Grow a rectangle by `margin` on every side, for a state cell around a control and its glow. */
export function around(rect: Rect, margin: number): Rect {
  return [
    rect[0] - margin,
    rect[1] - margin,
    rect[2] + 2 * margin,
    rect[3] + 2 * margin,
  ];
}

/**
 * Layout fields that place a kit component's root at `at` in the specimen's
 * Stack, `width` wide; its height is the component's own.
 */
export function placed(at: Point, width?: number): GuiKitLayout {
  return {
    margin_left: at[0],
    margin_top: at[1],
    align_x: -1,
    align_y: -1,
    ...(width === undefined ? {} : { width }),
  };
}

export interface Edges {
  readonly top?: number;
  readonly right?: number;
  readonly bottom?: number;
  readonly left?: number;
}

/**
 * A Button sized and placed by its margins, in the skin `skin`; the runtime
 * centres its label line in the box on both axes. A selected button paints
 * its checked variant; one that does not take focus is no Tab stop and
 * leaves focus where it is when pressed, as the items of lists and menus do.
 */
export function CentredButton({
  id,
  width,
  height,
  label,
  font,
  size,
  skin,
  disabled = false,
  selected = false,
  focusable = true,
  control,
  margin = {},
  alignY = 0,
}: {
  readonly id: string;
  readonly width: number;
  readonly height: number;
  readonly label: string;
  readonly font: AssetReference;
  readonly size: number;
  readonly skin: ReactNode;
  readonly disabled?: boolean;
  readonly selected?: boolean;
  readonly focusable?: boolean;
  readonly control?: GuiControlRef;
  readonly margin?: Edges;
  readonly alignY?: number;
}) {
  return (
    <Entity id={id}>
      <Layout
        kind={LEAF}
        width={width}
        height={height}
        align_x={-1}
        align_y={alignY}
        margin_top={margin.top ?? 0}
        margin_right={margin.right ?? 0}
        margin_bottom={margin.bottom ?? 0}
        margin_left={margin.left ?? 0}
      />
      {skin}
      <Font source={font} font_size={size} />
      {(disabled || !focusable) && (
        <Behavior enabled={!disabled} focusable={focusable} />
      )}
      <Button
        label={label}
        selected={selected}
        {...(control ? { ref: control } : {})}
      />
    </Entity>
  );
}
