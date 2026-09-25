/** GUI event callbacks for `@ipp/react/gui`.
 *
 * Application callbacks observe committed effects only. The host app feeds
 * committed input outcomes (the client's momentary `buttonPressed`,
 * revision-keyed `controlCommitted` and text `submitted` effects) together
 * with the acknowledged listener table, and this module invokes the matching
 * control callback plus the logical ancestor `onAction` path, whose outermost
 * entries are the GuiRoot's own listeners. Transient cursors (focus, hover, scroll) are
 * not representable here and never produce callbacks.
 *
 * IPP keeps interaction and editing ownership: dispatch returns an
 * observation summary only. There is no retroactive cancel channel back to
 * the runtime; JavaScript `stopPropagation` controls callbacks alone, and
 * committed values, revisions and bounds are already final when callbacks
 * run. Unknown effect kinds are rejected fail-closed, effects for nodes
 * without an acknowledged record are skipped, and a
 * throwing listener is isolated through `onError` without breaking later
 * deliveries.
 *
 * Render-time purity: everything here runs outside the commit phase on
 * already-committed data and performs no transport.
 */
import type {
  GuiCancelObservation,
  GuiCommitSource,
  GuiCommittedEffect,
  GuiConflictObservation,
  GuiControlValue,
  GuiNodeData,
  GuiObservationBatch,
  GuiUnhandledObservation,
  GuiVirtualRangeChangedEffect,
} from "@ipp/client";
import {
  dispatchGuiAction,
  resolveGuiActionPath,
  type GuiActionListeners,
} from "./description.js";
import type {
  GuiActionListener,
  GuiRangeChangeListener,
} from "./components.js";

/** Controls that carry committed values and event callbacks. */
export type GuiControlKind = "button" | "checkbox" | "slider" | "textInput";

/** Momentary-press observation. Buttons store no value and no revision. */
export interface GuiPressEvent {
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly name?: string | undefined;
  /** Routing frame, when the feeding publication carries ticks. */
  readonly sourceTick?: bigint | undefined;
  /** Application frame, when the feeding publication carries ticks. */
  readonly effectTick?: bigint | undefined;
}

/** Committed-value observation with its producing revision. */
export interface GuiControlEvent<T> {
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly revision: number;
  readonly value: T;
  /** What produced a committed value; absent for submissions and when the
   * feeding publication omits it. */
  readonly source?: GuiCommitSource | undefined;
  readonly name?: string | undefined;
  /** Routing frame, when the feeding publication carries ticks. */
  readonly sourceTick?: bigint | undefined;
  /** Application frame, when the feeding publication carries ticks. */
  readonly effectTick?: bigint | undefined;
}

export type GuiPressListener = (event: GuiPressEvent) => void;
export type GuiToggleListener = (event: GuiControlEvent<boolean>) => void;
export type GuiScalarCommitListener = (event: GuiControlEvent<number>) => void;
export type GuiTextCommitListener = (event: GuiControlEvent<string>) => void;
/** Observer of Enter submitting the committed text outside composition. */
export type GuiTextSubmitListener = (event: GuiControlEvent<string>) => void;

function isU32(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= 0xffffffff
  );
}

function isControlValue(value: unknown): value is GuiControlValue {
  if (typeof value !== "object" || value === null) return false;
  const kind = (value as { kind: unknown }).kind;
  switch (kind) {
    case "none":
      return true;
    case "bool":
      return typeof (value as { value: unknown }).value === "boolean";
    case "scalar":
      return typeof (value as { value: unknown }).value === "number";
    case "text":
      return typeof (value as { value: unknown }).value === "string";
    default:
      return false;
  }
}

/** Fail-closed guard for optional runtime metadata: a present path must be
 * node identities and present ticks must be frame identities. */
function isEffectMetadata(effect: Record<string, unknown>): boolean {
  if (effect.path !== undefined) {
    if (!Array.isArray(effect.path) || !effect.path.every(isU32)) return false;
  }
  if (effect.sourceTick !== undefined && typeof effect.sourceTick !== "bigint")
    return false;
  if (effect.effectTick !== undefined && typeof effect.effectTick !== "bigint")
    return false;
  return true;
}

