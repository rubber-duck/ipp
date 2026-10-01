import { type Client, requestLifecycleStatistics } from "./client.js";
import type { WorldReference } from "./types.js";

/** Lifecycle publisher counters, never a lifecycle or completed-frame observation. */
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

export interface LifecycleDiagnostics {
  /** Requires an acknowledged current endpoint. Saturated samples cannot prove deltas. */
  statistics(output: bigint): Promise<LifecycleDiagnosticSample>;
}

/** Every Host answers; no fallback to wire counts or synthetic zero samples. */
export function lifecycleDiagnostics(client: Client): LifecycleDiagnostics {
  return { statistics: (output) => requestLifecycleStatistics(client, output) };
}
