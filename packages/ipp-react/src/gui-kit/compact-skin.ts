import {
  compactControlRows,
  compactControlMetrics,
  compactMetrics,
} from "./sizing.js";
import type { GuiKitContract, GuiKitTokens } from "./kit.js";
import {
  kitThemes,
  type GuiKitRecipeInputs,
  type KitTheme,
  type KitThemeName,
} from "./themes.js";
import type { GuiSkinDefinition, GuiSkinTheme } from "../gui/skin-types.js";

/** Compile native control and composite recipes against the receiving World's contract. */
export function compactSkin(
  contract: GuiKitContract,
  tokens: GuiKitTokens,
  recipes: GuiKitRecipeInputs["looks"],
): GuiSkinDefinition {
  const looks = Object.fromEntries(
    Object.entries(recipes).map(([name, look]) => [
      name,
      {
        ...look,
        parts:
          name === "checkbox" || name === "switch"
            ? compactControlRows(contract, name, look.parts)
            : look.parts,
        metrics: compactControlMetrics(name),
      },
    ]),
  ) as unknown as GuiKitRecipeInputs["looks"];
  const kit = kitThemes({ ...contract, looks, tokens });
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
      metrics: recipe.metrics ?? compactMetrics,
      motion: recipe.motion ?? looks.button!.motion,
    };
  return {
    tokens,
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
