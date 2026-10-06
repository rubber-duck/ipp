/**
 * Sheet h, tabs, drawn with the GUI kit's `Tabs`: the sheet's strip that
 * fits, Signals selected over its content, and its overflowing strip, whose
 * scroll buttons dock at its ends; then, below the crop, the overflowing
 * strip with keyboard focus moved from the selected Signals to Events, which
 * the runtime scrolled into view, so selection and focus show apart, a
 * hovered tab of the strip that fits, and an overflowing strip whose More
 * button a click has opened, listing every tab.
 *
 * The kit's tabs hug their labels from the strip's start, where the sheet
 * spreads them over its width, and the strip is the control height. The
 * kit's More is a docked chevron button rather than the sheet's labelled
 * one, and its menu marks no tab: the strip shows the selected one. Drawn at
 * the navigation sheet's scale through a nested `GuiKit`.
 */
import { GuiKit, Tabs, TextLine, type TabItem } from "@ipp/react/gui-kit";
import {
  FONT_ADVANCE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
  placed,
} from "../kit.js";
import { SHEET_H_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DOCKED_WIDTH,
  INSET,
  LINE,
  ROW,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The h01 crop is 2x of sheet h (14, 112) to (1522, 548); its first 1024 units, strips added below. */
const EXTENT = [1024, 1020] as const;

const u = (value: number) => value * K;
const STRIP = u(CONTROL_HEIGHT);
/** The sheet's strip and content area. */
const HEIGHT = 164;

const FITS = ["Overview", "Signals", "Events"];
const MANY = [...FITS, "Assets", "Diagnostics", "Settings"];

/** Strip `strip`'s tabs, each with one line of content. */
const tabs = (strip: string, labels: readonly string[]): TabItem[] =>
  labels.map((label) => {
    const id = `${strip}/content-${label.toLowerCase()}`;
    return {
      value: label.toLowerCase(),
      label,
      content: <TextLine id={id} text={`${label} content`} />,
    };
  });

/**
 * Each strip's top-left and width, in tree and so Tab order: the sheet's two
 * where it has them, the overflowing one narrower than the sheet's so that it
 * ends inside the canvas.
 */
const STRIPS = {
  fits: { at: [22.5, 145], width: 605, tabs: FITS },
  focused: { at: [695, 470], width: 317, tabs: MANY },
  overflow: { at: [695, 188], width: 317, tabs: MANY },
  hover: { at: [22.5, 470], width: 605, tabs: FITS },
  more: { at: [22.5, 670], width: 317, tabs: MANY.slice(0, 5) },
} as const satisfies Record<
  string,
  { at: Point; width: number; tabs: readonly string[] }
>;
type Strip = keyof typeof STRIPS;

const rect = (strip: Strip): Rect => [
  ...STRIPS[strip].at,
  STRIPS[strip].width,
  HEIGHT,
];

/** A tab's width: its label and the content inset either side. */
const tabWidth = (label: string) =>
  [...label].length * FONT_ADVANCE * u(TEXT_BODY) +
  u(TEXT_BODY) / 100 +
  u(2 * INSET);

/** The centre of tab `index` of `strip`, before any scrolling. */
function tabCentre(strip: Strip, index: number): Point {
  const { at, tabs } = STRIPS[strip];
  const scrolls = strip !== "fits" && strip !== "hover";
  let x = at[0] + (scrolls ? u(DOCKED_WIDTH) : 0);
  for (const label of tabs.slice(0, index)) x += tabWidth(label);
  return [x + tabWidth(tabs[index]!) / 2, at[1] + STRIP / 2];
}

const CAPTIONS: Readonly<Partial<Record<Strip, string>>> = {
  focused: "Arrow keys move focus | Enter activates",
  hover: "Hover",
};

/** The More menu's height: its rows and the menu's margins and frame. */
const MENU = u(INSET + STRIPS.more.tabs.length * ROW + 2 * LINE);

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "h01-tabs.png", origin: [0, 0] },
  states: [
    { name: "fits", cell: around(rect("fits"), 12) },
    { name: "overflow", cell: around(rect("overflow"), 12) },
    {
      // Tab enters each strip at its selected Signals, the first strip
      // being the one that fits; Right moves the ring to Events, which the
      // runtime scrolls into view, and selects nothing.
      name: "focused",
      cell: around(rect("focused"), 12),
      pin: [
        { kind: "key", key: "tab" },
        { kind: "key", key: "tab" },
        { kind: "key", key: "right" },
      ],
    },
    {
      name: "hover",
      cell: around(rect("hover"), 12),
      pin: [{ kind: "hover", at: tabCentre("hover", 2) }],
    },
    {
      // A click on More, docked at the strip's end, opens its menu below it.
      name: "more",
      cell: around(
        [STRIPS.more.at[0], STRIPS.more.at[1], STRIPS.more.width, STRIP + MENU],
        12,
      ),
      pin: [
        {
          kind: "click",
          at: [
            STRIPS.more.at[0] + STRIPS.more.width - u(DOCKED_WIDTH) / 2,
            STRIPS.more.at[1] + STRIP / 2,
          ],
        },
        { kind: "wait", seconds: 0.25 },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(STRIPS) as Strip[]).map((strip) => (
          <Tabs
            key={strip}
            id={`tabs-${strip}`}
            tabs={tabs(strip, STRIPS[strip].tabs)}
            defaultValue="signals"
            overflowMenu={strip === "more"}
            layout={{
              ...placed(STRIPS[strip].at, STRIPS[strip].width),
              height: HEIGHT,
            }}
          />
        ))}
      </GuiKit>
      <Label
        id="caption-more"
        at={[
          STRIPS.more.at[0] + STRIPS.more.width + 24,
          STRIPS.more.at[1] + 12,
        ]}
        text="More reveals hidden tabs"
        font={lab.font}
        size={u(12.5)}
        color={SHEET_CAPTION}
      />
      {(Object.keys(CAPTIONS) as Strip[]).map((strip) => (
        <Label
          key={strip}
          id={`caption-${strip}`}
          at={[STRIPS[strip].at[0], STRIPS[strip].at[1] + HEIGHT + 8]}
          text={CAPTIONS[strip]!}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
