import type { DatasetValue, DatasetValueKind } from "./datasets.js";
export type {
  DataBindingPage,
  ExpressionResult,
  ExpressionDriverStatus,
  DatasetColumn,
  DatasetDelta,
  DatasetOutcome,
  DatasetPage,
  DatasetProducer,
  DatasetTransfer,
  DatasetValue,
  DatasetValueKind,
} from "./datasets.js";
/** Dataset metadata comes from the executed target contract, separately from World layouts. */
const DATASET_CONTRACT = {
  requestMagic: DATASET_REQUEST_MAGIC,
  responseMagic: DATASET_RESPONSE_MAGIC,
  tag(name: string): number {
    return WIRE[name as keyof typeof WIRE] ?? fail("Dataset contract tag");
  },
  limit(name: string): number {
    return DATASET_LIMITS[name] ?? fail("Dataset contract limit");
  },
};
const DATASET_LIMITS: Readonly<Record<string, number>> = {
  CHUNK_BYTES: DATASET_CHUNK_BYTES,
  UPDATE_BYTES: DATASET_UPDATE_BYTES,
  FRAME_BYTES: DATASET_FRAME_BYTES,
  PAGE_BYTES: DATASET_PAGE_BYTES,
  PAGE_ROWS: DATASET_PAGE_ROWS,
  COLUMNS: DATASET_COLUMNS,
  NAME_BYTES: DATASET_NAME_BYTES,
  DELTAS: DATASET_DELTAS,
  TRANSFERS: DATASET_TRANSFERS,
  STAGING_BYTES: DATASET_STAGING_BYTES,
  PRODUCERS: DATASET_PRODUCERS,
};
