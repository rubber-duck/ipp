/**
 * Sheet i, status badges, drawn with the GUI kit's `StatusBadge`: the
 * sheet's Online, Syncing, Offline and Error, each at the sheet's left edge
 * and centred on its row, and a degraded warning, which the sheet omits,
 * below them where the sheet has its caption.
 *
 * Drawn at the feedback sheet's scale through a nested `GuiKit`.
 */
import {
  GuiKit,
  StatusBadge,
  type StatusBadgeStatus,
} from "@ipp/react/gui-kit";
import { Fill, SHEET_PAGE, placed } from "../kit.js";
import { SHEET_I_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { SMALL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

/** The i06 crop is 2x of sheet i (796, 786) to (1508, 918); a row added below. */
const EXTENT = [712, 140] as const;

const HEIGHT = SMALL_HEIGHT * K;

/** Each badge's left edge and vertical centre on the sheet. */
const BADGES: readonly {
  readonly status: StatusBadgeStatus;
  readonly label: string;
  readonly x: number;
  readonly y: number;
}[] = [
  { status: "active", label: "Online", x: 13, y: 38 },
  { status: "busy", label: "Syncing", x: 184, y: 38 },
  { status: "inactive", label: "Offline", x: 356.5, y: 38 },
  { status: "error", label: "Error", x: 544, y: 38 },
  { status: "warning", label: "Degraded", x: 13, y: 104 },
];

const cell = (index: number): Rect => {
  const { x, y } = BADGES[index]!;
  return [Math.max(x - 8, 0), y - HEIGHT / 2 - 8, 160, HEIGHT + 16];
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "i06-status-badges.png", origin: [0, 0] },
  states: BADGES.map(({ status }, index) => ({
    name: status,
    cell: cell(index),
  })),
  render: () => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        {BADGES.map(({ status, label, x, y }) => (
          <StatusBadge
            key={status}
            id={`badge-${status}`}
            status={status}
            label={label}
            layout={placed([x, y - HEIGHT / 2])}
          />
        ))}
      </GuiKit>
    </>
  ),
});
