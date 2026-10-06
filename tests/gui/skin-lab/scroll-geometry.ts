/**
 * Geometry shared by the scroll specimens (a06-scroll-view, a07-virtual-list),
 * in sheet units: a list frame of three rows, as the sheet draws it, in the
 * design language's sizes (`themes/geometry.ts`). Rows fill the frame with an
 * equal margin above and below.
 *
 * The runtime places the bar from the control box: at the specimens' 16-unit
 * body type its default is `BAR` wide, `BAR_INSET` inside the frame's outer
 * right edge and half a bar inside its top and bottom edges, the track's ends
 * pointed over half a bar (`crates/ipp-core/src/world/systems/gui/layout/scroll_bars.rs`).
 * The padding here only places the rows.
 */
import type { Point, Rect } from "./specimen.js";
import { BAR, BAR_INSET, DENSE_ROW, LINE } from "./themes/geometry.js";

/** Outer size of a list frame. */
export const FRAME = [228, 80] as const;

/** Fixed row height. */
export const ROW = DENSE_ROW;

/** From the frame's outer top edge to the first row. */
const ROWS_TOP = (FRAME[1] - 3 * ROW) / 2;

/**
 * Viewport height: three rows without the third row's separator, which would
 * otherwise double the frame's bottom line, ending on a whole unit so the clip
 * does not show a sliver of it.
 */
export const VIEWPORT_HEIGHT = 3 * ROW - Math.ceil(LINE);

/** Padding of the scrolling control: rows where the sheet has them. */
export const PADDING = {
  top: ROWS_TOP,
  bottom: FRAME[1] - ROWS_TOP - VIEWPORT_HEIGHT,
} as const;

/**
 * Pointer position on the vertical thumb at a scroll offset, following the
 * runtime geometry: the track runs between the end insets, the thumb travels
 * the track without its pointed ends, is as long as the visible share of that
 * travel and at least two bars, and moves through the rest in proportion to
 * the offset.
 */
export function thumbCentre(
  [x, y, width, height]: Rect,
  content: number,
  offset: number,
): Point {
  const end = BAR / 2;
  const travel = height - 2 * end - BAR;
  const length = Math.max((travel * VIEWPORT_HEIGHT) / content, 2 * BAR);
  return [
    x + width - BAR_INSET - BAR / 2,
    y +
      end +
      BAR / 2 +
      ((travel - length) * offset) / (content - VIEWPORT_HEIGHT) +
      length / 2,
  ];
}
