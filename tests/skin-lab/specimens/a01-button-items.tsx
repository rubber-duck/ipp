/**
 * Buttons as the items of composites, beside the button states of a01:
 * selected primary buttons idle, hovered, focused and disabled and a
 * selected secondary button; list rows idle, selected and not focusable; a
 * text field that keeps focus while a row that does not take focus is
 * pressed; and Tab moving focus to a row below a scroll view's viewport,
 * which scrolls it into view. The sheets show no such row; no reference.
 */
import { Children, Entity } from "@ipp/react";
import { Font, Layout, ScrollView, TextInput } from "@ipp/react/gui";
import {
  CentredButton,
  COLUMN,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
} from "../kit.js";
import { ROW_WIDTH } from "../scroll-kit.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, ROW, TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [660, 260] as const;

/** The top row: 108-wide buttons at the control height, 126 apart. */
const BUTTON_COLUMNS = [
  { name: "selected", caption: "Selected", look: "default" },
  { name: "selected-hover", caption: "Hover", look: "default" },
  { name: "selected-focus", caption: "Focus", look: "default" },
  { name: "secondary", caption: "Secondary", look: "secondary" },
  { name: "disabled", caption: "Disabled", look: "default" },
] as const;
const buttonX = (index: number) => 15 + 126 * index;
const button = (index: number): Rect => [
  buttonX(index),
  44 - CONTROL_HEIGHT / 2,
  108,
  CONTROL_HEIGHT,
];

/** List rows: 180 wide at the row height, in a column from `LIST`. */
const LIST: Point = [15, 116];
const ROWS = [
  { name: "row-idle", label: "SCAN", selected: false, focusable: true },
  { name: "row-selected", label: "GAIN", selected: true, focusable: true },
  { name: "row-option", label: "PULSE", selected: false, focusable: false },
] as const;
const row = (index: number): Rect => [LIST[0], LIST[1] + ROW * index, 180, ROW];

/** The field, and below it a row that does not take focus. */
const FIELD: Rect = [215, 116, 168, CONTROL_HEIGHT];
const OPTION: Rect = [215, 172, 168, ROW];

/**
 * A scroll view 90 tall, its rows inset 4 from the frame, over five rows
 * (180): Tab from row 0 to row 3 (108..144) scrolls the 82-tall viewport
 * 62, which shows row 3 at its bottom (46..82).
 */
const VIEW: Rect = [420, 116, 228, 90];
const VIEW_INSET = 4;
const VIEWPORT = VIEW[3] - 2 * VIEW_INSET;
const VIEW_ROWS = ["LOG 0", "LOG 1", "LOG 2", "LOG 3", "LOG 4"] as const;
/** The revealed row, which is selected so its fill shows where it lands. */
const REVEALED = 3;
const REVEAL_OFFSET = (REVEALED + 1) * ROW - VIEWPORT;

/** Rectangles the probes read, by name. */
export const ITEMS = {
  ...(Object.fromEntries(
    BUTTON_COLUMNS.map(({ name }, index) => [name, button(index)]),
  ) as Record<(typeof BUTTON_COLUMNS)[number]["name"], Rect>),
  ...(Object.fromEntries(
    ROWS.map(({ name }, index) => [name, row(index)]),
  ) as Record<(typeof ROWS)[number]["name"], Rect>),
  field: FIELD,
  option: OPTION,
  view: VIEW,
  /** Where Tab leaves the revealed row in the viewport. */
  revealed: [
    VIEW[0] + VIEW_INSET,
    VIEW[1] + VIEW_INSET + REVEALED * ROW - REVEAL_OFFSET,
    ROW_WIDTH,
    ROW,
  ] as Rect,
} as const;

const centre = ([x, y, width, height]: Rect): Point => [
  x + width / 2,
  y + height / 2,
];

