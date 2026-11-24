import { cartoonAssets } from "./assets.js";
import { cartoonNoticeIcons } from "./notice-icons.js";
import { cartoonComposites } from "./composites.js";
import { cartoonTiltVariants } from "./tilt.js";
import { compactSkin } from "../gui-kit/compact-skin.js";
import type { GuiKitContract } from "../gui-kit/kit.js";
import type { GuiSkinDefinition } from "../gui/skin-types.js";
import { cartoonLooks } from "./looks.js";
import { cartoonPlots } from "./charts.js";
import { cartoonTokens } from "./palette.js";

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
    assetIds: [
      "tab-paint",
      "meter",
      "notice-icon",
      "pencil-axis",
      "chart-mark",
      "chart-crate",
      "chart-mesh",
      "chart-card",
      "chart-paper",
      "chart-base",
    ],
    assets: cartoonAssets,
    plots: cartoonPlots,
  };
}
