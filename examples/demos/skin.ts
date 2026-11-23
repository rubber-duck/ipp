import type { GuiKitContract } from "@ipp/react/gui-kit";
import { cartoonSkin } from "@ipp/react/gui-cartoon";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";

/** Application chrome needs a quiet backing over every scene's moving content. */
export function demoSkins(contract: GuiKitContract) {
  return {
    glass: glassSkin(contract, {
      appearance: "liquid",
      panelOpacity: 0.98,
      reflectionStrength: 0,
    }),
    cartoon: cartoonSkin(contract),
    cybertron: cybertronSkin(contract),
  };
}