export default defineSpecimen({
  extent: EXTENT,
  theme: "items",
  states: [
    ...BUTTON_COLUMNS.map(({ name }, index) => ({
      name,
      cell: [buttonX(index) - 9, 0, 126, 100] as Rect,
      ...(name === "selected-hover"
        ? { pin: [{ kind: "hover", at: centre(button(index)) }] as const }
        : name === "selected-focus"
          ? {
              pin: [
                { kind: "action", control: name, action: { kind: "focus" } },
              ] as const,
            }
          : {}),
    })),
    { name: "rows", cell: [6, 100, 196, 160] },
    {
      // Focus the field, then hold a press on the row that does not take
      // focus: the field keeps its ring and caret while the row is pressed.
      name: "keep-focus",
      cell: [202, 100, 200, 160],
      pin: [
        { kind: "click", at: [FIELD[0] + FIELD[2] - 12, FIELD[1] + 20] },
        { kind: "press", at: centre(OPTION) },
      ],
    },
    {
      // Focus row 0 with the pointer, then Tab to row 3 below the viewport.
      name: "reveal",
      cell: [402, 100, 250, 160],
      pin: [
        { kind: "click", at: [VIEW[0] + 40, VIEW[1] + VIEW_INSET + ROW / 2] },
        { kind: "key", key: "tab" },
        { kind: "key", key: "tab" },
        { kind: "key", key: "tab" },
        // The pointer leaves for the page, so the rows show no hover.
        { kind: "hover", at: [VIEW[0] + 40, 250] },
      ],
      restore: [
        {
          kind: "action",
          control: "view",
          action: { kind: "scrollTo", offset: [0, 0] },
        },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {BUTTON_COLUMNS.map(({ name, look }, index) => {
        const [x, y, width, height] = button(index);
        return (
          <CentredButton
            key={name}
            id={`button-${name}`}
            width={width}
            height={height}
            margin={{ left: x, top: y }}
            alignY={-1}
            label="PULSE"
            font={lab.font}
            size={TEXT_BODY}
            skin={lab.skin(look)}
            selected
            disabled={name === "disabled"}
            control={lab.control(name)}
          />
        );
      })}
      <Entity id="list">
        <Layout
          kind={COLUMN}
          width={180}
          height={ROW * ROWS.length}
          margin_left={LIST[0]}
          margin_top={LIST[1]}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          {ROWS.map(({ name, label, selected, focusable }) => (
            <CentredButton
              key={name}
              id={`list/${name}`}
              width={180}
              height={ROW}
              label={label}
              font={lab.font}
              size={TEXT_BODY}
              skin={lab.skin("docked")}
              selected={selected}
              focusable={focusable}
            />
          ))}
        </Children>
      </Entity>
      <Entity id="field">
        <Layout
          kind={0}
          width={FIELD[2]}
          height={FIELD[3]}
          margin_left={FIELD[0]}
          margin_top={FIELD[1]}
          align_x={-1}
          align_y={-1}
        />
        {lab.skin("default")}
        <Font source={lab.font} font_size={TEXT_BODY} />
        <TextInput text="NIGHT-07" />
      </Entity>
      <CentredButton
        id="option"
        width={OPTION[2]}
        height={OPTION[3]}
        margin={{ left: OPTION[0], top: OPTION[1] }}
        alignY={-1}
        label="PULSE"
        font={lab.font}
        size={TEXT_BODY}
        skin={lab.skin("docked")}
        focusable={false}
      />
      <Entity id="view">
        <Layout
          kind={0}
          width={VIEW[2]}
          height={VIEW[3]}
          margin_left={VIEW[0]}
          margin_top={VIEW[1]}
          align_x={-1}
          align_y={-1}
          padding_left={VIEW_INSET}
          padding_top={VIEW_INSET}
          padding_bottom={VIEW_INSET}
        />
        {lab.skin("default")}
        <Font source={lab.font} font_size={TEXT_BODY} />
        <ScrollView ref={lab.control("view")} />
        <Children>
          <Entity id="view/content">
            <Layout kind={COLUMN} width={ROW_WIDTH} />
            <Children>
              {VIEW_ROWS.map((label, index) => (
                <CentredButton
                  key={label}
                  id={`view/row-${index}`}
                  width={ROW_WIDTH}
                  height={ROW}
                  label={label}
                  font={lab.font}
                  size={TEXT_BODY}
                  skin={lab.skin("docked")}
                  selected={index === REVEALED}
                />
              ))}
            </Children>
          </Entity>
        </Children>
      </Entity>
      {[
        ...BUTTON_COLUMNS.map(({ caption }, index) => ({
          caption,
          at: [buttonX(index) + 2, 78.5] as Point,
        })),
        { caption: "Idle, selected, no focus", at: [15, 232] as Point },
        { caption: "Field keeps focus", at: [215, 232] as Point },
        { caption: "Tab reveals row 3", at: [420, 232] as Point },
      ].map(({ caption, at }, index) => (
        <Label
          key={caption}
          id={`caption-${index}`}
          at={at}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
