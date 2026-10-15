import { compactControlRows } from "../gui-kit/sizing.js";
import type { GuiKitContract } from "../gui-kit/kit.js";
import {
  kitThemes,
  type KitTheme,
  type GuiKitRecipeInputs,
  type KitThemeName,
} from "../gui-kit/themes.js";
import type { GuiSkinDefinition, GuiSkinTheme } from "../gui/skin-types.js";
import { cybertronLooks } from "./looks.js";
import { cybertronTokens } from "./tokens.js";
import { cybertronControlMetrics, cybertronMetrics } from "./metrics.js";

/** Explicit client-owned Cybertron recipes, encoded against the receiving runtime. */
export function cybertronSkin(contract: GuiKitContract): GuiSkinDefinition {
  const looks = Object.fromEntries(
    Object.entries(cybertronLooks(contract)).map(([name, look]) => [
      name,
      {
        ...look,
        parts:
          name === "checkbox" || name === "switch"
            ? compactControlRows(contract, name, look.parts)
            : look.parts,
        metrics: cybertronControlMetrics(name),
      },
    ]),
  ) as unknown as GuiKitRecipeInputs["looks"];
  const kit = kitThemes({ ...contract, looks, tokens: cybertronTokens });
  const themes: Record<string, GuiSkinTheme> = Object.fromEntries(
    Object.entries(looks).map(([name, look]) => [
      name,
      {
        em: look.em,
        rows: look.parts,
        motion: look.motion,
        metrics: look.metrics,
      },
    ]),
  );
  for (const [name, recipe] of Object.entries(kit) as [string, KitTheme][])
    themes[`kit/${name}`] = {
      ...recipe,
      metrics: recipe.metrics ?? cybertronMetrics,
      motion: recipe.motion ?? looks.button!.motion,
    };
  return {
    tokens: cybertronTokens,
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
    themes,
    controls: {
      button: "button",
      checkbox: "checkbox",
      switch: "switch",
      textInput: "textInput",
      slider: "slider",
      dial: "dial",
      scroll: "scroll",
      color: "color",
    },
    kit: Object.fromEntries(
      Object.keys(kit).map((name) => [name, `kit/${name}`]),
    ) as Record<KitThemeName, string>,
  };
}