/** Fail-closed guard: unknown or malformed effects never dispatch. */
export function isCommittedEffect(value: unknown): value is GuiCommittedEffect {
  if (typeof value !== "object" || value === null) return false;
  const effect = value as Record<string, unknown>;
  if (typeof effect.entity !== "bigint") return false;
  if (typeof effect.rootIncarnation !== "bigint") return false;
  if (!isU32(effect.node) || (effect.node as number) === 0) return false;
  switch (effect.kind) {
    case "buttonPressed":
      return isEffectMetadata(effect);
    case "controlCommitted":
      return (
        isU32(effect.revision) &&
        isControlValue(effect.value) &&
        (effect.source === undefined ||
          effect.source === "user" ||
          effect.source === "semantic" ||
          effect.source === "external") &&
        isEffectMetadata(effect)
      );
    case "submitted":
      return (
        isU32(effect.revision) &&
        typeof effect.text === "string" &&
        isEffectMetadata(effect)
      );
    default:
      return false;
  }
}

/** Control kind for one node kind, or null for non-controls. */
export function controlKindForData(data: GuiNodeData): GuiControlKind | null {
  switch (data.kind) {
    case "button":
      return "button";
    case "checkbox":
      return "checkbox";
    case "slider":
      return "slider";
    case "textInput":
      return "textInput";
    default:
      return null;
  }
}

/** Human-readable name for one node's authored strings, if any.
 *
 * Button labels and nonempty text-input placeholders; otherwise none. This
 * matches the semantic name the runtime reports.
 */
export function nameForData(data: GuiNodeData): string | undefined {
  switch (data.kind) {
    case "button":
      return data.label;
    case "textInput":
      return data.placeholder.length > 0 ? data.placeholder : undefined;
    default:
      return undefined;
  }
}

/** Acknowledged listener record for one node. Control nodes carry their
 * control listeners; structural ancestors subscribe as `"container"` for
 * the `onAction` path only and never consume control effects. */
export interface GuiControlListenerRecord {
  readonly kind: GuiControlKind | "container";
  readonly name?: string | undefined;
  readonly onPress?: GuiPressListener | undefined;
  readonly onToggle?: GuiToggleListener | undefined;
  readonly onScalarCommit?: GuiScalarCommitListener | undefined;
  readonly onTextCommit?: GuiTextCommitListener | undefined;
  readonly onSubmit?: GuiTextSubmitListener | undefined;
  readonly onAction?: GuiActionListener | undefined;
  readonly onActionCapture?: GuiActionListener | undefined;
}

/** Resolution inputs for one dispatch: acknowledged listeners plus ancestry. */
export interface GuiCallbackResolution {
  /** Logical parents within the exact acknowledged root identity. */
  readonly parentOf: (
    entity: bigint,
    rootIncarnation: bigint,
  ) => ReadonlyMap<number, number | undefined>;
  /** Acknowledged listener record within the exact root identity. */
  readonly listeners: (
    entity: bigint,
    rootIncarnation: bigint,
    node: number,
  ) => GuiControlListenerRecord | undefined;
  /** The GuiRoot's own action listeners, outermost on every path. */
  readonly rootListeners?: GuiRootListenerResolver | undefined;
  readonly onError?: (error: Error) => void;
}

/** Resolves the GuiRoot action listeners of one exact root identity. */
export type GuiRootListenerResolver = (
  entity: bigint,
  rootIncarnation: bigint,
) => GuiActionListeners | undefined;

/** Observation summary. No retroactive cancel channel exists by design. */
export interface GuiCallbackSummary {
  readonly delivered: number;
  readonly skipped: number;
}

