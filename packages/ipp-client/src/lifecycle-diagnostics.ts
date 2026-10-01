import type { Client } from "./client.js";
import type { WorldReference } from "./types.js";

/** Diagnostic-build counters, never a lifecycle or completed-frame observation. */
export interface LifecycleDiagnosticSample {
  world: WorldReference;
  output: bigint;
  work: { lookups: bigint; recipientVisits: bigint; saturated: boolean };
  traffic: { queuedEvents: bigint; queuedBytes: bigint; saturated: boolean };
}

export interface LifecycleDiagnosticQuery {
  world: WorldReference;
  output: bigint;
}

export interface LifecycleTesting {
  /** Requires an acknowledged current endpoint. Saturated samples cannot prove deltas. */
  statistics(output: bigint): Promise<LifecycleDiagnosticSample>;
}

/** Diagnostic builds only; no fallback to wire counts or synthetic zero samples. */
export function lifecycleTesting(client: Client): LifecycleTesting {
  const diagnostic = client as Client & {
    lifecycleStatistics?: (
      output: bigint,
    ) => Promise<LifecycleDiagnosticSample>;
  };
  if (typeof diagnostic.lifecycleStatistics !== "function")
    throw new Error("Lifecycle diagnostics are not compiled in this contract");
  return { statistics: (output) => diagnostic.lifecycleStatistics!(output) };
}
