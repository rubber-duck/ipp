import { cartoonAssets } from "./assets.js";
import { cartoonNoticeIcons } from "./notice-icons.js";
import { cartoonComposites } from "./composites.js";
import { cartoonTiltVariants } from "./tilt.js";
import { compactSkin } from "../gui-kit/compact-skin.js";
import type { GuiKitContract } from "../gui-kit/kit.js";
import type { GuiSkinDefinition } from "../gui/skin-types.js";
import { cartoonLooks } from "./looks.js";
import { cartoonTokens, grass, pine, sky } from "./palette.js";

/** Inked contours, two-tone button faces and opaque, night-blue cards. */
export function cartoonSkin(contract: GuiKitContract): GuiSkinDefinition {
  const { primary, segment, ...recipes } = cartoonLooks(contract);
  const skin = compactSkin(contract, cartoonTokens, recipes);
  const themes = { ...skin.themes };
  themes.button = { ...themes.button!, rows: primary.parts };
  cartoonComposites(contract, skin, themes);

  const iconThemes = cartoonNoticeIcons(contract, themes);
  const themeVariants = cartoonTiltVariants(
    contract,
    skin,
    themes,
    segment.parts,
  );

  return {
    ...skin,
    themes,
    iconThemes,
    themeVariants,
    notificationAngles: [-0.014, -0.009, 0.01, 0.015],
    assetIds: ["tab-paint", "meter", "notice-icon"],
    assets: cartoonAssets,
    plots: () => {
      const frame = {
        red: sky[0],
        green: sky[1],
        blue: sky[2],
        alpha: 1,
        grid_red: pine[0],
        grid_green: pine[1],
        grid_blue: pine[2],
        grid_alpha: 0.7,
      };
      return {
        frame2d: {
          ...frame,
          highlight_red: grass[0],
          highlight_green: grass[1],
          highlight_blue: grass[2],
          highlight_alpha: 1,
        },
        frame3d: { ...frame, mark_outlines: true },
      };
    },
  };
}
