/**
 * The workbench's COLOUR tab: the projection's colour in the kit's colour
 * picker, its field, hue rail and swatch, the R, G and B channels, the
 * presets and the hex entry. The projector's light, lens, beam, dust and lit
 * trim and the scope's sweep band take the colour; choosing an ACCENT sets it
 * to the accent's colour, which the picker writes to its control.
 *
 * The picker is a fixed-size composite wider than the workbench at the
 * dashboard's body size, so it draws in a nested kit at three quarters of
 * it, which keeps its proportions.
 */
import type { GuiHsva } from "@ipp/react/gui";
import { ColorPicker, GuiKit, parseHex } from "@ipp/react/gui-kit";
import { BODY } from "./presentation.js";
import { ACCENT_HSV } from "./projector.js";
import type { GuiSceneState } from "./scene.js";

export const COLOUR_PICKER = "gui-colour";

/** The picker's body size: three quarters of the dashboard's. */
const PICKER_BODY = (BODY * 3) / 4;

/** Colours the picker offers: the two accents' and three more. */
const PRESETS = [
  { value: { ...ACCENT_HSV.cyan, alpha: 1 }, label: "Cyan" },
  { value: { ...ACCENT_HSV.amber, alpha: 1 }, label: "Amber" },
  ...[
    { hex: "#F4449F", label: "Magenta" },
    { hex: "#59FF8A", label: "Green" },
    { hex: "#F2F6FF", label: "White" },
  ].map(({ hex, label }) => ({ value: parseHex(hex)!, label })),
];

export function ColourTab({ scene }: { readonly scene: GuiSceneState }) {
  const tuning = scene.tuning;
  const color: GuiHsva = { ...tuning.tuning.color, alpha: 1 };
  return (
    <GuiKit fontSize={PICKER_BODY}>
      <ColorPicker
        id={COLOUR_PICKER}
        label="PROJECTION"
        alpha={false}
        value={color}
        presets={PRESETS}
        onChange={(next) =>
          tuning.setColor({
            hue: next.hue,
            saturation: next.saturation,
            value: next.value,
          })
        }
      />
    </GuiKit>
  );
}
