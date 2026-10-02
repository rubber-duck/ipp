/**
 * Sheet i, inline alerts, drawn with the GUI kit's `InlineAlert`: the
 * sheet's information and warning alerts, then an error, which the sheet
 * omits, with a Retry action at rest and hovered.
 *
 * Drawn at the feedback sheet's scale through a nested `GuiKit`, so every
 * kit length is its design size times `K`; placements follow the sheet.
 */
import { GuiKit, InlineAlert } from "@ipp/react/gui-kit";
import { FONT_ADVANCE, Fill, SHEET_PAGE, placed } from "../kit.js";
import { SHEET_I_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  INSET,
  SMALL_HEIGHT,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";

/** The i05 crop is 2x of sheet i (796, 630) to (1508, 782); two rows added below. */
const EXTENT = [712, 300] as const;

/** The alerts' left edge and width, and each alert's top, on the sheet. */
const LEFT = 12;
const WIDTH = 687;
const TOPS = [10.5, 83.5, 156.5, 229.5] as const;
const HEIGHT = CONTROL_HEIGHT * K;

/** The centre of row `row`'s action: a small secondary button at its end. */
function action(row: number, label: string): Point {
  const margin = ((CONTROL_HEIGHT - SMALL_HEIGHT) / 2) * K;
  const width = (label.length * FONT_ADVANCE * TEXT_SMALL + 2 * INSET) * K;
  return [LEFT + WIDTH - margin - width / 2, TOPS[row]! + HEIGHT / 2];
}

/** A state's cell: its alert with room for the action's glow. */
const cell = (row: number): Rect => [0, TOPS[row]! - 8, EXTENT[0], HEIGHT + 16];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "i05-inline-alerts.png", origin: [0, 0] },
  states: [
    { name: "information", cell: cell(0) },
    { name: "warning", cell: cell(1) },
    { name: "error", cell: cell(2) },
    {
      name: "action-hover",
      cell: cell(3),
      pin: [{ kind: "hover", at: action(3, "Retry") }],
    },
  ],
  render: () => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        <InlineAlert
          id="information"
          severity="information"
          text="Changes apply to this panel."
          layout={placed([LEFT, TOPS[0]], WIDTH)}
        />
        <InlineAlert
          id="warning"
          severity="warning"
          text="Connection lost. Reconnect to continue."
          layout={placed([LEFT, TOPS[1]], WIDTH)}
        />
        {[2, 3].map((row) => (
          <InlineAlert
            key={row}
            id={`error-${row}`}
            severity="error"
            text="Upload failed."
            action={{ label: "Retry" }}
            layout={placed([LEFT, TOPS[row]!], WIDTH)}
          />
        ))}
      </GuiKit>
    </>
  ),
});
