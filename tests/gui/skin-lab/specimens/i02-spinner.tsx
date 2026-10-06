/**
 * Sheet i, the spinner, drawn with the GUI kit's `Spinner`: the sheet's
 * running spinner and its reduced-motion symbol, each ring centred where the
 * sheet has it. Reduced motion is the same symbol held still, so both show
 * the rest pose: a turning arc never settles for a capture, and the frames of
 * a turning spinner are the shared Host's evidence rather than a state here.
 *
 * Drawn at the feedback sheet's scale through a nested `GuiKit`, which also
 * holds both spinners still without changing the World's preference.
 */
import { GuiKit, Spinner } from "@ipp/react/gui-kit";
import { FONT_ADVANCE, Fill, SHEET_PAGE, around, placed } from "../kit.js";
import { SHEET_I_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { ICON, INSET, TEXT_BODY } from "../themes/geometry.js";

/** The i02 crop is 2x of sheet i (24, 400) to (744, 540). */
const EXTENT = [720, 140] as const;

/** Each ring's centre on the sheet, and the spinner's text. */
const SPINNERS = [
  { name: "spinner", label: "Preparing...", at: [71.5, 48.5] },
  { name: "reduced", label: "Reduced motion", at: [437.5, 46.5] },
] as const;

/** A spinner's outer rectangle: its ring, the gap and its text. */
function bounds({ label, at }: (typeof SPINNERS)[number]): Rect {
  const width =
    (ICON + INSET / 2 + label.length * FONT_ADVANCE * TEXT_BODY) * K;
  return [at[0] - (ICON * K) / 2, at[1] - (ICON * K) / 2, width, ICON * K];
}

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "i02-spinner.png", origin: [0, 0] },
  states: SPINNERS.map((spinner) => ({
    name: spinner.name,
    cell: around(bounds(spinner), 12),
  })),
  render: () => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K} reducedMotion>
        {SPINNERS.map((spinner) => {
          const [x, y] = bounds(spinner);
          return (
            <Spinner
              key={spinner.name}
              id={`spinner-${spinner.name}`}
              label={spinner.label}
              layout={placed([x, y])}
            />
          );
        })}
      </GuiKit>
    </>
  ),
});
