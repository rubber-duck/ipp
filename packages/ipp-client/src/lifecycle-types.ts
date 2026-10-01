import type {
  ComponentFieldValue,
  LifecycleObservation,
  WorldReference,
} from "./types.js";

export type LifecycleTarget =
  | { kind: "entity"; entity: bigint }
  | { kind: "component"; entity: bigint; component: number }
  /**
   * Current values of schema fields of one component: offsets strictly
   * ascending, at most 64, never rows or named properties. Its baseline reports
   * the component's lifetime.
   */
  | {
      kind: "value";
      entity: bigint;
      component: number;
      fields: readonly number[];
    };

export interface LifecycleTargetSelection {
  target: LifecycleTarget;
  /** Entity bits 1/2/4; component bits 8/16/32/64; value targets select 128. */
  kinds: number;
}

/** One reported field of a value record. */
export interface LifecycleFieldValue {
  offset: number;
  value: ComponentFieldValue;
}

export interface LifecycleMemberId {
  output: bigint;
  generation: bigint;
}

export type LifecycleTargetLifetime =
  | { kind: "entity"; live: boolean }
  | { kind: "component"; entityLive: boolean; incarnation: bigint | null }
  | { kind: "removed" };

export interface LifecycleBaseline {
  member: LifecycleMemberId;
  target: LifecycleTarget;
  lifetime: LifecycleTargetLifetime;
}

export interface LifecycleMembershipCut {
  world: WorldReference;
  session: bigint;
  request: bigint;
  /** Applied mutation boundary, not a completed World frame. */
  sequence: bigint;
  tick: bigint;
}

/** Narrow unknown failures with isLifecycleWatchRemoveError, not constructor identity. */
export interface LifecycleWatchRemoveError extends Error {
  readonly name: "LifecycleWatchRemoveError";
  readonly cuts: readonly LifecycleMembershipCut[];
  readonly unconfirmedMembers: readonly LifecycleMemberId[];
  readonly cause: unknown;
}

export type LifecycleMembershipResult =
  | { kind: "applied"; baselines: LifecycleBaseline[] }
  | {
      kind: "rejected";
      reason:
        | "StaleWorld"
        | "StaleSession"
        | "StaleMember"
        | "AlreadyActive"
        | "TrackingEnded"
        | "Capacity";
    }
  | { kind: "cancelled" };

export type LifecycleWatchRecord = { world: WorldReference; output: bigint } & (
  | {
      kind: "ack";
      action: "add" | "remove";
      cut: { sequence: bigint; tick: bigint } | null;
      result: LifecycleMembershipResult;
    }
  | {
      kind: "event";
      member: LifecycleMemberId;
      sequence: bigint;
      tick: bigint;
      observation: Exclude<LifecycleObservation, { kind: "asset" }>;
    }
  | {
      /**
       * Current values of a value member at the end of an evaluated frame: first
       * after its ACK, then whenever a frame ends with values that differ from
       * the last report. A newer report supersedes an undelivered one.
       */
      kind: "value";
      member: LifecycleMemberId;
      tick: bigint;
      /** Values in the target's field order, or null while the entity or component is absent. */
      values: readonly LifecycleFieldValue[] | null;
    }
);

export type LifecycleWatchRequest = { world: WorldReference } & (
  | { kind: "add"; targets: readonly LifecycleTargetSelection[] }
  | { kind: "remove"; output: bigint; generations: readonly bigint[] }
);

export type LifecycleTargetEvent = Extract<
  LifecycleWatchRecord,
  { kind: "event" }
>;
export type LifecycleValueRecord = Extract<
  LifecycleWatchRecord,
  { kind: "value" }
>;
/**
 * What a watch listener receives, in delivery order: lifecycle events of
 * entity and component members, and value records of value members. A value
 * member reports its current values first, then each frame-end change.
 */
export type LifecycleWatchEvent = LifecycleTargetEvent | LifecycleValueRecord;
export type LifecycleWatchClosure =
  | { kind: "removed"; cuts: readonly LifecycleMembershipCut[] }
  | { kind: "closed"; reason: Error };

export interface LifecycleTargetWatch {
  readonly world: WorldReference;
  /** Frozen baselines from each ordered add page, never attachment witnesses. */
  readonly baselines: readonly LifecycleBaseline[];
  readonly cuts: readonly LifecycleMembershipCut[];
  readonly closed: Promise<LifecycleWatchClosure>;
  /** Remove only original group members; handed-off prefixes precede each ACK. */
  removeMembers(
    members: readonly LifecycleMemberId[],
  ): Promise<readonly LifecycleMembershipCut[]>;
  remove(): Promise<readonly LifecycleMembershipCut[]>;
}
