import { useMemo } from "react";
import {
  CanvasWorld,
  Children,
  Entity,
  FlatSurface,
  LookAt,
  Transform,
} from "@ipp/react";
import type { CanvasWorldHandle } from "@ipp/react";
import { SkinProvider } from "@ipp/react/gui";
import { Panel, type GuiKitContract } from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GallerySkin } from "../../shared/skin-preference.js";
import {
  PlatformerMovementControls,
  type PlatformerMovement,
} from "./controls.js";

/** Clears the standing mannequin's head, including the panel's half-height. */
export const MOVEMENT_HEIGHT = 2.75;
export const MOVEMENT_SURFACE = "platformer-movement-surface";
export const MOVEMENT_EXTENT = [420, 160] as const;
export const MOVEMENT_DENSITY = 180;

const PANEL_WORLD = {
  selectedSystems: [
    "ipp.lifecycle-publisher",
    "ipp.animation",
    "ipp.asset-dependencies",
    "ipp.hierarchy",
    "ipp.gui",
    "ipp.gui-layout",
    "ipp.canvas",
  ],
};

/** Root travel and camera-facing orientation are evaluated by native Systems. */
export function PlatformerMovementPanel({
  camera,
  contract,
  font,
  skin,
  movement,
  onReady,
}: {
  readonly camera: bigint;
  readonly contract: GuiKitContract;
  readonly font: string;
  readonly skin: GallerySkin;
  readonly movement: PlatformerMovement;
  readonly onReady: (handle: CanvasWorldHandle) => void;
}) {
  const skins = useMemo(
    () => ({ glass: glassSkin(contract), cybertron: cybertronSkin(contract) }),
    [contract],
  );
  return (
    <>
      <Entity bindTo="platformer-root">
        <Children>
          <Entity id="platformer-movement-facing">
            <Transform x={0} y={MOVEMENT_HEIGHT} z={0} />
            <LookAt target={camera} />
            <Children>
              <Entity id={MOVEMENT_SURFACE}>
                {/* LookAt aims -Z; Surface's readable front is +Z. */}
                <Transform qy={1} qw={0} />
                <FlatSurface
                  width={MOVEMENT_EXTENT[0] / MOVEMENT_DENSITY}
                  height={MOVEMENT_EXTENT[1] / MOVEMENT_DENSITY}
                />
              </Entity>
            </Children>
          </Entity>
        </Children>
      </Entity>
      <CanvasWorld
        presentation={{ anchor: MOVEMENT_SURFACE }}
        create={PANEL_WORLD}
        extent={MOVEMENT_EXTENT}
        unitsPerMetre={MOVEMENT_DENSITY}
        onReady={onReady}
        onError={movement.report}
      >
        <SkinProvider
          id="platformer-movement-skin"
          contract={contract}
          font={font}
          fontSize={16}
          skin={skins[skin]}
        >
          <Panel
            id="platformer-movement-panel"
            width={MOVEMENT_EXTENT[0]}
            height={MOVEMENT_EXTENT[1]}
          >
            <PlatformerMovementControls movement={movement} />
          </Panel>
        </SkinProvider>
      </CanvasWorld>
    </>
  );
}