function errorOf(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

/** Ancestor path for one committed effect: the pinned runtime path when it
 * names this target, otherwise the acknowledged ancestry. A stale pinned
 * path never misroutes; it reports and falls back. The bound mirrors the
 * core node limit. */
function resolveEffectPath(
  effect: GuiCommittedEffect,
  parentOf: ReadonlyMap<number, number | undefined>,
  report: (message: string) => void,
): readonly number[] {
  const candidate = effect.path;
  if (candidate !== undefined) {
    const usable =
      candidate.length > 0 &&
      candidate.length <= 65536 &&
      candidate.every(isU32) &&
      candidate[candidate.length - 1] === effect.node;
    if (usable) return candidate;
    report(
      `GUI effect for node ${effect.node} carries a stale runtime path; ` +
        `falling back to acknowledged ancestry`,
    );
  }
  return resolveGuiActionPath(parentOf, effect.node);
}

/** Dispatch committed effects to control callbacks and the `onAction` path.
 *
 * For each effect, in order: skip unknown nodes, report-and-skip
 * value-kind mismatches, invoke the matching control callback
 * (observe-only; throws are isolated), then dispatch capture/bubble
 * `onAction` along the logical ancestor path.
 */
export function dispatchControlEffects(
  effects: readonly GuiCommittedEffect[],
  resolution: GuiCallbackResolution,
): GuiCallbackSummary {
  let delivered = 0;
  let skipped = 0;
  const report = (message: string): void => {
    resolution.onError?.(new Error(message));
  };
  for (const effect of effects) {
    const record = resolution.listeners(
      effect.entity,
      effect.rootIncarnation,
      effect.node,
    );
    if (!record) {
      skipped += 1;
      continue;
    }
    const invoke = matchControlCallback(record, effect, report);
    if (!invoke) {
      skipped += 1;
      continue;
    }
    try {
      invoke();
    } catch (error) {
      resolution.onError?.(errorOf(error));
    }
    delivered += 1;
    const parentOf = resolution.parentOf(effect.entity, effect.rootIncarnation);
    const path = resolveEffectPath(effect, parentOf, report);
    dispatchGuiAction(
      path,
      (identity) => {
        const entry = resolution.listeners(
          effect.entity,
          effect.rootIncarnation,
          identity,
        );
        return {
          capture: entry?.onActionCapture,
          bubble: entry?.onAction,
        };
      },
      effect.node,
      resolution.rootListeners?.(effect.entity, effect.rootIncarnation),
      resolution.onError,
    );
  }
  return { delivered, skipped };
}

function matchControlCallback(
  record: GuiControlListenerRecord,
  effect: GuiCommittedEffect,
  report: (message: string) => void,
): (() => void) | null {
  const base: {
    entity: bigint;
    rootIncarnation: bigint;
    node: number;
    name?: string | undefined;
    sourceTick?: bigint | undefined;
    effectTick?: bigint | undefined;
  } = {
    entity: effect.entity,
    rootIncarnation: effect.rootIncarnation,
    node: effect.node,
  };
  if (record.name !== undefined) base.name = record.name;
  if (effect.sourceTick !== undefined) base.sourceTick = effect.sourceTick;
  if (effect.effectTick !== undefined) base.effectTick = effect.effectTick;
  switch (record.kind) {
    case "button": {
      if (effect.kind !== "buttonPressed") {
        report(`GUI button node ${effect.node} ignores ${effect.kind} effects`);
        return null;
      }
      const listener = record.onPress;
      return () => listener?.(base);
    }
    case "checkbox": {
      if (effect.kind !== "controlCommitted" || effect.value.kind !== "bool") {
        report(
          `GUI checkbox node ${effect.node} expects a bool controlCommitted effect`,
        );
        return null;
      }
      const listener = record.onToggle;
      const event: GuiControlEvent<boolean> = {
        ...base,
        revision: effect.revision,
        value: effect.value.value,
        ...(effect.source === undefined ? {} : { source: effect.source }),
      };
      return () => listener?.(event);
    }
    case "slider": {
      if (
        effect.kind !== "controlCommitted" ||
        effect.value.kind !== "scalar"
      ) {
        report(
          `GUI slider node ${effect.node} expects a scalar controlCommitted effect`,
        );
        return null;
      }
      const listener = record.onScalarCommit;
      const event: GuiControlEvent<number> = {
        ...base,
        revision: effect.revision,
        value: effect.value.value,
        ...(effect.source === undefined ? {} : { source: effect.source }),
      };
      return () => listener?.(event);
    }
    case "textInput": {
      if (effect.kind === "submitted") {
        const listener = record.onSubmit;
        const event: GuiControlEvent<string> = {
          ...base,
          revision: effect.revision,
          value: effect.text,
        };
        return () => listener?.(event);
      }
      if (effect.kind !== "controlCommitted" || effect.value.kind !== "text") {
        report(
          `GUI textInput node ${effect.node} expects a text controlCommitted effect`,
        );
        return null;
      }
      const listener = record.onTextCommit;
      const event: GuiControlEvent<string> = {
        ...base,
        revision: effect.revision,
        value: effect.value.value,
        ...(effect.source === undefined ? {} : { source: effect.source }),
      };
      return () => listener?.(event);
    }
    case "container": {
      report(`GUI container node ${effect.node} ignores control effects`);
      return null;
    }
  }
}

function deliverRange(
  listener: GuiRangeChangeListener,
  range: GuiVirtualRangeChangedEffect,
  onError?: (error: Error) => void,
): void {
  try {
    listener({ first: range.first, last: range.last });
  } catch (error) {
    onError?.(errorOf(error));
  }
}

/** Non-effect observation listeners. A throwing listener is isolated
 * through the resolution `onError` without breaking later deliveries. */
export interface GuiObservationSink {
  readonly onUnhandled?:
    | ((observation: GuiUnhandledObservation) => void)
    | undefined;
  readonly onConflict?:
    | ((observation: GuiConflictObservation) => void)
    | undefined;
  readonly onCancelled?:
    | ((observation: GuiCancelObservation) => void)
    | undefined;
  readonly onError?: ((error: Error) => void) | undefined;
}

/** Observation summary. No retroactive cancel channel exists by design. */
export interface GuiObservationSummary extends GuiCallbackSummary {
  readonly conflicts: number;
  readonly cancelled: number;
  readonly unhandled: number;
}

/** Dispatch one ordered observation batch: committed effects reach control
 * callbacks plus the logical ancestor `onAction` path, while conflicts,
 * cancellations and unhandled inputs reach the sink for reporting and scene
 * fallback. Order within each list is preserved; categories never overlap
 * because the core never reports one source input twice. */
export function dispatchGuiObservations(
  batch: GuiObservationBatch,
  resolution: GuiCallbackResolution,
  sink: GuiObservationSink = {},
): GuiObservationSummary {
  const onError = sink.onError ?? resolution.onError;
  const { delivered, skipped } = dispatchControlEffects(batch.effects, {
    ...resolution,
    ...(onError === undefined ? {} : { onError }),
  });
  let conflicts = 0;
  for (const conflict of batch.conflicts ?? []) {
    conflicts += 1;
    try {
      sink.onConflict?.(conflict);
    } catch (error) {
      onError?.(errorOf(error));
    }
  }
  let cancelled = 0;
  for (const cancellation of batch.cancellations ?? []) {
    cancelled += 1;
    try {
      sink.onCancelled?.(cancellation);
    } catch (error) {
      onError?.(errorOf(error));
    }
  }
  let unhandled = 0;
  for (const input of batch.unhandled ?? []) {
    unhandled += 1;
    try {
      sink.onUnhandled?.(input);
    } catch (error) {
      onError?.(errorOf(error));
    }
  }
  return { delivered, skipped, conflicts, cancelled, unhandled };
}

/** Root- and node-fenced listener registry feeding committed observations.
 *
 * The reconciler subscribes each acknowledged control node once and drops
 * the record when the node is removed or the root unmounts. Node identities
 * are never reused within a root incarnation and a replaced root
 * resubscribes under its new incarnation, so delayed effects naming a
 * retired node or incarnation find no record and skip. Dispatch itself stays
 * observe-only: exactly-once delivery rests on the host draining each
 * observation once, and momentary presses carry no idempotency key, so the
 * registry never dedupes — every fed record dispatches once.
 */
export class GuiEffectSubscriptions {
  private readonly records = new Map<string, GuiControlListenerRecord>();
  /** VirtualList range observers by `entity:node`, with the latest range fed
   * for each list. A range can arrive before its list is acknowledged, so
   * the latest one waits for its observer. */
  private readonly ranges = new Map<
    string,
    {
      listener?: GuiRangeChangeListener | undefined;
      latest?: GuiVirtualRangeChangedEffect | undefined;
    }
  >();

  private key(entity: bigint, rootIncarnation: bigint, node: number): string {
    return `${entity}:${rootIncarnation}:${node}`;
  }

  /** Retain one VirtualList's range observer and deliver the latest range
   * already fed for it. */
  subscribeRange(
    entity: bigint,
    node: number,
    listener: GuiRangeChangeListener | undefined,
    onError?: (error: Error) => void,
  ): void {
    const key = `${entity}:${node}`;
    const entry = this.ranges.get(key) ?? {};
    const fresh = entry.listener === undefined && listener !== undefined;
    entry.listener = listener;
    this.ranges.set(key, entry);
    if (fresh && entry.latest !== undefined)
      deliverRange(listener!, entry.latest, onError);
  }

  /** Deliver wanted ranges in order; a range older than the latest one fed
   * for its list is stale and skipped. */
  feedRanges(
    ranges: readonly GuiVirtualRangeChangedEffect[],
    onError?: (error: Error) => void,
  ): number {
    let delivered = 0;
    for (const range of ranges) {
      const key = `${range.entity}:${range.node}`;
      const entry = this.ranges.get(key) ?? {};
      if (entry.latest !== undefined && range.revision <= entry.latest.revision)
        continue;
      entry.latest = range;
      this.ranges.set(key, entry);
      if (entry.listener === undefined) continue;
      deliverRange(entry.listener, range, onError);
      delivered += 1;
    }
    return delivered;
  }

  /** Retain (or replace) the acknowledged record for one node identity. */
  subscribe(
    entity: bigint,
    rootIncarnation: bigint,
    node: number,
    record: GuiControlListenerRecord,
  ): void {
    this.records.set(this.key(entity, rootIncarnation, node), record);
  }

  /** Drop one node's record when its node is removed. */
  unsubscribe(entity: bigint, rootIncarnation: bigint, node: number): boolean {
    this.ranges.delete(`${entity}:${node}`);
    return this.records.delete(this.key(entity, rootIncarnation, node));
  }

  /** Drop every record on root unmount. */
  clear(): void {
    this.records.clear();
    this.ranges.clear();
  }

  get size(): number {
    return this.records.size;
  }

  /** Live resolution over retained records for one dispatch. */
  resolution(
    parentOf: GuiCallbackResolution["parentOf"],
    onError?: (error: Error) => void,
    rootListeners?: GuiRootListenerResolver,
  ): GuiCallbackResolution {
    const listeners = (
      entity: bigint,
      rootIncarnation: bigint,
      node: number,
    ): GuiControlListenerRecord | undefined =>
      this.records.get(this.key(entity, rootIncarnation, node));
    return {
      parentOf,
      listeners,
      ...(rootListeners === undefined ? {} : { rootListeners }),
      ...(onError === undefined ? {} : { onError }),
    };
  }

  /** Dispatch committed effects through retained records. */
  feed(
    effects: readonly GuiCommittedEffect[],
    parentOf: GuiCallbackResolution["parentOf"],
    onError?: (error: Error) => void,
    rootListeners?: GuiRootListenerResolver,
  ): GuiCallbackSummary {
    return dispatchControlEffects(
      effects,
      this.resolution(parentOf, onError, rootListeners),
    );
  }

  /** Dispatch one ordered observation batch through retained records;
   * VirtualList ranges reach their list observers first. */
  feedObservations(
    batch: GuiObservationBatch,
    parentOf: GuiCallbackResolution["parentOf"],
    sink: GuiObservationSink = {},
    rootListeners?: GuiRootListenerResolver,
  ): GuiObservationSummary {
    this.feedRanges(batch.virtualRanges ?? [], sink.onError);
    return dispatchGuiObservations(
      batch,
      this.resolution(parentOf, sink.onError, rootListeners),
      sink,
    );
  }
}
