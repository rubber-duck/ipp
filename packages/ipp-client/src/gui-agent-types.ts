import type { Client } from "./client.js";
import type { GuiAction } from "./gui-types.js";
import type { BatchOutcome } from "./types.js";

/** Decimal strings preserve runtime identity and ticks through JSON. */
export interface GuiAgentWorld {
  key: string;
  id: string;
  incarnation: string;
  session: string;
  schemaHash: string;
}

export type GuiAgentKind =
  | "button"
  | "checkbox"
  | "slider"
  | "textInput"
  | "color"
  | "scrollView"
  | "virtualList"
  | "inputTarget"
  | "group"
  | "meter";

export interface GuiAgentControl {
  /** Opaque reference scoped to this adapter and exact runtime lifetime. */
  ref: string;
  world: string;
  entity: string;
  component: number;
  incarnation: string;
  kind: GuiAgentKind;
  label: string;
  parent: string | null;
  /** Authored field associations and validation information. */
  semantics?: {
    description: string;
    error: string;
    required: boolean;
    invalid: boolean;
    labelFor: string | null;
  };
  /** Nearest observed native group, not an inferred application composite. */
  group: string | null;
  tick: string;
  /** Groups without GuiBehavior have no evaluated eligibility record. */
  enabled: boolean | null;
  visible: boolean | null;
  available: boolean | null;
  actions: GuiAction["kind"][];
  /** Focus parts are indexes; per-part semantic labels are not implemented. */
  focusParts: number[];
  state: Record<string, string | number | boolean | null>;
}

export interface GuiAgentObservation {
  world: GuiAgentWorld;
  status: "observed" | "unavailable";
  /** Independent reads, never an atomic World or cross-World snapshot. */
  firstTick: string | null;
  lastTick: string | null;
  truncated: boolean;
  /** Detected lifetime or tree inconsistencies across reads. Take another snapshot to retry. */
  invalidated: number;
  error: string | null;
}

export interface GuiAgentSnapshot {
  controls: GuiAgentControl[];
  observations: GuiAgentObservation[];
}

export interface GuiAgentQuery {
  world?: string;
  kind?: GuiAgentKind;
  /** Exact, case-sensitive semantic label. All duplicate matches are returned. */
  label?: string;
  /** A native group's opaque reference; lists materialized native items only. */
  within?: string;
}

export type GuiAgentTarget =
  | { ref: string }
  | { label: string; world?: string };

export interface GuiAgentStep {
  target: GuiAgentTarget;
  action: GuiAction;
}

export interface GuiAgentStepResult {
  index: number;
  ref: string | null;
  /** Unknown means submission had no acknowledgement; it may have applied. */
  /** Failed means an acknowledged commit failure; its effects may remain. */
  status: "acknowledged" | "rejected" | "failed" | "unknown" | "notRun";
  batch: {
    id: string;
    tick: string;
    ok: boolean;
    error: {
      scope: "operation" | "commit";
      operation: number | null;
      reason: string;
    } | null;
  } | null;
  error: string | null;
  matches: string[];
}

export interface GuiAgentActResult {
  /** Every step acknowledged success; does not assert requested final values. */
  ok: boolean;
  steps: GuiAgentStepResult[];
  /** Fresh observations after acknowledged actions and optional application settling. */
  observed: GuiAgentSnapshot;
  observationErrors: string[];
}

export interface GuiAgentOptions {
  /** Upper bound on entities inspected in each World, including non-controls. */
  maxEntities?: number;
  /**
   * Optional application barrier, e.g. flush a known React root's controlled
   * reconciliation. The adapter cannot await arbitrary asynchronous app work.
   */
  afterAction?: (client: Client, outcome: BatchOutcome) => Promise<void>;
}

export interface GuiAgent {
  snapshot(options?: { world?: string }): Promise<GuiAgentSnapshot>;
  query(options?: GuiAgentQuery): Promise<GuiAgentSnapshot>;
  /** Ordered batches of one action each; failure stops later steps, without rollback. */
  act(request: { steps: readonly GuiAgentStep[] }): Promise<GuiAgentActResult>;
}
