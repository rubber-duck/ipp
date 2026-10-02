/**
 * Sheet a, row 06: ScrollView idle, with its thumb dragged, and scrolled to
 * the end, plus the thumb under the pointer, which the sheet does not show.
 */
import { Children, Entity } from "@ipp/react";
import { Font, Layout, ScrollView } from "@ipp/react/gui";
import { Fill, Label, SHEET_CAPTION, SHEET_PAGE } from "../kit.js";
import {
  FRAME,
  ROW,
  VIEWPORT_HEIGHT,
  thumbCentre,
} from "../scroll-geometry.js";
import { ROW_WIDTH, ScrollFrame, scrollRow } from "../scroll-kit.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [1010, 120] as const;
const ENTRIES = [
  "PULSE  SENT",
  "GAIN  UPDATED",
  "SCAN  ENABLED",
  "SYSTEM  READY",
  "NIGHT-07  ONLINE",
  "CALIBRATION  OK",
  "SIGNAL  STABLE",
  "NO  ERRORS",
];
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 15.5 },
  { name: "thumb-drag", caption: "Thumb drag", x: 270.5 },
  { name: "at-end", caption: "At end", x: 516.5 },
  // Beyond the sheet row: no reference.
  { name: "thumb-hover", caption: "Thumb hover", x: 770.5 },
] as const;

const view = (x: number): Rect => [x, 12, ...FRAME];

/** Each column's frame rectangle, by column name. */
export const VIEWS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [name, view(x)]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;
const cell = (x: number): Rect => [x - 10, 0, 246, 120];
const CONTENT = ENTRIES.length * ROW;
/** Offset of the thumb-drag column: the third entry at the top. */
const DRAGGED = 2 * ROW;

export default defineSpecimen({
  extent: EXTENT,
  theme: "scroll",
  reference: { image: "a06-scroll-view.png", origin: [352, 0] },
  states: [
    { name: "idle", cell: cell(COLUMNS[0].x) },
    {
      name: "thumb-drag",
      cell: cell(COLUMNS[1].x),
      pin: [
        {
          kind: "action",
          control: "thumb-drag",
          action: { kind: "scrollTo", offset: [0, DRAGGED] },
        },
        {
          kind: "press",
          at: thumbCentre(view(COLUMNS[1].x), CONTENT, DRAGGED),
        },
      ],
    },
    {
      name: "at-end",
      cell: cell(COLUMNS[2].x),
      pin: [
        {
          kind: "action",
          control: "at-end",
          action: { kind: "scrollTo", offset: [0, CONTENT - VIEWPORT_HEIGHT] },
        },
      ],
    },
    {
      name: "thumb-hover",
      cell: cell(COLUMNS[3].x),
      pin: [{ kind: "hover", at: thumbCentre(view(COLUMNS[3].x), CONTENT, 0) }],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {COLUMNS.map(({ name, x }) => (
        <ScrollFrame
          key={name}
          id={`scroll-${name}`}
          at={[x, 12]}
          content={
            <Entity id={`scroll-${name}/content`}>
              {/* The column fits its rows; the view clips and scrolls it. */}
              <Layout kind={2} width={ROW_WIDTH} />
              <Children>
                {ENTRIES.map((entry, index) => (
                  <Entity key={entry} id={`scroll-${name}/row-${index}`}>
                    {scrollRow({
                      id: `scroll-${name}/row-${index}`,
                      text: entry,
                      font: lab.font,
                      last: index === ENTRIES.length - 1,
                    })}
                  </Entity>
                ))}
              </Children>
            </Entity>
          }
        >
          {lab.skin("default")}
          {/* The body type the design language's lengths are drawn at. */}
          <Font source={lab.font} font_size={TEXT_BODY} />
          <ScrollView ref={lab.control(name)} />
        </ScrollFrame>
      ))}
      {COLUMNS.map(({ name, caption, x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[x + 1, 99]}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
