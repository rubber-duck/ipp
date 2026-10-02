/**
 * Sheet i, progress bars, drawn with the GUI kit's `ProgressBar`: the
 * sheet's 65% upload and its completed upload, then the states the sheet
 * omits: an unknown duration, a cancelled and a failed upload, and a
 * two-part and a three-part bar. Running bars are drawn as reduced motion
 * shows them, their leading section steady at its rest and the unknown
 * duration's segment centred, since motion never settles for a capture.
 *
 * Drawn at the feedback sheet's scale through a nested `GuiKit`, which also
 * holds the bars' motion still without changing the World's preference; each
 * bar's frame sits where the sheet has its frame.
 */
import { GuiKit, ProgressBar, type ProgressBarProps } from "@ipp/react/gui-kit";
import { Fill, SHEET_PAGE, placed } from "../kit.js";
import { SHEET_I_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { SMALL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

/** The i01 crop is 2x of sheet i (24, 194) to (744, 394); five bars added below. */
const EXTENT = [720, 713] as const;

/** The frames' left edge and width, and each frame's top, on the sheet. */
const LEFT = 38;
const WIDTH = 645;
const FRAME_TOPS = [46.5, 148, 249.5, 351, 452.5, 554, 655.5] as const;

/** From a bar's top to its frame's: the label row and the gap below it. */
const ABOVE_FRAME = (24 + 4) * K;

const BARS: readonly (Omit<ProgressBarProps, "id" | "layout"> & {
  readonly name: string;
})[] = [
  { name: "determinate", label: "Uploading", value: 0.65 },
  { name: "complete", label: "Uploading", status: "complete" },
  { name: "unknown", label: "Scanning" },
  { name: "cancelled", label: "Uploading", value: 0.4, status: "cancelled" },
  { name: "failed", label: "Uploading", value: 0.4, status: "failed" },
  {
    name: "two-part",
    label: "Syncing",
    segments: [{ value: 0.5 }, { value: 0.15, tone: "amber" }],
  },
  {
    name: "three-part",
    label: "Testing",
    segments: [
      { value: 0.55 },
      { value: 0.1, tone: "neutral" },
      { value: 0.08, tone: "error" },
    ],
  },
];

const cell = (index: number): Rect => [
  0,
  FRAME_TOPS[index]! - ABOVE_FRAME - 6,
  EXTENT[0],
  ABOVE_FRAME + SMALL_HEIGHT * K + 12,
];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "i01-progress-bar.png", origin: [0, 0] },
  states: BARS.map(({ name }, index) => ({ name, cell: cell(index) })),
  render: () => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K} reducedMotion>
        {BARS.map(({ name, ...bar }, index) => (
          <ProgressBar
            key={name}
            id={`bar-${name}`}
            {...bar}
            layout={placed([LEFT, FRAME_TOPS[index]! - ABOVE_FRAME], WIDTH)}
          />
        ))}
      </GuiKit>
    </>
  ),
});
