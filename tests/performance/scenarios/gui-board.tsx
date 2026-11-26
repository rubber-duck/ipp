import type { GuiPhysicalContext } from "@ipp/client";
import type * as Generated from "@ipp/host-contract";
import {
  Animation,
  AnimationAsset,
  Asset,
  assetRef,
  Children,
  Entity,
  Resources,
} from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import { glassSkin } from "@ipp/react/gui-glass";
import { cartoonSkin } from "@ipp/react/gui-cartoon";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GallerySkin } from "../../../examples/demos/shared/skin-preference.js";
import type { GuiSkinDefinition } from "@ipp/react/gui-kit";
import { Column, TextLine, GuiKit } from "@ipp/react/gui-kit";
import { controls, modalBody, type GuiSpecimen } from "./gui-controls.js";
import {
  BoardChart,
  chartIds,
  ChartColumns,
  type GuiChartId,
} from "./gui-charts.js";

import { chartCaseName, families, modals } from "./gui-cases.js";

export const boardSkins = {
  glass: glassSkin,
  cybertron: cybertronSkin,
  cartoon: cartoonSkin,
} satisfies Record<GallerySkin, unknown>;

const pitch = 224;
const caption = 20;
const margin = 8;
const groups = [
  ...new Set(controls.filter((s) => s.family !== "paint").map((s) => s.family)),
];
const width = groups.length * pitch;
const lanes = groups.map((family, i) => {
  let y = 12;
  return controls
    .filter((c) => c.family === family)
    .map((c) => {
      const slot = {
        ...c,
        x: i * pitch + margin,
        y,
        width: pitch - 2 * margin,
      };
      y += caption + c.height + 12;
      return slot;
    });
});
const chartTop =
  Math.max(...lanes.flat().map((s) => s.y + caption + s.height)) + 24;
const plots: GuiSpecimen[] = chartIds.map((id) => ({
  id,
  family: "charts",
  height: 140,
  body: null,
}));
export const slots = [
  ...lanes.flat(),
  ...[...plots, ...controls.filter((s) => s.family === "paint")].map(
    (s, i) => ({
      ...s,
      x: i * pitch + margin,
      y: chartTop,
      width: pitch - 2 * margin,
    }),
  ),
];
export const viewport = {
  width,
  height: chartTop + caption + 158,
  devicePixelRatio: 1,
};
export const fiducials = [
  [12, 4],
  [width - 12, 4],
  [12, viewport.height - 12],
  [width - 12, viewport.height - 12],
] as const;
export function selectSlots(selection: string) {
  const base = chartCaseName(selection);
  const name =
    base === "area-1k" || base === "band-1k" ? base.slice(0, -3) : base;
  if (name === "data-grid-scroll")
    return slots.filter((s) => s.id === "data-grid");
  if (name === "all") return slots;
  if (name === "blank" || modals.includes(name)) return [];
  const exclude = name.startsWith("without-");
  const key = exclude ? name.slice(8) : name;
  if (!slots.some((s) => s.id === key || s.family === key))
    throw new Error(
      "Unknown GUI case " +
        name +
        ". Choose all, blank, " +
        [...families, ...slots.map((s) => s.id), ...modals].join(", ") +
        " or without-<widget/family>.",
    );
  return slots.filter((s) => (s.id === key || s.family === key) !== exclude);
}

function bitmap() {
  const bytes = new Uint8Array(16 + 16 * 16 * 4);
  bytes.set([73, 80, 80, 84]);
  const header = new DataView(bytes.buffer);
  [3, 16, 16].forEach((n, i) => header.setUint32(4 + i * 4, n, true));
  for (let y = 0; y < 16; y++)
    for (let x = 0; x < 16; x++)
      bytes.set(
        [24 + x * 12, 80 + y * 8, ((x >> 2) ^ (y >> 2)) % 2 ? 225 : 96, 255],
        16 + 4 * (y * 16 + x),
      );
  return bytes;
}

export function GuiBoard({
  name,
  contract,
  font,
  wave,
  sources,
  input,
  skin,
}: {
  name: string;
  contract: typeof Generated;
  font: Uint8Array<ArrayBuffer>;
  wave: Uint8Array<ArrayBuffer>;
  input?: GuiPhysicalContext;
  skin: GuiSkinDefinition;
  sources: ReadonlyMap<string, string>;
}) {
  const selected = selectSlots(name);
  return (
    <>
      <Resources>
        <Asset
          id="board-font"
          kind={17}
          data={font}
          encode={(bytes) => bytes}
        />
        <Asset
          id="board-wave"
          kind={18}
          data={wave}
          encode={(bytes) => bytes}
        />
        <Asset id="board-image" kind={2} data={1} encode={bitmap} />
        <AnimationAsset
          id="redraw"
          clip={{
            duration: 1,
            tracks: [
              {
                property: {
                  component: contract.CanvasStyle.id,
                  offsets: [contract.CanvasStyle.fields.x.offset],
                },
                keys: [
                  {
                    time: 0,
                    value: { kind: "f32", value: 0 },
                    interpolation: { kind: "linear" },
                  },
                  {
                    time: 1,
                    value: { kind: "f32", value: viewport.width - 10 },
                  },
                ],
              },
            ],
          }}
        />
        {sources.size > 0 && <ChartColumns contract={contract} />}
      </Resources>
      <Entity id="background">
        <Style red={0.016} green={0.024} blue={0.042} />
        <Box width={viewport.width} height={viewport.height} />
      </Entity>
      <GuiKit
        skin={skin}
        contract={contract}
        font={assetRef("board-font")}
        fontSize={10}
        reducedMotion
      >
        <Entity id="layout">
          <Layout kind={3} width={viewport.width} height={viewport.height} />
          <Children>
            {selected.map((s) => (
              <Column
                key={s.id}
                id={"cell-" + s.id}
                width={s.width}
                height={caption + s.height}
                alignX="start"
                alignY="start"
                layout={{
                  clip: true,
                  margin_left: s.x,
                  margin_top: s.y,
                  align_x: -1,
                  align_y: -1,
                }}
              >
                <TextLine
                  text={s.id.toUpperCase()}
                  height={caption}
                  tone="accent"
                />
                {s.body}
              </Column>
            ))}
            {modals.includes(name) && (
              <Column
                width={236}
                layout={{
                  margin_left: margin,
                  margin_top: 40,
                  align_x: -1,
                  align_y: -1,
                }}
              >
                {modalBody(name, input)}
              </Column>
            )}
          </Children>
        </Entity>
        {selected
          .filter((s) => s.family === "charts")
          .map((s) => (
            <BoardChart
              key={s.id}
              id={s.id as GuiChartId}
              contract={contract}
              source={sources.get(s.id)!}
              x={s.x}
              y={s.y + caption}
              width={s.width}
              height={s.height}
            />
          ))}
      </GuiKit>
      {fiducials.map(([x, y]) => (
        <Entity key={x + "-" + y}>
          <Style x={x - 2} y={y - 2} red={1} green={0} blue={1} />
          <Box width={5} height={5} />
        </Entity>
      ))}
      <Entity id="redraw-marker">
        <Style x={0} y={viewport.height - 4} red={1} green={1} blue={1} />
        <Box width={8} height={3} />
        <Animation source={assetRef("redraw")} looping playing />
      </Entity>
    </>
  );
}
export const bodyTop = caption;
