import type { ReactNode } from "react";
import { Entity, Children } from "@ipp/react";
import { Font, Layout, Style, Box } from "@ipp/react/gui";
import {
  Panel,
  PanelHeader,
  PanelFooter,
  WindowControl,
  Expander,
  DataGrid,
  TextLine,
  Row,
  SecondaryButton,
} from "@ipp/react/gui-kit";
import {
  GlassGuiKit,
  GlassScrollView,
  GlassVirtualList,
} from "@ipp/react/gui-glass";
import { resolveGlassPalette } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import type {
  GlassPaletteInput,
  GlassLight,
} from "../../../../packages/ipp-react/src/gui-glass/types.js";
import {
  defineSpecimen,
  type SpecimenContext,
} from "../../skin-lab/specimen.js";

export const CONTAINERS_EXTENT = [860, 620] as const;
export interface ContainerEvents {
  minimize: number;
  maximize: number;
  restore: number;
  close: number;
  selected: string;
  edit?: string;
  expanded?: boolean;
}
export function containerEvents(): ContainerEvents {
  return { minimize: 0, maximize: 0, restore: 0, close: 0, selected: "bravo" };
}
export interface ContainerOptions {
  palette?: Partial<GlassPaletteInput>;
  light?: Partial<GlassLight>;
  editing?: boolean;
  backdrop?: readonly [number, number, number, number];
}
function at(
  id: string,
  x: number,
  y: number,
  width: number,
  height: number,
  children: ReactNode,
) {
  return (
    <Entity id={`${id}/slot`}>
      <Layout
        kind={3}
        margin_left={x}
        margin_top={y}
        width={width}
        height={height}
        align_x={-1}
        align_y={-1}
      />
      <Children>{children}</Children>
    </Entity>
  );
}
function rows(id: string, count = 3) {
  return Array.from({ length: count }, (_, index) => (
    <Row
      key={index}
      id={`${id}/${index}`}
      height={26}
      layout={{ padding_left: 12, padding_right: 12 }}
    >
      <TextLine
        id={`${id}/${index}/name`}
        text={`CHANNEL ${index + 1}`}
        layout={{ flex: 1 }}
      />
      <TextLine id={`${id}/${index}/value`} text="READY" tone="neutral" />
    </Row>
  ));
}
export function containersSpecimen(
  contract: GlassKitContract,
  events: ContainerEvents,
  options: ContainerOptions = {},
) {
  const palette = resolveGlassPalette(options.palette);
  const background = options.backdrop ?? palette.background;
  return defineSpecimen({
    extent: CONTAINERS_EXTENT,
    states: [
      { name: "containers", cell: [0, 0, ...CONTAINERS_EXTENT] },
      {
        name: "window-hover",
        cell: [260, 20, 144, 56],
        pin: [{ kind: "hover", at: [373, 48] }],
      },
      {
        name: "expander-focus",
        cell: [430, 94, 404, 64],
        pin: [
          { kind: "action", control: "expander", action: { kind: "focus" } },
        ],
      },
    ],
    render: (lab: SpecimenContext) => (
      <GlassGuiKit
        contract={contract}
        font={lab.font}
        fontSize={16}
        reducedMotion
        {...(options.palette ? { palette: options.palette } : {})}
        {...(options.light ? { light: options.light } : {})}
      >
        <Entity id="containers/page">
          <Layout kind={3} width={860} height={620} />
          <Children>
            <Entity id="containers/background">
              <Layout kind={0} width={860} height={620} />
              <Style
                red={background[0]}
                green={background[1]}
                blue={background[2]}
                alpha={background[3]}
              />
              <Box width={860} height={620} />
            </Entity>
            {at(
              "main",
              24,
              24,
              380,
              264,
              <Panel id="main" padding={0} layout={{ width: 380, height: 264 }}>
                <PanelHeader id="main/header" title="RUNTIME PANEL">
                  {(["minimize", "maximize", "restore", "close"] as const).map(
                    (kind) => (
                      <WindowControl
                        key={kind}
                        id={`main/${kind}`}
                        kind={kind}
                        ref={lab.control(kind)}
                        onPress={() => events[kind]++}
                        layout={{ margin_left: 4 }}
                      />
                    ),
                  )}
                </PanelHeader>
                {rows("main/channel", 4)}
                <PanelFooter id="main/footer">
                  <TextLine
                    id="main/footer/state"
                    text="ONLINE / SYNCED"
                    tone="neutral"
                    layout={{ flex: 1 }}
                  />
                  <SecondaryButton
                    id="main/footer/action"
                    label="DETAILS"
                    ref={lab.control("details")}
                  />
                </PanelFooter>
              </Panel>,
            )}
            {at(
              "minimized",
              432,
              24,
              404,
              64,
              <Panel
                id="minimized"
                padding={0}
                minimized
                layout={{ width: 404 }}
              >
                <PanelHeader id="minimized/header" title="MINIMIZED WINDOW">
                  <WindowControl
                    id="minimized/restore"
                    kind="restore"
                    onPress={() => events.restore++}
                  />
                </PanelHeader>
              </Panel>,
            )}
            {at(
              "expander",
              432,
              100,
              404,
              152,
              <Expander
                id="expander"
                label="CHANNEL SETTINGS"
                summary="3 ITEMS"
                defaultExpanded
                ref={lab.control("expander")}
                onExpandedChange={(expanded) => {
                  events.expanded = expanded;
                }}
                layout={{ width: 404 }}
              >
                {rows("expander/channel")}
              </Expander>,
            )}
            {at(
              "locked",
              432,
              280,
              404,
              64,
              <Expander
                id="locked"
                label="LOCKED CONFIGURATION"
                disabled
                ref={lab.control("disabled-expander")}
                layout={{ width: 404 }}
              >
                {rows("locked/channel")}
              </Expander>,
            )}
            {at(
              "grid",
              24,
              320,
              380,
              272,
              <DataGrid
                id="grid"
                columns={[
                  { key: "name", title: "NODE", flex: 1 },
                  { key: "status", title: "STATUS", width: 96 },
                ]}
                rows={Array.from({ length: 12 }, (_, index) => ({
                  key:
                    index === 0
                      ? "alpha"
                      : index === 1
                        ? "bravo"
                        : `node-${index}`,
                  cells: { name: `NODE ${index + 1}`, status: "READY" },
                }))}
                selected={events.selected}
                focusedCell={{ row: "bravo", column: "name" }}
                {...(options.editing
                  ? { editingCell: { row: "alpha", column: "status" } }
                  : {})}
                onRowPress={(key) => {
                  events.selected = key;
                }}
                onCellEdit={(_cell, text) => {
                  events.edit = text;
                }}
                scroll
                scrollRef={lab.control("grid-scroll")}
                footer="12 NODES / LIVE"
                layout={{ width: 380, height: 272 }}
              />,
            )}
            {at(
              "scroll",
              432,
              376,
              192,
              216,
              <Entity id="scroll">
                <Layout kind={3} width={192} height={216} />
                <Font source={lab.font} font_size={16} />
                <GlassScrollView ref={lab.control("scroll")} />
                <Children>
                  <Entity id="scroll/content">
                    <Layout kind={2} width={170} />
                    <Children>{rows("scroll/row", 18)}</Children>
                  </Entity>
                </Children>
              </Entity>,
            )}
            {at(
              "virtual",
              644,
              376,
              192,
              216,
              <Entity id="virtual">
                <Layout kind={3} width={192} height={216} />
                <Font source={lab.font} font_size={16} />
                <GlassVirtualList
                  ref={lab.control("virtual")}
                  item_count={100}
                  item_extent={26}
                  renderItem={(index) => (
                    <>
                      <Layout kind={3} height={26} />
                      <Children>
                        <TextLine
                          id={`virtual/label/${index}`}
                          text={`ITEM ${String(index + 1).padStart(3, "0")}`}
                          layout={{ margin_left: 12, align_y: 0 }}
                        />
                      </Children>
                    </>
                  )}
                />
              </Entity>,
            )}
          </Children>
        </Entity>
      </GlassGuiKit>
    ),
  });
}
