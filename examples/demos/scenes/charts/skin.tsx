import { useSyncExternalStore, type ReactNode } from "react";
import { Entity, LookAt, Transform, PlotSkinProvider } from "@ipp/react";
import type { GuiKitContract, GuiSkinDefinition } from "@ipp/react/gui-kit";
import { cartoonSkin } from "@ipp/react/gui-cartoon";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { type GallerySkin } from "../../shared/skin-preference.js";

/** One application choice, observed independently by each chart's World. */
export class ChartSkinSelection {
  private value: GallerySkin = "glass";
  private readonly listeners = new Set<() => void>();
  readonly skins: Record<GallerySkin, GuiSkinDefinition>;

  constructor(readonly contract: GuiKitContract) {
    this.skins = {
      glass: glassSkin(contract),
      cartoon: cartoonSkin(contract),
      cybertron: cybertronSkin(contract),
    };
  }

  readonly read = () => this.value;
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  select(value: GallerySkin) {
    this.value = value;
    for (const listener of this.listeners) listener();
  }
}

export function ChartSkin({
  selection,
  id,
  children,
}: {
  selection: ChartSkinSelection;
  id: string;
  children?: ReactNode;
}) {
  const name = useSyncExternalStore(selection.subscribe, selection.read);
  return (
    <PlotSkinProvider id={`${id}/skin`} skin={selection.skins[name]}>
      {children}
    </PlotSkinProvider>
  );
}

/** Ordinary native aiming keeps spatial legends readable while orbiting. */
export function ChartLegendFacing({
  selection,
  facing,
  surface,
  camera,
  depth,
  children,
}: {
  selection: ChartSkinSelection;
  facing: string;
  surface: string;
  camera: bigint;
  depth: number;
  children?: ReactNode;
}) {
  const skin = useSyncExternalStore(selection.subscribe, selection.read);
  const aim = skin === "glass" || skin === "cartoon";
  return (
    <>
      <Entity bindTo={facing}>
        <Transform z={aim ? depth * 0.5 : depth} />
        <LookAt target={camera} enabled={aim} />
      </Entity>
      <Entity bindTo={surface}>
        <Transform
          qy={aim ? 1 : 0}
          qw={aim ? 0 : 1}
          sx={skin === "glass" ? 0.88 : 1}
          sy={skin === "glass" ? 0.88 : 1}
        />
        {children}
      </Entity>
    </>
  );
}
