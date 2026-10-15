import type { GuiKitControlName } from "../gui-kit/kit.js";
import type { KitThemeName } from "../gui-kit/themes.js";
import {
  glassKitTheme,
  type GlassKitRecipes,
  type GlassKitTheme,
} from "./kit-theme.js";
import { glassStateRows } from "./rows.js";
import type { GlassPalette } from "./types.js";

/** Parent sheets provide one continuous material behind flat rows and headers. */
export function containerKitThemes(
  contract: GlassKitRecipes,
  palette: GlassPalette,
  base: Record<KitThemeName, GlassKitTheme>,
): Partial<Record<KitThemeName, GlassKitTheme>> {
  const result: Partial<Record<KitThemeName, GlassKitTheme>> = {};
  for (const name of [
    "container",
    "containerUnlit",
    "expanderBody",
    "expanderBodyDisabled",
    "gridFrame",
  ] as const)
    result[name] = glassKitTheme(contract, palette, base[name], {
      body: [4, 0, 1, 0],
    });
  for (const name of [
    "expanderHeader",
    "gridRow",
    "dockedIcon",
    "dockedIconAmber",
    "secondaryIcon",
    "secondaryIconAmber",
  ] as const)
    result[name] = glassKitTheme(contract, palette, base[name], {
      body: [9, 0, 1, name === "expanderHeader" ? 5 : 0],
      accent: name.endsWith("Amber") ? palette.warning : palette.primary,
    });
  result.gridBody = scrollTheme(contract, palette, base.gridBody, false);
  result.cellEditor = glassKitTheme(contract, palette, base.cellEditor);
  result.cellFocus = glassKitTheme(contract, palette, base.cellFocus, {
    body: [9, 0, 1, 0],
    rows: [
      {
        part: contract.guiPaintPartIndex({ part: "background" }),
        shape: 0,
        paint_data: [0, 0, 0, 4],
      },
    ],
  });
  return result;
}

function scrollTheme(
  contract: GlassKitRecipes,
  palette: GlassPalette,
  base: GlassKitTheme,
  frame = true,
): GlassKitTheme {
  const tracks = ["scrollTrackX", "scrollTrackY"],
    thumbs = ["scrollThumbX", "scrollThumbY"];
  return glassKitTheme(contract, palette, base, {
    parts: [...(frame ? ["background"] : []), ...tracks, ...thumbs],
    body: [4, 0, 1, 0],
    rows: [
      ...tracks.flatMap((part) =>
        glassStateRows(contract, palette, part, { shape: 0 }),
      ),
      ...thumbs.flatMap((part) =>
        glassStateRows(contract, palette, part, { shape: 0 }).map((row) => {
          const data = row.paint_data as number[];
          return {
            ...row,
            paint_data: [data[0], data[1], data[2], 2],
            ...(data[2] ? { opacity: 0 } : {}),
          };
        }),
      ),
      ...(!frame
        ? glassStateRows(contract, palette, "background", {
            paint: false,
            color: [0, 0, 0, 0],
          })
        : []),
    ],
  });
}

export function containerKitControls(
  contract: GlassKitRecipes,
  palette: GlassPalette,
): Partial<Record<GuiKitControlName, GlassKitTheme>> {
  const look = contract.looks.scroll!;
  return {
    scroll: scrollTheme(contract, palette, {
      rows: look.parts,
      em: look.em,
      metrics: look.metrics,
      motion: look.motion,
    }),
  };
}
