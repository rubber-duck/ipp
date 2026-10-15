/** Shared, flat design-review entry point for the actual cockpit screens. */
import { Asset, assetRef } from "@ipp/react";
import { GuiKit, type GuiKitContract } from "@ipp/react/gui-kit";
import {
  CockpitPane,
  COCKPIT_SCREEN_WIDTH,
  COCKPIT_SCREEN_HEIGHT,
} from "../../../examples/demos/scenes/cockpit/panels.js";
import { cockpitSkins } from "../../../examples/demos/scenes/cockpit/skin.js";
import { cockpitOptions } from "../../../examples/demos/scenes/cockpit/options.js";
import type { FrameRateContract } from "../../../examples/demos/frame-rate-panel.js";
import type { GalleryDiagnosticsMonitor } from "../../../examples/demos/frame-rate.js";
import { useMemo, useState } from "react";

export function FlatCockpitScreen({
  font,
  contract,
  index = 0,
  skin = "glass",
  tab = "scene",
  monitor,
}: {
  font: Uint8Array<ArrayBuffer>;
  contract: FrameRateContract & GuiKitContract;
  index?: number;
  skin?: "glass" | "cybertron";
  tab?: "scene" | "gui";
  monitor?: GalleryDiagnosticsMonitor | undefined;
}) {
  const [state, setState] = useState(() => cockpitOptions({}));
  const skins = useMemo(
    () => cockpitSkins(contract, state.panelOpacity),
    [contract, state.panelOpacity],
  );
  return (
    <>
      <Asset id="screen-font" kind={17} data={font} encode={(bytes) => bytes} />
      <GuiKit
        id="screen-skin"
        contract={contract}
        skin={skins[skin]}
        font={assetRef("screen-font")}
        fontSize={16}
      >
        <CockpitPane
          key={`${index}/${tab}`}
          index={index}
          width={COCKPIT_SCREEN_WIDTH}
          height={COCKPIT_SCREEN_HEIGHT}
          state={{ ...state, skin }}
          contract={contract}
          monitor={monitor}
          initialTab={tab}
          update={(patch) =>
            setState((previous) => cockpitOptions({ ...previous, ...patch }))
          }
        />
      </GuiKit>
    </>
  );
}
