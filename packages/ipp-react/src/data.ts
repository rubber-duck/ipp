/** Headless data declarations; only committed snapshots perform Host I/O. */
import { createElement, type ReactNode } from "react";
import type {
  ClientDatasets,
  ClientAssetSource,
  DatasetColumn,
  DatasetDelta,
  DatasetOutcome,
  DatasetProducer,
  DynamicPropertyInput,
} from "@ipp/client";
import { Asset, type AssetReference } from "./assets.js";
import { componentContract } from "./components.js";

export const DATA_SOURCE_HOST_TYPE = "ipp-data-source";

export type DataSourceProps = {
  name: string;
  children?: ReactNode;
} & (
  | { ownership: "borrowed"; datasets?: never; kind?: never; schema?: never }
  | {
      ownership: "producer";
      datasets: ClientDatasets;
      kind: "buffer" | "streaming";
      /** Immutable schema; replace the declaration to change an incarnation. */
      schema: readonly DatasetColumn[];
    }
);

/** Producer removal releases ownership; root unmount preserves every source. */
export function DataSource(props: DataSourceProps) {
  return createElement<DataSourceProps>(DATA_SOURCE_HOST_TYPE, props);
}

export interface DataSourceHandle {
  readonly name: string;
  readonly producer: DatasetProducer;
  /** Uses the existing ordered, bounded dataset lane; inspect partial outcomes. */
  update(deltas: readonly DatasetDelta[]): Promise<DatasetOutcome>;
}

export interface ReactDataSourceState {
  readonly name: string;
  readonly ownership: "borrowed" | "producer";
  readonly status: "borrowed" | "preparing" | "ready" | "failed";
  readonly handle?: DataSourceHandle;
  readonly error?: Error;
}

export interface DataSourceDescription {
  readonly identity: number;
  readonly props: DataSourceProps;
  readonly signature: string;
}

/** The generated ExpressionBuilder.encode() supplies the immutable definition. */
export interface ColumnBindingAssetProps {
  id: string;
  definition: Uint8Array<ArrayBuffer>;
}

const expressionBytes = (bytes: Uint8Array<ArrayBuffer>) => bytes;

export function ColumnBindingAsset({
  id,
  definition,
}: ColumnBindingAssetProps) {
  return createElement(Asset<Uint8Array<ArrayBuffer>>, {
    id,
    kind: 19,
    data: definition,
    encode: expressionBytes,
  });
}

export interface DataColumnBinding {
  readonly definition: AssetReference | ClientAssetSource;
  /** Animatable typed <column>_parameter; null removes it, omission preserves it. */
  readonly parameter?: DynamicPropertyInput | null;
}

export interface BufferDataSourceBindingProps {
  source: string;
  /** null removes that output and its parameter; omission preserves stored properties. */
  columns: Readonly<Record<string, DataColumnBinding | null>>;
  children?: ReactNode;
}

/** Exact generated window declaration shape; encoding stays target supplied. */
export type DataWindow =
  | { kind: "count"; count: bigint }
  | {
      kind: "range";
      column: string;
      width: number;
      anchor:
        | { kind: "latest" }
        | { kind: "hostTime"; unitsPerSecond: number }
        | {
            kind: "supplied";
            value: { kind: "f32" | "i32" | "u32"; value: number };
          };
    };

export interface StreamingDataSourceBindingProps
  extends BufferDataSourceBindingProps {
  windows?: readonly DataWindow[];
  encodeWindows: (windows: readonly DataWindow[]) => Uint8Array<ArrayBuffer>;
}

function columnsProperties(columns: BufferDataSourceBindingProps["columns"]) {
  const properties: Record<
    string,
    DynamicPropertyInput | AssetReference | null
  > = {};
  for (const [name, column] of Object.entries(columns)) {
    if (!/^[A-Za-z_][A-Za-z_0-9]*$/.test(name) || name.endsWith("_parameter"))
      throw new Error(`Invalid data column binding name: ${name}`);
    if (column === null) {
      properties[name] = null;
      properties[`${name}_parameter`] = null;
      continue;
    }
    properties[name] =
      "assetId" in column.definition
        ? column.definition
        : { kind: "asset", value: column.definition };
    if (column.parameter !== undefined)
      properties[`${name}_parameter`] = column.parameter;
  }
  return properties;
}

export function BufferDataSourceBinding({
  source,
  columns,
  children,
}: BufferDataSourceBindingProps) {
  return createElement(componentContract.BufferDataSourceBinding.host, {
    source,
    properties: columnsProperties(columns),
    children,
  });
}

export function StreamingDataSourceBinding({
  source,
  columns,
  windows = [],
  encodeWindows,
  children,
}: StreamingDataSourceBindingProps) {
  return createElement(componentContract.StreamingDataSourceBinding.host, {
    source,
    windows: encodeWindows(windows),
    properties: columnsProperties(columns),
    children,
  });
}
