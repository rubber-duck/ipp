/** Thin Plot declarations. Rust owns preparation, geometry, labels and picking. */
import { createElement } from "react";
import type {
  ComponentDescriptor,
  RowsInput,
  RowsLayoutDescriptor,
} from "@ipp/client";
import {
  componentContract,
  type ComponentFields,
  type ComponentProps,
} from "../components.js";

/** Pass the generated module belonging to the receiving Host/target. */
export interface PlotContract {
  readonly components: Readonly<Record<string, ComponentDescriptor>>;
  encodeRowsTable<Row extends object>(
    layout: RowsLayoutDescriptor,
    table: RowsInput<Row>,
  ): Uint8Array<ArrayBuffer>;
}

export interface PlotSeries {
  readonly name?: string;
  readonly x?: string;
  readonly y?: string;
  readonly z?: string;
  readonly value?: string;
  readonly radius?: string;
  readonly height?: string;
  readonly color_column?: string;
  readonly color?: readonly [number, number, number, number];
  readonly visible?: boolean;
}

/** `series` is a stable series slot; `row` is a Data Service source-row identity. */
export interface PlotLabel {
  readonly series: number;
  readonly row: bigint;
  readonly text: string;
  readonly highlighted?: boolean;
  readonly offset?: readonly [number, number];
  readonly connector?: boolean;
}

export interface PlotRowsProps {
  contract: PlotContract;
  /** Caller-owned slots and nextSlot survive reordering, removal and updates. */
  series?: RowsInput<PlotSeries>;
  labels?: RowsInput<PlotLabel>;
}

export type PlotFrame2dProps = ComponentProps & ComponentFields<"PlotFrame2d">;
export type PlotFrame3dProps = ComponentProps & ComponentFields<"PlotFrame3d">;
export function PlotFrame2d(props: PlotFrame2dProps) {
  return createElement(componentContract.PlotFrame2d.host, props);
}

export function PlotFrame3d(props: PlotFrame3dProps) {
  return createElement(componentContract.PlotFrame3d.host, props);
}

type ChartName =
  | "PlotLine2d"
  | "PlotBars2d"
  | "PlotPie2d"
  | "PlotGridBars3d"
  | "PlotHeightSurface3d"
  | "PlotPoints3d"
  | "PlotPie3d";
type PlotProps<Name extends ChartName> = ComponentProps &
  Omit<ComponentFields<Name>, "series" | "labels"> &
  PlotRowsProps;
export type PlotLine2dProps = Omit<PlotProps<"PlotLine2d">, "interpolation"> & {
  interpolation?: "straight" | "smooth";
};
export type PlotBars2dProps = PlotProps<"PlotBars2d">;
export type PlotPie2dProps = PlotProps<"PlotPie2d">;
export type PlotGridBars3dProps = PlotProps<"PlotGridBars3d">;
export type PlotHeightSurface3dProps = PlotProps<"PlotHeightSurface3d">;
export type PlotPoints3dProps = PlotProps<"PlotPoints3d">;
export type PlotPie3dProps = PlotProps<"PlotPie3d">;

/** Convert identities directly; neither BigInt rows nor slots are remapped. */
export function plotLabelRows(table: RowsInput<PlotLabel>) {
  return {
    nextSlot: table.nextSlot,
    rows: new Map(
      [...table.rows].map(([slot, label]) => {
        if (
          typeof label.row !== "bigint" ||
          label.row <= 0n ||
          label.row > 0xffffffffffffffffn
        )
          throw new RangeError("Plot label row must be a positive u64 BigInt");
        return [
          slot,
          {
            series: label.series,
            row_id: label.row.toString(10),
            text: label.text,
            highlighted: label.highlighted ?? false,
            offset: label.offset ?? [0, 0],
            connector: label.connector ?? true,
          },
        ] as const;
      }),
    ),
  };
}

function chart<Name extends ChartName>(name: Name, props: PlotProps<Name>) {
  const { contract, series, labels, ...fields } = props;
  const component = contract.components[name];
  if (!component) throw new Error(`Target does not expose ${name}`);
  const encode = <Row extends object>(field: string, table: RowsInput<Row>) => {
    const layout = component.fields[field]?.rows;
    if (!layout)
      throw new Error(`Target does not expose ${name}.${field} rows`);
    return contract.encodeRowsTable(layout, table);
  };
  return createElement(componentContract[name].host, {
    ...fields,
    ...(series === undefined
      ? {}
      : {
          series: encode("series", {
            nextSlot: series.nextSlot,
            rows: new Map(
              [...series.rows].map(([slot, item]) => [
                slot,
                {
                  name: item.name ?? "",
                  x: item.x ?? "x",
                  y: item.y ?? "y",
                  z: item.z ?? "z",
                  value: item.value ?? "value",
                  radius: item.radius ?? "",
                  height: item.height ?? "",
                  color_column: item.color_column ?? "",
                  color: item.color ?? [0, 0.8, 1, 1],
                  visible: item.visible ?? true,
                },
              ]),
            ),
          }),
        }),
    ...(labels === undefined
      ? {}
      : { labels: encode("labels", plotLabelRows(labels)) }),
  });
}

export function PlotLine2d({ interpolation, ...props }: PlotLine2dProps) {
  return chart("PlotLine2d", {
    ...props,
    ...(interpolation === undefined
      ? {}
      : { interpolation: interpolation === "smooth" ? 1 : 0 }),
  });
}

export function PlotBars2d(props: PlotBars2dProps) {
  return chart("PlotBars2d", props);
}

export function PlotPie2d(props: PlotPie2dProps) {
  return chart("PlotPie2d", props);
}

export function PlotGridBars3d(props: PlotGridBars3dProps) {
  return chart("PlotGridBars3d", props);
}

export function PlotHeightSurface3d(props: PlotHeightSurface3dProps) {
  return chart("PlotHeightSurface3d", props);
}

export function PlotPoints3d(props: PlotPoints3dProps) {
  return chart("PlotPoints3d", props);
}

export function PlotPie3d(props: PlotPie3dProps) {
  return chart("PlotPie3d", props);
}
