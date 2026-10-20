import type {
  GuiWorldClient,
  RowsInput,
  RowsLayoutDescriptor,
  WorldReference,
} from "@ipp/client";
import type { BashOptions } from "@everruns/bashkit-wasm";

export interface TerminalFont {
  unitsPerEm: number;
  ascent: number;
  advance: number;
  glyphs: Readonly<Record<string, number>>;
}

export interface TerminalContract {
  encodeRowsTable<Row extends object>(
    layout: RowsLayoutDescriptor,
    rows: RowsInput<Row>,
  ): Uint8Array<ArrayBuffer>;
}

export interface TerminalConfiguration {
  world: WorldReference;
  target: bigint;
  fontSource: string;
  fontMetricsUrl: string;
  fontSize: number;
  lineHeight: number;
  /** Let the surrounding Surface show through the terminal's default background. */
  transparentBackground?: boolean;
  wasm: string;
  callbacksWasm: string;
  bashkit: string;
  shell?: Omit<BashOptions, "customBuiltins" | "fs">;
}

export interface TerminalSessionOptions extends TerminalConfiguration {
  client: GuiWorldClient;
  contract: TerminalContract;
  onError(error: Error): void;
}
