import type { GuiKitContract, GuiKitRow } from "../gui-kit/kit.js";
import type { GuiSkinDefinition, GuiSkinTheme } from "../gui/skin-types.js";
import { blue, cartoonTokens as t, cream, grass, ink } from "./palette.js";
import { tabAppearance } from "./tab-paint.js";

const angles = [
  [-1.55, 0.85],
  [1.25, -0.65],
  [-0.9, -0.85],
  [1.65, 0.7],
] as const;
const states = ["idle", "hovered", "pressed", "disabled"] as const;
const roomy = [2.5, 3.5, 7.5, 2.2] as const;
type Shape = readonly [number, number, number, number];
type Inset = readonly [number, number];

export function cartoonTiltVariants(
  contract: GuiKitContract,
  skin: GuiSkinDefinition,
  themes: Record<string, GuiSkinTheme>,
  segmentRows: readonly GuiKitRow[],
) {
  const key = (part: string, state?: string, variant?: string) =>
    contract.guiPaintPartIndex({
      part,
      ...(state && { state }),
      ...(variant && { variant }),
    });
  const variants: Record<string, string[]> = {};
  const paint = {
    paint: true,
    shape: 0,
    fill_mode: 0,
    border_width: 0,
    corner_radius: [0, 0],
    outset: t.tabPaintPadding,
  };
  const applySelection = (
    id: string,
    shape: Shape,
    inset: Inset = [0, 0],
    quietHover = false,
  ) => {
    const base = themes[id]!;
    const rows = new Map(base.rows.map((row) => [row.part, row]));
    for (const state of states) {
      const rest = key("background", state, "unchecked");
      const face = {
        ...rows.get(key("background")),
        ...rows.get(key("background", state)),
        ...rows.get(rest),
      };
      // Unselected faces fade in and out using the hover pigment throughout:
      // the motion sampler interpolates colour and alpha independently.
      const clear = Array.isArray(face.color) && face.color[3] === 0;
      const color = clear ? [blue[0], blue[1], blue[2], 0] : face.color;
      const hover = quietHover && (state === "hovered" || state === "pressed");
      const tint =
        face.border_color === cream ? 1 : face.border_color === t.line ? -1 : 0;
      rows.set(rest, {
        part: rest,
        ...paint,
        color: hover
          ? [blue[0], blue[1], blue[2], state === "pressed" ? 0.65 : 0.45]
          : color,
        paint_data: [
          0,
          hover ? 0 : (face.border_width ?? 0),
          tint,
          face.fill_mode === 1 ? 1 : 0,
        ],
      });
      const checked = key("background", state, "checked");
      rows.set(checked, {
        ...rows.get(checked),
        part: checked,
        ...paint,
        color: state === "disabled" ? t.line : grass,
        paint_data: [1, 1.8, 0, state === "disabled" ? 0 : 1],
      });
      const ring = key("focusRing", state, "checked");
      rows.set(ring, {
        part: ring,
        ...paint,
        color: t.amber,
        paint_data: [-1, 1.8, 0, 0],
      });
      if (state !== "disabled") {
        const label = key("label", state, "checked");
        rows.set(label, { part: label, color: ink });
      }
    }
    const parts = [...rows.values()];
    variants[id] = angles.map(([angle, skew], i) => {
      const name = i === 0 ? id : `${id}/tilt-${i}`;
      themes[name] = {
        ...base,
        rows: parts,
        paint: tabAppearance(shape, angle, skew, inset),
      };
      return name;
    });
  };
  const segment = skin.kit.segment;
  themes[segment] = { ...themes[segment]!, rows: segmentRows };
  applySelection(segment, roomy, [-4, -4], true);
  for (const role of ["segmentFirst", "segmentLast", "segmentOnly"] as const) {
    const id = skin.kit[role];
    themes[id] = themes[segment]!;
    variants[id] = variants[segment]!;
  }
  // A tab's hit slot includes side clearance for keyboard reveal and clipping.
  applySelection(
    skin.kit.tab,
    [roomy[0] - t.tabPaintPadding[0], roomy[1], roomy[2], roomy[3]],
    [-3, -2],
    true,
  );
  for (const id of [
    skin.controls.button,
    skin.kit.secondary,
    skin.kit.amber,
    skin.kit.stepperStep,
    skin.kit.stepperComplete,
  ])
    applySelection(id, roomy);
  // The radio uses the same animated face, scaled around its circular mark.
  // Its larger hit target still belongs to the native control.
  const markInset =
    ((t.choiceIndicator - t.choiceTarget) / 2) *
    (t.controlHeight / t.choiceTarget);
  const selectedInset = markInset + 2.5;
  applySelection(
    skin.kit.radio,
    [selectedInset, selectedInset, t.controlHeight, roomy[3]],
    [markInset, markInset],
  );
  // Keep the selected ink inside a packed row. Inset both endpoints by
  // the same amount so its growth and tilt match the Difficulty segments.
  for (const role of [
    "gridRow",
    "menuRow",
    "menuRowAmber",
    "optionCheckRow",
  ] as const)
    applySelection(
      skin.kit[role],
      [-0.5, -1.5, roomy[2], roomy[3]],
      [-7, -9],
      true,
    );
  return variants;
}
