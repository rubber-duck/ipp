/** Chrome Trace Event output keeps raw semantic and clock identities in args. */
export interface ExportedProfileTrace {
  readonly traceEvents: readonly {
    readonly name: string;
    readonly cat: string;
    readonly ph: "X";
    readonly pid: number;
    readonly tid: number;
    readonly ts: number;
    readonly dur: number;
    readonly args: Readonly<Record<string, string | null>>;
  }[];
  readonly displayTimeUnit: "ms";
  readonly metadata: {
    readonly clock: Readonly<Record<string, unknown>>;
    readonly overflow: Readonly<Record<string, unknown>>;
    readonly tracks: Readonly<Record<string, string>>;
  };
}
export function exportProfileTrace(
  capture: unknown,
  options?: {
    readonly browserTracks?: unknown;
    readonly referenceParticipant?: unknown;
  },
): ExportedProfileTrace;
