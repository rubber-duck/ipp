/**
 * Sheet i, toasts, drawn with the GUI kit's `ToastStack`: the sheet's saved
 * scene and failed upload, in the error colour with its octagon and a Retry
 * action rather than the sheet's amber triangle; then states the sheet
 * omits, a warning toast hovered and an information toast with an Undo
 * action hovered. Every toast here is persistent, so the captures do not
 * race the toasts' time.
 *
 * The stack is a manual overlay at the top of the canvas, inset from its
 * right edge: declared inside an entity but outside its `Children`, it is a
 * root of the canvas, as an application declares it at its top level. Drawn
 * at the feedback sheet's scale through a nested `GuiKit`.
 */
import { Entity } from "@ipp/react";
import { GuiKit, ToastStack, type ToastItem } from "@ipp/react/gui-kit";
import {
  FONT_ADVANCE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
} from "../kit.js";
import { SHEET_I_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DOCKED_WIDTH,
  INSET,
  LINE,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";

/** The i04 crop is 2x of sheet i; two toasts added below the sheet's. */
const EXTENT = [720, 470] as const;

const u = (value: number) => value * K;

const TOASTS: readonly ToastItem[] = [
  {
    key: "saved",
    severity: "success",
    text: "Scene saved",
    persistent: true,
  },
  {
    key: "upload",
    severity: "error",
    text: "Upload failed",
    action: { label: "Retry" },
  },
  {
    key: "unstable",
    severity: "warning",
    text: "Connection unstable",
    persistent: true,
  },
  {
    key: "removed",
    text: "Marker removed",
    action: { label: "Undo" },
  },
];

/** The stack's right edge and its toasts' height and pitch, in canvas units. */
const RIGHT = EXTENT[0] - u(INSET);
const WIDTH = u(480);
const HEIGHT = u(CONTROL_HEIGHT + INSET);
const PITCH = HEIGHT + u(INSET);
const top = (index: number) => u(INSET) + index * PITCH;

/** A state's cell: its toast with room for the hover glow. */
const cell = (index: number): Rect => [
  0,
  top(index) - 10,
  EXTENT[0],
  HEIGHT + 20,
];

/** The centre of an action button labelled `label`, before the divider and close button. */
function action(index: number, label: string): Point {
  const gap = u(INSET / 2);
  const width = label.length * FONT_ADVANCE * u(TEXT_SMALL) + u(2 * INSET);
  return [
    RIGHT - gap - u(DOCKED_WIDTH) - gap - u(LINE) - gap - width / 2,
    top(index) + HEIGHT / 2,
  ];
}

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "i04-toasts.png", origin: [0, 0] },
  states: [
    { name: "success", cell: cell(0) },
    { name: "error", cell: cell(1) },
    {
      name: "body-hover",
      cell: cell(2),
      pin: [
        { kind: "hover", at: [RIGHT - WIDTH + u(200), top(2) + HEIGHT / 2] },
      ],
    },
    {
      name: "action-hover",
      cell: cell(3),
      pin: [{ kind: "hover", at: action(3, "Undo") }],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {["No focus stealing.", "Errors persist until dismissed."].map(
        (text, index) => (
          <Label
            key={text}
            id={`caption-${index}`}
            at={[u(INSET), top(TOASTS.length) + index * 22]}
            text={text}
            font={lab.font}
            size={12.5 * K}
            color={SHEET_CAPTION}
          />
        ),
      )}
      <Entity id="overlays">
        <GuiKit fontSize={TEXT_BODY * K}>
          <ToastStack id="toasts" toasts={TOASTS} side="top" />
        </GuiKit>
      </Entity>
    </>
  ),
});
