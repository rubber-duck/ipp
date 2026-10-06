/** Sheet a, row 07: VirtualList populated, scrolled, and empty. */
import { Font, VirtualList } from "@ipp/react/gui";
import {
  CAPS_HEIGHT,
  CAPS_TOP,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
} from "../kit.js";
import { FRAME, ROW } from "../scroll-geometry.js";
import { ScrollFrame, scrollRow } from "../scroll-kit.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { TEXT_BODY, TEXT_SMALL } from "../themes/geometry.js";
import { neutral } from "../themes/palette.js";

const EXTENT = [770, 124] as const;
const EVENTS = [
  "PULSE  SENT",
  "GAIN  UPDATED",
  "SCAN  ENABLED",
  "AUTOSCAN  ARMED",
  "SYSTEM  READY",
  "NIGHT-07  ONLINE",
  "CALIBRATION  OK",
  "SIGNAL  STABLE",
  "LINK  CHECKED",
  "NO  ERRORS",
  "UPLINK  IDLE",
  "BOOT  COMPLETE",
];
const COLUMNS = [
  { name: "populated", caption: "Populated", x: 15.5, count: EVENTS.length },
  { name: "scrolled", caption: "Scrolled", x: 270.5, count: EVENTS.length },
  { name: "empty", caption: "Empty", x: 516.5, count: 0 },
] as const;

const TOP = 14.5;
/** Each column's frame rectangle, by column name. */
export const LISTS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [name, [x, TOP, ...FRAME] as Rect]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;
const cell = (x: number): Rect => [x - 10, 0, 246, 124];
/** Newest first, a minute apart, as on the sheet. */
const entry = (index: number) =>
  `14:${String(32 - index).padStart(2, "0")}   ${EVENTS[index]}`;

export default defineSpecimen({
  extent: EXTENT,
  theme: "scroll",
  reference: { image: "a07-virtual-list.png", origin: [352, 0] },
  states: [
    { name: "populated", cell: cell(COLUMNS[0].x) },
    {
      name: "scrolled",
      cell: cell(COLUMNS[1].x),
      // 14:28 SYSTEM READY at the top, as on the sheet.
      pin: [
        {
          kind: "action",
          control: "scrolled",
          action: { kind: "scrollToIndex", index: 4 },
        },
      ],
    },
    { name: "empty", cell: cell(COLUMNS[2].x) },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {COLUMNS.map(({ name, x, count }) => (
        <ScrollFrame key={name} id={`list-${name}`} at={[x, TOP]}>
          {lab.skin("default")}
          {/* The body type the design language's lengths are drawn at. */}
          <Font source={lab.font} font_size={TEXT_BODY} />
          <VirtualList
            ref={lab.control(name)}
            item_count={count}
            item_extent={ROW}
            overscan={1}
            renderItem={(index) =>
              scrollRow({
                id: `list-${name}/item-${index}`,
                text: entry(index),
                font: lab.font,
                last: index === count - 1,
              })
            }
          />
        </ScrollFrame>
      ))}
      {/* Secondary text, its capitals centred in the frame. */}
      <Label
        id="empty-message"
        at={[
          COLUMNS[2].x,
          TOP + FRAME[1] / 2 - (CAPS_TOP + CAPS_HEIGHT / 2) * TEXT_SMALL,
        ]}
        width={FRAME[0]}
        centred
        text="No events"
        font={lab.font}
        size={TEXT_SMALL}
        color={neutral}
      />
      {COLUMNS.map(({ name, caption, x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[x + 1, 101]}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
