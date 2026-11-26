import type { DatasetValue } from "@ipp/client";
import {
  assetRef,
  BufferDataSourceBinding,
  ColumnBindingAsset,
  DataSource,
  Entity,
  PlotBars2d,
  PlotFrame2d,
  PlotLine2d,
  PlotPie2d,
  type PlotSeries,
} from "@ipp/react";
import { Style } from "@ipp/react/gui";
import type * as Generated from "@ipp/host-contract";

export const chartIds = [
  "straight-line",
  "smooth-line",
  "markers",
  "area",
  "band",
  "bars",
  "pie",
] as const;
export type GuiChartId = (typeof chartIds)[number];

export const chartSchema = [
  { name: "x", kind: "f32" },
  { name: "y", kind: "f32" },
  { name: "lower", kind: "f32" },
  { name: "color", kind: "vec4" },
] as const;
export function chartRows(count: number): DatasetValue[][] {
  return Array.from({ length: count }, (_, i) => {
    const x = i / (count - 1);
    const y = 0.58 + 0.19 * Math.sin(x * 13) + 0.1 * Math.cos(x * 29);
    return [
      { kind: "f32", value: x },
      { kind: "f32", value: y },
      { kind: "f32", value: y - 0.18 },
      {
        kind: "vec4",
        value: [
          0.15 + (0.75 * (i % 3)) / 2,
          0.84 - (0.5 * (i % 5)) / 4,
          0.45 + (0.5 * (i % 7)) / 6,
          1,
        ],
      },
    ];
  });
}
export function BoardChart({
  id,
  x,
  y,
  width,
  height,
  contract,
  source,
}: {
  id: GuiChartId;
  x: number;
  y: number;
  width: number;
  height: number;
  contract: typeof Generated;
  source: string;
}) {
  const columns = Object.fromEntries(
    chartSchema.map(({ name }) => [
      name,
      { definition: assetRef("board-column-" + name) },
    ]),
  );
  const series: PlotSeries = {
    name: "Signal",
    x: "x",
    y: "y",
    value: "y",
    color: [0.16, 0.88, 0.99, 1],
    ...(id === "pie" ? { color_column: "color" } : {}),
    ...(id === "band" ? { lower: "lower" } : {}),
  };
  const props = {
    contract,
    series: { nextSlot: 1, rows: new Map([[0, series]]) },
  };
  return (
    <DataSource ownership="borrowed" name={source}>
      <Entity id={"chart-" + id}>
        <Style x={x} y={y} />
        <BufferDataSourceBinding source={source} columns={columns} />
        <PlotFrame2d
          width={width}
          height={height}
          padding_left={18}
          padding_right={6}
          padding_bottom={16}
          padding_top={4}
          min_x={0}
          max_x={1}
          min_y={0}
          max_y={1}
          automatic_x={false}
          automatic_y={false}
          ticks={3}
          font_size={8}
          source={assetRef("board-font")}
        />
        {id === "bars" ? (
          <PlotBars2d {...props} gap={0.22} />
        ) : id === "pie" ? (
          <PlotPie2d {...props} />
        ) : (
          <PlotLine2d
            {...props}
            interpolation={
              id === "smooth-line" || id === "band" ? "smooth" : "straight"
            }
            line_width={1.5}
            marker_size={id === "markers" ? 4 : 0}
            fill_opacity={id === "area" || id === "band" ? 0.5 : 0}
          />
        )}
      </Entity>
    </DataSource>
  );
}
export function ChartColumns({ contract }: { contract: typeof Generated }) {
  return (
    <>
      {chartSchema.map(({ name, kind }) => {
        const builder = new contract.ExpressionBuilder();
        return (
          <ColumnBindingAsset
            key={name}
            id={"board-column-" + name}
            definition={builder.encode(builder.input("column:" + name, kind))}
          />
        );
      })}
    </>
  );
}
