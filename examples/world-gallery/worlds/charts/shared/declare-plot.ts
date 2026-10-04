/** Shared native/example authoring; source data stays in the Data Service. */
import { createElement as h } from "react";
import type { AnimationWorldClient, RowPropertyValue } from "@ipp/client";
import {
  BufferDataSourceBinding,
  StreamingDataSourceBinding,
  type DataWindow,
  DataSource,
  ColumnBindingAsset,
  assetRef,
  Entity,
  PlotFrame2d,
  PlotFrame3d,
  PlotLine2d,
  PlotBars2d,
  PlotPie2d,
  PlotGridBars3d,
  PlotHeightSurface3d,
  PlotPoints3d,
  PlotPie3d,
  createRoot,
  type PlotContract,
  type PlotSeries,
  type PlotLabel,
} from "@ipp/react";

type Row = Readonly<Record<string, RowPropertyValue>>;
const charts = {
  PlotLine2d,
  PlotBars2d,
  PlotPie2d,
  PlotGridBars3d,
  PlotHeightSurface3d,
  PlotPoints3d,
  PlotPie3d,
};

/** Native fixture values enter the same public typed declarations as an app. */
export async function declarePlot(
  client: AnimationWorldClient,
  contract: PlotContract,
  entity: string,
  component: keyof typeof charts,
  source: string,
  definitions: Readonly<Record<string, Uint8Array<ArrayBuffer>>>,
  series: readonly Row[],
  labels: readonly Row[],
  extra: Readonly<
    Record<string, number | boolean | string | Uint8Array<ArrayBuffer>>
  >,
  parameters: Readonly<Record<string, number>> = { y2: 1 },
  streaming?: {
    windows: readonly DataWindow[];
    encodeWindows: (windows: readonly DataWindow[]) => Uint8Array<ArrayBuffer>;
  },
) {
  const root = createRoot(client);
  const seriesRows = {
    nextSlot: series.length,
    rows: new Map(series.map((row, slot) => [slot, row as PlotSeries])),
  };
  const labelRows = {
    nextSlot: labels.length,
    rows: new Map(
      labels.map((row, slot) => [
        slot,
        {
          series: row.series as number,
          row: BigInt(row.row_id as string),
          text: row.text as string,
          offset: row.offset as readonly [number, number],
          highlighted: row.highlighted as boolean,
          connector: row.connector as boolean,
        } satisfies PlotLabel,
      ]),
    ),
  };
  const { interpolation, ...properties } = extra;
  const chart =
    component === "PlotLine2d"
      ? h(PlotLine2d, {
          ...properties,
          contract,
          series: seriesRows,
          labels: labelRows,
          interpolation: interpolation === 1 ? "smooth" : "straight",
        })
      : h(charts[component], {
          ...properties,
          contract,
          series: seriesRows,
          labels: labelRows,
        });
  const bindingProps = {
    source,
    columns: Object.fromEntries(
      Object.keys(definitions).map((output) => [
        output,
        {
          definition: assetRef(output),
          ...(parameters[output] !== undefined
            ? { parameter: { kind: "f32" as const, value: parameters[output] } }
            : {}),
        },
      ]),
    ),
  };
  try {
    await root.render(
      h(
        DataSource,
        { name: source, ownership: "borrowed" },
        ...Object.entries(definitions).map(([id, definition]) =>
          h(ColumnBindingAsset, { key: id, id, definition }),
        ),
        h(
          Entity,
          { bindTo: entity },
          h(component.endsWith("2d") ? PlotFrame2d : PlotFrame3d, {}),
          streaming
            ? h(StreamingDataSourceBinding, { ...bindingProps, ...streaming })
            : h(BufferDataSourceBinding, bindingProps),
          chart,
        ),
      ),
    );
  } catch (error) {
    await root.render(null).catch(() => {});
    await root.unmount().catch(() => {});
    throw error;
  }
  return root;
}
