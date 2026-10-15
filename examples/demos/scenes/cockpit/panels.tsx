import { Entity } from "@ipp/react";
import type { GuiTarget } from "@ipp/client";
import { Button as GrabButton, Behavior, Layout } from "@ipp/react/gui";
import {
  Column,
  Stack,
  Panel,
  PanelHeader,
  ScrollArea,
  TextLine,
  useGuiKit,
} from "@ipp/react/gui-kit";
import type {
  GalleryAssets,
  GalleryOptions,
  GallerySceneContext,
  GallerySceneControlsProps,
} from "../../shared/scene.js";
import { SceneChoice, sceneActions } from "../../shared/control-panel.js";
import {
  InstrumentAssets,
  PulseInstrument,
  RadarInstrument,
  type InstrumentContract,
} from "./instruments.js";
import { cockpitOptions, type CockpitOptions } from "./options.js";
import {
  ControlScreen,
  type CockpitNavigationSource,
} from "./control-screen.js";
import { SystemScreen } from "./system-screen.js";
import { TerminalScreen } from "./terminal-screen.js";
import type { FrameRateContract } from "../../frame-rate-panel.js";
import type { GalleryDiagnosticsMonitor } from "../../frame-rate.js";

export const PANEL_DENSITY = 200;
export const COCKPIT_SCREEN_WIDTH = 960;
export const COCKPIT_SCREEN_HEIGHT = 800;
export const COCKPIT_SCREEN_TITLES = [
  "Bridge controls",
  "System monitor",
  "Terminal",
] as const;

/** This frame is shared with the flat native design review, without a parallel layout. */
export function CockpitPane({
  index,
  width,
  height,
  state,
  update,
  wheelTarget,
  contract,
  monitor,
  application,
  initialTab,
  navigation,
}: {
  index: number;
  width: number;
  height: number;
  state: CockpitOptions;
  update(patch: GalleryOptions): void | Promise<void>;
  wheelTarget?(role: "grip" | "content", target: GuiTarget | null): void;
  contract: FrameRateContract;
  monitor?: GalleryDiagnosticsMonitor | undefined;
  application?: GallerySceneContext["application"];
  initialTab?: "scene" | "gui" | undefined;
  navigation?: CockpitNavigationSource | undefined;
}) {
  const kit = useGuiKit();
  const inset = kit.spacing.inset;
  return (
    <Stack width={width} height={height}>
      <Entity id={`cockpit-grip-${index}`}>
        <Layout width={width} height={height} />
        <GrabButton
          skin={false}
          ref={(handle) => wheelTarget?.("grip", handle?.target ?? null)}
        />
        <Behavior focusable={false} />
      </Entity>
      <Panel
        id={`cockpit-pane-${index}`}
        width={width}
        height={height}
        padding={0}
      >
        <PanelHeader
          id={`cockpit-header-${index}`}
          title={COCKPIT_SCREEN_TITLES[index]!}
        >
          <TextLine text={`0${index + 1} / 03`} tone="neutral" size="small" />
        </PanelHeader>
        <ScrollArea
          id={`cockpit-content-${index}`}
          ref={(handle) => wheelTarget?.("content", handle?.target ?? null)}
          padding={inset}
          fitHeight={false}
          gap={0}
          layout={{ flex: 1 }}
        >
          {index === 0 ? (
            <ControlScreen
              state={state}
              update={update}
              application={application}
              initialTab={initialTab}
              navigation={navigation}
            />
          ) : index === 1 ? (
            <SystemScreen
              monitor={monitor}
              contract={contract}
              width={width - 2 * inset}
            />
          ) : (
            <TerminalScreen state={state} />
          )}
        </ScrollArea>
      </Panel>
    </Stack>
  );
}

export function DashboardInstrument({
  width,
  height,
  radar,
  contract,
  assets,
  state,
  report,
}: {
  width: number;
  height: number;
  radar: boolean;
  contract: InstrumentContract;
  assets: GalleryAssets;
  state: CockpitOptions;
  report(error: unknown): void;
}) {
  return (
    <>
      <InstrumentAssets contract={contract} />
      {radar ? (
        <RadarInstrument
          state={state}
          size={Math.min(width, height)}
          report={report}
        />
      ) : (
        <PulseInstrument
          state={state}
          assets={assets}
          contract={contract}
          width={width}
          height={height}
          report={report}
        />
      )}
    </>
  );
}

export function CockpitControls(props: GallerySceneControlsProps) {
  const state = cockpitOptions(props.mount.options);
  const { update } = sceneActions(props);
  return (
    <Column gap={12}>
      <TextLine
        text="Choose a bridge screen. Scene and display controls live together on the first screen."
        tone="neutral"
        size="small"
      />
      <SceneChoice
        id="cockpit-settings-screen"
        label="Screen"
        value={String(Math.round(state.carousel))}
        choices={COCKPIT_SCREEN_TITLES.map((label, index) => ({
          key: String(index),
          label,
        }))}
        change={(value) => update({ carousel: Number(value) })}
      />
    </Column>
  );
}
