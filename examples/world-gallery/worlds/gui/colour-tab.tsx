/**
 * The settings PROJECTION COLOUR panel: the projection's colour in the kit's colour
 * picker, its field, hue rail and swatch, the R, G and B channels, the
 * presets and the hex entry. The projector's light, lens, beam, dust and lit
 * trim and the scope's sweep band take the colour; choosing an ACCENT sets it
 * to the accent's colour, which the picker writes to its control.
 *
 * The labeled settings panel gives the picker its normal body-size font
 * and enough padded space for the field, channels and entry rows.
 */
import type { GuiHsva } from "@ipp/react/gui";
import { ColorPicker, parseHex } from "@ipp/react/gui-kit";
import { ACCENT_HSV } from "./projector.js";
import type { GuiScene } from "./scene.js";
import { useStoreValue } from "./store.js";

export const COLOUR_PICKER = "gui-colour";

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

export function ColourTab({ scene }: { readonly scene: GuiScene }) {
  const tuning = scene.tuning;
  const projection = useStoreValue(scene.state, (state) => state.tuning.color);
  const color: GuiHsva = { ...projection, alpha: 1 };
  return (
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
  );
}
