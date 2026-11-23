import type { GuiKitContract } from "../gui-kit/kit.js";
import { compactSkin } from "../gui-kit/compact-skin.js";
import type { GuiSkinDefinition } from "../gui/skin-types.js";
import { cybertronLooks } from "./looks.js";
import { cybertronTokens } from "./tokens.js";
/** Explicit client-owned Cybertron recipes, encoded against the receiving runtime. */
export function cybertronSkin(contract: GuiKitContract): GuiSkinDefinition {
  return {
    ...compactSkin(contract, cybertronTokens, cybertronLooks(contract)),
    // Reassert the original chart frame after another skin is removed. Native
    // declaration fields deliberately retain their value when a prop is omitted.
    plots: () => {
      const frame = {
        red: 0.45,
        green: 0.8,
        blue: 1,
        alpha: 1,
        grid_red: 0.04,
        grid_green: 0.18,
        grid_blue: 0.24,
        grid_alpha: 0.7,
      };
      return {
        frame2d: {
          ...frame,
          highlight_red: 1,
          highlight_green: 1,
          highlight_blue: 1,
          highlight_alpha: 1,
        },
        frame3d: { ...frame, mark_outlines: true },
      };
    },
  };
}
