import type { ClientAssetSource } from "@ipp/client";
import {
  CanvasWorld,
  Children,
  Entity,
  Surface,
  SurfaceCache,
  Transform,
  type CanvasWorldHandle,
} from "@ipp/react";
import {
  Box,
  Button,
  Checkbox,
  Drawing,
  Font,
  Image,
  Layout,
  ScrollView,
  Skin,
  Slider,
  Style,
  Text,
  TextInput,
  Theme,
  VirtualList,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { GUI_STRESS_WORKLOAD } from "./workload.js";

export interface GuiStressAssets {
  readonly font: ClientAssetSource;
  readonly drawing: ClientAssetSource;
  readonly panel: ClientAssetSource;
  readonly bitmap: ClientAssetSource;
}

export interface GuiStressSceneProps {
  readonly assets: GuiStressAssets;
  readonly panels: number;
  readonly treeRows: number;
  readonly revision: number;
  readonly layoutEpoch: number;
  readonly themeIndex: number;
  readonly churnEpoch: number;
  readonly callbackRevision: number;
  readonly themes: readonly Uint8Array<ArrayBuffer>[];
  readonly firstButton: { current: GuiControlHandle | null };
  readonly firstList: { current: GuiControlHandle | null };
  readonly onPress: (revision: number) => void;
  readonly onRange: (panel: number, first: number, last: number) => void;
  readonly onWorld: (
    panel: number,
    raw: boolean,
    handle: CanvasWorldHandle,
  ) => void;
}

const PANEL_WIDTH = GUI_STRESS_WORKLOAD.layout.panelWidth;
const PANEL_HEIGHT = GUI_STRESS_WORKLOAD.layout.panelHeight;

export const GUI_STRESS_COLORS = [
  { base: [0.025, 0.09, 0.16, 1], accent: [0.24, 0.82, 0.97, 1] },
  { base: [0.16, 0.055, 0.035, 1], accent: [1, 0.68, 0.25, 1] },
] as const;

const SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
];

function position(panel: number, count: number): readonly [number, number] {
  const columns = Math.ceil(Math.sqrt(count));
  const rows = Math.ceil(count / columns);
  return [
    ((panel % columns) - (columns - 1) / 2) * GUI_STRESS_WORKLOAD.layout.stepX,
    ((rows - 1) / 2 - Math.floor(panel / columns)) *
      GUI_STRESS_WORKLOAD.layout.stepY,
  ];
}

