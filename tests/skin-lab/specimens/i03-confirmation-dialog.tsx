/**
 * Sheet i, the confirmation dialog, drawn with the GUI kit's
 * `ConfirmationDialog`, open: a modal dialog centred on the canvas whose
 * opening moved focus to Cancel, with the ring since nothing held focus,
 * beside Delete at rest, its quiet amber line without glow. One modal
 * dialog leaves no region for a second state, whose cell would come from
 * another capture; Tab moving focus to Delete inside the dialog is a case of
 * the composites scenario.
 *
 * The dialog is a root of the canvas: declared inside an entity but outside
 * its `Children`. Its header strip draws the title in the accent and its
 * close button docked, Cancel is a secondary button and Delete a primary
 * one in the amber variant. Drawn at the feedback sheet's scale through a
 * nested `GuiKit`.
 */
import { Entity } from "@ipp/react";
import { ConfirmationDialog, GuiKit } from "@ipp/react/gui-kit";
import { Fill, Label, SHEET_CAPTION, SHEET_PAGE } from "../kit.js";
import { SHEET_I_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DENSE_ROW,
  INSET,
  LINE,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The i03 crop is 2x of sheet i. */
const EXTENT = [738, 346] as const;

const u = (value: number) => value * K;

const BODY = [
  "Delete Cube from the scene?",
  "This action cannot be undone.",
] as const;

/** The dialog: 368 wide, its header, division, body and buttons. */
const WIDTH = u(368 + 2 * LINE);
const HEIGHT = u(
  CONTROL_HEIGHT +
    LINE +
    3 * INSET +
    BODY.length * DENSE_ROW +
    CONTROL_HEIGHT +
    INSET +
    2 * LINE,
);
const DIALOG: Rect = [
  (EXTENT[0] - WIDTH) / 2,
  (EXTENT[1] - HEIGHT) / 2,
  WIDTH,
  HEIGHT,
];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "i03-confirmation-dialog.png", origin: [0, 0] },
  states: [
    {
      name: "dialog",
      cell: [DIALOG[0] - 16, DIALOG[1] - 16, WIDTH + 32, HEIGHT + 32],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <Label
        id="caption"
        at={[18, 316]}
        text="Escape cancels / restore focus."
        font={lab.font}
        size={u(12.5)}
        color={SHEET_CAPTION}
      />
      <Entity id="overlays">
        <GuiKit fontSize={u(TEXT_BODY)}>
          <ConfirmationDialog
            id="confirm"
            open
            title="Delete node?"
            body={BODY}
            action="Delete"
          />
        </GuiKit>
      </Entity>
    </>
  ),
});
