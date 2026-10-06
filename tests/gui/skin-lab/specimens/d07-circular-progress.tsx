/**
 * Sheet d, circular progress, drawn with the GUI kit's `CircularProgress`:
 * the sheet's large 65% upload, its empty and its completed small rings,
 * centred where the sheet has them (the completed one lower, below the empty
 * one's caption), and below the crop the states the sheet omits: a large
 * failed upload and a small unknown total. Running rings are drawn as reduced
 * motion shows them, the leading arc steady at its rest and the unknown
 * total's arc still, since motion never settles for a capture.
 *
 * The kit puts every caption under its ring, where the sheet puts the small
 * rings' words beside them. Drawn at the value sheet's scale through a nested
 * `GuiKit`, which also holds the rings' motion still without changing the
 * World's preference.
 */
import {
  CircularProgress,
  GuiKit,
  type CircularProgressProps,
} from "@ipp/react/gui-kit";
import { FONT_ADVANCE, Fill, SHEET_PAGE, around, placed } from "../kit.js";
import { SHEET_D_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { DENSE_ROW, INSET, TEXT_BODY } from "../themes/geometry.js";

/** The d07 crop is 2x of sheet d (1022, 501) to (1522, 937); two rings added below. */
const EXTENT = [500, 700] as const;

/** The kit's ring diameters and the space above the caption. */
const DIAMETER = { large: 128, small: 64 } as const;
const CAPTION_GAP = 8;

const RINGS: readonly {
  readonly name: string;
  /** The ring's centre on the sheet. */
  readonly at: readonly [number, number];
  readonly ring: Omit<CircularProgressProps, "id" | "layout">;
}[] = [
  {
    name: "determinate",
    at: [136, 235],
    ring: { label: "Uploading", value: 0.65 },
  },
  {
    name: "empty",
    at: [348.5, 173.5],
    ring: { label: "Uploading", value: 0, size: "small" },
  },
  {
    // Below the sheet's ring, clear of the empty ring's caption.
    name: "complete",
    at: [348.5, 330],
    ring: { label: "Uploading", status: "complete", size: "small" },
  },
  {
    name: "failed",
    at: [136, 545],
    ring: { label: "Uploading", value: 0.4, status: "failed" },
  },
  {
    name: "unknown",
    at: [360, 520],
    ring: { label: "Scanning", size: "small" },
  },
];

const OUTCOMES = { complete: "100%", failed: "Failed" } as const;

/** A component's outer rectangle: as wide as its ring or caption, centred on the ring. */
function bounds({ at, ring }: (typeof RINGS)[number]): Rect {
  const text = (value: string) => [...value].length * FONT_ADVANCE * TEXT_BODY;
  const diameter = DIAMETER[ring.size ?? "large"];
  const caption =
    text(ring.label) +
    (ring.status ? INSET / 2 + text(OUTCOMES[ring.status]) : 0);
  const width = Math.max(diameter, caption) * K;
  return [
    at[0] - width / 2,
    at[1] - (diameter * K) / 2,
    width,
    (diameter + CAPTION_GAP + DENSE_ROW) * K,
  ];
}

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d07-circular-progress.png", origin: [0, 0] },
  states: RINGS.map((ring) => ({
    name: ring.name,
    cell: around(bounds(ring), 8),
  })),
  render: () => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K} reducedMotion>
        {RINGS.map((entry) => {
          const [x, y] = bounds(entry);
          return (
            <CircularProgress
              key={entry.name}
              id={`ring-${entry.name}`}
              {...entry.ring}
              layout={placed([x, y])}
            />
          );
        })}
      </GuiKit>
    </>
  ),
});
