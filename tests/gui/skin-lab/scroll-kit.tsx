/**
 * Placement and rows of the scroll specimens: a list frame at the sheet's
 * size whose padding places the rows (see
 * [scroll-geometry.ts](scroll-geometry.ts)), and the sheet's list rows.
 */
import type { ReactNode } from "react";
import { Children, Entity, type AssetReference } from "@ipp/react";
import { Layout } from "@ipp/react/gui";
import { CAPS_HEIGHT, CAPS_TOP, Fill, Label } from "./kit.js";
import { FRAME, PADDING, ROW } from "./scroll-geometry.js";
import type { Point } from "./specimen.js";
import { BAR, BAR_INSET, INSET, LINE, TEXT_SMALL } from "./themes/geometry.js";
import { line as separatorColor, text as textColor } from "./themes/palette.js";

/** Row width: rows end a bar inset short of the bar. */
export const ROW_WIDTH = FRAME[0] - 2 * BAR_INSET - BAR;

/** Separators start half the content inset into the frame. */
const LINE_START = INSET / 2;

/** Text line box start within a row: the content inset, capitals centred. */
const TEXT_INSET: Point = [
  INSET,
  ROW / 2 - (CAPS_TOP + CAPS_HEIGHT / 2) * TEXT_SMALL,
];

/** A scrolling control's entity at `at` with the frame's size and row padding. */
export function ScrollFrame({
  id,
  at,
  children,
  content,
}: {
  readonly id: string;
  readonly at: Point;
  readonly children?: ReactNode;
  readonly content?: ReactNode;
}) {
  return (
    <Entity id={id}>
      <Layout
        kind={0}
        width={FRAME[0]}
        height={FRAME[1]}
        margin_left={at[0]}
        margin_top={at[1]}
        align_x={-1}
        align_y={-1}
        padding_top={PADDING.top}
        padding_bottom={PADDING.bottom}
      />
      {children}
      {content !== undefined && <Children>{content}</Children>}
    </Entity>
  );
}

/**
 * The layout and children of one list row: monospace text and, unless it is
 * the last row, a thin separator along its bottom. Use it inside an
 * `Entity`, or as a VirtualList item's declarations; `id` prefixes the child
 * entities.
 */
export function scrollRow({
  id,
  text,
  font,
  last,
}: {
  readonly id: string;
  readonly text: string;
  readonly font: AssetReference;
  readonly last: boolean;
}): ReactNode {
  return (
    <>
      <Layout kind={3} width={ROW_WIDTH} height={ROW} />
      <Children>
        <Label
          id={`${id}/text`}
          at={TEXT_INSET}
          width={ROW_WIDTH - TEXT_INSET[0]}
          text={text}
          font={font}
          size={TEXT_SMALL}
          color={textColor}
        />
        {!last && (
          <Fill
            id={`${id}/line`}
            rect={[LINE_START, ROW - LINE, ROW_WIDTH - LINE_START, LINE]}
            color={separatorColor}
          />
        )}
      </Children>
    </>
  );
}
