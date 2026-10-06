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
import { Asset, type AssetReference } from "../assets/declarations.js";
import { componentContract } from "../components.js";

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
  /**
   * Per-lane rate per Host second. Initial/newly valid rows and source
   * or definition replacements snap; retained row IDs keep motion through windows.
   * Invalid targets publish immediately. null disables and snaps; omission preserves.
   */
  readonly interpolation?: DataColumnInterpolation | null;
}

/** Positive rates apply independently to each numeric output lane. */
export type DataColumnInterpolation =
  | { readonly kind: "fixed"; readonly unitsPerSecond: number }
  | {
      readonly kind: "percent";
      readonly percentage: number;
      /** Explicit nonnegative scale for headless, non-axis or ambiguous outputs. */
      readonly reference?: number;
    };

/** Rate-limit F32/vec2/vec3/vec4 output lanes after pure projection. */
export function fixed(
  unitsPerSecond: number,
): Extract<DataColumnInterpolation, { kind: "fixed" }> {
  const speed = Math.fround(unitsPerSecond);
  if (!Number.isFinite(speed) || speed <= 0)
    throw new RangeError(
      "Data interpolation speed must be finite and positive",
    );
  return { kind: "fixed", unitsPerSecond: speed };
}

/**
 * Percent of the reference per Host second, not percent of remaining distance.
 * Plot supplies its fitted pre-step axis scale when reference is omitted.
 * An explicit zero reference holds without accumulating missed movement.
 */
export function percent(
  percentage: number,
  reference?: number,
): Extract<DataColumnInterpolation, { kind: "percent" }> {
  const rate = Math.fround(percentage);
  const maximum = reference === undefined ? undefined : Math.fround(reference);
  if (!Number.isFinite(rate) || rate <= 0)
    throw new RangeError(
      "Data interpolation percentage must be finite and positive",
    );
  if (maximum !== undefined && (!Number.isFinite(maximum) || maximum < 0))
    throw new RangeError(
      "Data interpolation reference must be finite and nonnegative",
    );
  return {
    kind: "percent",
    percentage: rate,
    ...(maximum === undefined ? {} : { reference: maximum }),
  };
}

/**
 * How interpolated outputs keep their displayed values when the view changes.
 * `"identity"` follows exact source rows, so new rows initialize immediately.
 * `"position"` keeps each persisting slot of the view, so it moves from its
 * displayed value toward whichever row now occupies it; suits windowed streams
 * whose rows replace a fixed set of stations.
 */
export type DataInterpolationKey = "identity" | "position";

export interface BufferDataSourceBindingProps {
  source: string;
  /** null removes that output and its companions; omission preserves stored properties. */
  columns: Readonly<Record<string, DataColumnBinding | null>>;
  /** Omission preserves the stored choice; new bindings start with `"identity"`. */
  interpolationKey?: DataInterpolationKey;
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
    if (
      !/^[A-Za-z_][A-Za-z_0-9]*$/.test(name) ||
      name.endsWith("_parameter") ||
      name.endsWith("_interp") ||
      name.endsWith("_interp_percent") ||
      name.endsWith("_interp_reference")
    )
      throw new Error(`Invalid data column binding name: ${name}`);
    if (column === null) {
      properties[name] = null;
      properties[`${name}_parameter`] = null;
      properties[`${name}_interp`] = null;
      properties[`${name}_interp_percent`] = null;
      properties[`${name}_interp_reference`] = null;
      continue;
    }
    properties[name] =
      "assetId" in column.definition
        ? column.definition
        : { kind: "asset", value: column.definition };
    if (column.parameter !== undefined)
      properties[`${name}_parameter`] = column.parameter;
    const interpolation = column.interpolation;
    if (interpolation === null) {
      properties[`${name}_interp`] = null;
      properties[`${name}_interp_percent`] = null;
      properties[`${name}_interp_reference`] = null;
    } else if (interpolation?.kind === "fixed") {
      // Remove the old mode before setting the new one: admission is per write.
      properties[`${name}_interp_percent`] = null;
      properties[`${name}_interp_reference`] = null;
      properties[`${name}_interp`] = fixed(
        interpolation.unitsPerSecond,
      ).unitsPerSecond;
    } else if (interpolation?.kind === "percent") {
      const normalized = percent(
        interpolation.percentage,
        interpolation.reference,
      );
      properties[`${name}_interp`] = null;
      properties[`${name}_interp_reference`] = normalized.reference ?? null;
      properties[`${name}_interp_percent`] = normalized.percentage;
    } else if (interpolation !== undefined) {
      throw new Error("Unsupported data interpolation mode");
    }
  }
  return properties;
}

function interpolationKeyField(key: DataInterpolationKey | undefined) {
  if (key === undefined) return {};
  if (key !== "identity" && key !== "position")
    throw new Error(`Unsupported data interpolation key: ${String(key)}`);
  return { interpolation_key: key === "position" ? 1 : 0 };
}

export function BufferDataSourceBinding({
  source,
  columns,
  interpolationKey,
  children,
}: BufferDataSourceBindingProps) {
  return createElement(componentContract.BufferDataSourceBinding.host, {
    source,
    ...interpolationKeyField(interpolationKey),
    properties: columnsProperties(columns),
    children,
  });
}

export function StreamingDataSourceBinding({
  source,
  columns,
  windows = [],
  encodeWindows,
  interpolationKey,
  children,
}: StreamingDataSourceBindingProps) {
  return createElement(componentContract.StreamingDataSourceBinding.host, {
    source,
    windows: encodeWindows(windows),
    ...interpolationKeyField(interpolationKey),
    properties: columnsProperties(columns),
    children,
  });
}