function Panel({
  panel,
  props,
}: {
  panel: number;
  props: GuiStressSceneProps;
}) {
  const [positionX, positionY] = position(panel, props.panels);
  const palette =
    GUI_STRESS_COLORS[props.themeIndex % GUI_STRESS_COLORS.length]!;
  const rows = Array.from({ length: props.treeRows }, (_, index) => index);
  const visibleRows = rows.filter(
    (index) => props.churnEpoch % 2 === 0 || index % 4 !== 0,
  );
  const highlighted =
    ((panel === 0 ? props.revision : 0) + panel) % Math.max(props.treeRows, 1);
  return (
    <>
      <Entity id={`stress-panel-${panel}`}>
        <Transform x={positionX} y={positionY} />
        <Surface width={PANEL_WIDTH} height={PANEL_HEIGHT} />
      </Entity>
      <CanvasWorld
        presentation={{ anchor: `stress-panel-${panel}` }}
        create={{
          symbolicId: `stress-panel-world-${panel}`,
          selectedSystems: SYSTEMS,
        }}
        extent={[PANEL_WIDTH, PANEL_HEIGHT]}
        unitsPerMetre={1}
        onReady={(handle) => props.onWorld(panel, false, handle)}
      >
        <Entity id="stress-theme">
          <Theme
            parts={props.themes[props.themeIndex % props.themes.length]!}
          />
        </Entity>
        <Entity id="canvas">
          <Font source={props.assets.font.source} font_size={0.1} />
          <Layout kind={3} width={PANEL_WIDTH} height={PANEL_HEIGHT} />
          <Children>
            <Entity id="background">
              <Layout width={PANEL_WIDTH} height={PANEL_HEIGHT} />
              <Style
                red={palette.base[0]}
                green={palette.base[1]}
                blue={palette.base[2]}
              />
              <Box width={PANEL_WIDTH} height={PANEL_HEIGHT} />
            </Entity>
            <Entity id="content">
              <Layout
                kind={2}
                width={PANEL_WIDTH}
                height={PANEL_HEIGHT}
                padding_top={0.07}
                padding_right={0.08}
                padding_bottom={0.06}
                padding_left={0.08}
              />
              <Children>
                <Entity id="header">
                  <Layout kind={1} width={2.04} height={0.23} />
                  <Children>
                    <Entity id="title">
                      <Layout width={1.79} />
                      <Style
                        red={palette.accent[0]}
                        green={palette.accent[1]}
                        blue={palette.accent[2]}
                      />
                      <Text
                        text={`P${panel.toString().padStart(2, "0")} / REV ${panel === 0 ? props.revision : 0}`}
                        source={props.assets.font.source}
                        font_size={0.16}
                      />
                    </Entity>
                    <Entity id="icon">
                      <Layout width={0.2} height={0.2} />
                      <Style scale_x={0.006} scale_y={0.006} />
                      <Drawing source={props.assets.drawing.source} />
                    </Entity>
                  </Children>
                </Entity>
                <Entity id="controls">
                  <Layout kind={1} width={2.04} height={0.27} />
                  <Children>
                    <Entity id="arm">
                      <Layout width={0.57} height={0.24} />
                      <Skin theme="stress-theme" />
                      <Button
                        {...(panel === 0 ? { ref: props.firstButton } : {})}
                        label="ARM"
                        onPress={() => props.onPress(props.callbackRevision)}
                      />
                    </Entity>
                    <Entity id="checkbox">
                      <Layout width={0.3} height={0.24} />
                      <Skin theme="stress-theme" />
                      <Checkbox checked={panel % 2 === 0} />
                    </Entity>
                    <Entity id="slider">
                      <Layout width={0.63} height={0.24} />
                      <Skin theme="stress-theme" />
                      <Slider
                        value={0.25 + (panel % 3) * 0.2}
                        min={0}
                        max={1}
                      />
                    </Entity>
                    <Entity id="image">
                      <Layout width={0.16} height={0.16} />
                      <Image
                        source={props.assets.bitmap.source}
                        width={0.16}
                        height={0.16}
                      />
                    </Entity>
                  </Children>
                </Entity>
                <Entity id="operator">
                  <Layout width={2.04} height={0.24} />
                  <Font source={props.assets.font.source} font_size={0.14} />
                  <Skin theme="stress-theme" />
                  <TextInput
                    text={`operator-${panel}`}
                    placeholder="operator"
                  />
                </Entity>
                <Entity id="virtual-list">
                  <Layout width={2.04} height={0.28} />
                  <Skin theme="stress-theme" />
                  <VirtualList
                    {...(panel === 0 ? { ref: props.firstList } : {})}
                    item_count={GUI_STRESS_WORKLOAD.virtualItemsPerPanel}
                    item_extent={0.17}
                    overscan={2}
                    onRangeChange={({ first, last }) =>
                      props.onRange(panel, first, last)
                    }
                    renderItem={(index) => (
                      <>
                        <Layout
                          width={1.9}
                          height={index % 3 === 0 ? 0.21 : 0.17}
                        />
                        <Style
                          red={palette.accent[0]}
                          green={palette.accent[1]}
                          blue={palette.accent[2]}
                        />
                        <Text
                          text={`EVENT ${index.toString().padStart(5, "0")}`}
                          source={props.assets.font.source}
                          font_size={0.12}
                        />
                      </>
                    )}
                  />
                </Entity>
                <Entity id="scroll">
                  <Layout
                    width={2.04}
                    height={0.36 + (props.layoutEpoch % 2) * 0.03}
                  />
                  <Skin theme="stress-theme" />
                  <ScrollView />
                  <Children>
                    <Entity id="explicit-rows">
                      <Layout kind={2} width={1.96} />
                      <Children>
                        {visibleRows.map((index) => {
                          const color =
                            index === highlighted
                              ? palette.accent
                              : palette.base;
                          return (
                            <Entity key={index} id={`row-${index}`}>
                              <Layout kind={1} width={1.96} height={0.18} />
                              <Children>
                                <Entity id={`indicator-${index}`}>
                                  <Layout kind={3} width={0.1} height={0.1} />
                                  <Style
                                    red={color[0]}
                                    green={color[1]}
                                    blue={color[2]}
                                  />
                                  <Box width={0.1} height={0.1} />
                                </Entity>
                                <Entity id={`row-label-${index}`}>
                                  <Layout width={1.82} height={0.18} />
                                  <Style red={0.8} green={0.9} blue={1} />
                                  <Text
                                    text={`ROW ${index.toString().padStart(3, "0")} / ${((index * 17 + GUI_STRESS_WORKLOAD.seed) >>> 0).toString(16)}`}
                                    source={props.assets.font.source}
                                    font_size={0.12}
                                  />
                                </Entity>
                              </Children>
                            </Entity>
                          );
                        })}
                      </Children>
                    </Entity>
                  </Children>
                </Entity>
              </Children>
            </Entity>
          </Children>
        </Entity>
      </CanvasWorld>
      <Entity id={`stress-raw-${panel}`}>
        <Transform
          x={positionX}
          y={positionY + GUI_STRESS_WORKLOAD.layout.rawYOffset}
        />
        <SurfaceCache
          direct_distance={0}
          texels_per_metre={128}
          max_refresh_hz={240}
        />
        <Surface
          width={PANEL_WIDTH}
          height={GUI_STRESS_WORKLOAD.layout.rawHeight}
        />
      </Entity>
      <CanvasWorld
        presentation={{ anchor: `stress-raw-${panel}` }}
        create={{
          symbolicId: `stress-raw-world-${panel}`,
          selectedSystems: SYSTEMS,
        }}
        extent={[PANEL_WIDTH, GUI_STRESS_WORKLOAD.layout.rawHeight]}
        unitsPerMetre={1}
        onReady={(handle) => props.onWorld(panel, true, handle)}
      >
        <Entity id="canvas">
          <Children>
            <Entity id="bar">
              <Style
                x={1.1}
                y={0.09}
                scale_x={PANEL_WIDTH}
                scale_y={0.18}
                red={palette.accent[0]}
                green={palette.accent[1]}
                blue={palette.accent[2]}
                alpha={0.7}
              />
              <Drawing source={props.assets.panel.source} />
            </Entity>
            <Entity id="caption">
              <Style x={0.08} y={0.04} />
              <Text
                text={`RAW ${panel}`}
                source={props.assets.font.source}
                font_size={0.1}
              />
            </Entity>
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  );
}

export function GuiStressScene(props: GuiStressSceneProps) {
  return Array.from({ length: props.panels }, (_, panel) => (
    <Panel key={panel} panel={panel} props={props} />
  ));
}
