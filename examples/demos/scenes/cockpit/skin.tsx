import { useMemo, type ReactNode } from "react";
import { GuiKit, type GuiKitContract } from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GalleryAssets } from "../../shared/scene.js";
import type { CockpitOptions } from "./options.js";
import { DEMO_ICON_FONT, demoFont } from "../../shared/fonts.js";

export function CockpitSkin({
  contract,
  assets,
  state,
  panelOpacity = state.panelOpacity,
  children,
}: {
  contract: GuiKitContract;
  assets: GalleryAssets;
  state: CockpitOptions;
  panelOpacity?: number;
  children: ReactNode;
}) {
  const skins = useMemo(
    () => cockpitSkins(contract, panelOpacity),
    [contract, panelOpacity],
  );
  return (
    <GuiKit
      id="cockpit-skin"
      contract={contract}
      font={assets.url(demoFont(state.font).path)}
      iconFont={assets.url(DEMO_ICON_FONT)}
      fontSize={16}
      skin={skins[state.skin]}
    >
      {children}
    </GuiKit>
  );
}

export function cockpitSkins(contract: GuiKitContract, panelOpacity: number) {
  const cybertron = cybertronSkin(contract);
  const themes = { ...cybertron.themes };
  for (const role of ["container", "containerUnlit"] as const) {
    const name = cybertron.kit[role];
    const theme = themes[name]!;
    themes[name] = {
      ...theme,
      rows: theme.rows.map((row) => {
        const color = row.color as readonly number[] | undefined;
        return color
          ? { ...row, color: [color[0]!, color[1]!, color[2]!, panelOpacity] }
          : row;
      }),
    };
  }
  return {
    glass: glassSkin(contract, {
      appearance: "liquid",
      sceneLighting: false,
      panelOpacity,
      reflectionStrength: 0,
      material: { ior: 1.47, roughness: 0.36, thickness: 4, edge: 1 },
      palette: {
        text: "#F2F8FC",
        mutedText: "#CFDDE3",
        neutral: "#A6C1CE",
        primary: "#B4E7FF",
        surface: "#303942",
        background: "#19232D",
      },
      light: { x: 0.18, y: 0.12, height: 3.8, intensity: 0.6 },
    }),
    cybertron: { ...cybertron, themes },
  };
}
