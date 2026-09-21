/** GUI event callbacks for `@ipp/react/gui` (ipp-9nx.14).
 *
 * Application callbacks observe committed effects only. The host app feeds
 * committed input outcomes (mirroring the frozen `GuiInputEffectKind`
 * control variants: momentary `ButtonPressed` and revision-keyed
 * `ControlCommitted`) together with the acknowledged listener table, and
 * this module invokes the matching control callback plus the logical
 * ancestor `onAction` path. Transient cursors (focus, hover, scroll) are
 * not representable here and never produce callbacks.
 *
 * IPP keeps interaction and editing ownership: dispatch returns an
 * observation summary only. There is no retroactive cancel channel back to
 * the runtime; JavaScript `stopPropagation` controls callbacks alone, and
 * committed values, revisions and bounds are already final when callbacks
 * run. Unknown effect kinds are rejected fail-closed, stale lifetimes are
 * reported as conflicts and skipped (invalidation before reuse), and a
 * throwing listener is isolated through `onError` without breaking later
 * deliveries.
 *
 * Render-time purity: everything here runs outside the commit phase on
 * already-committed data and performs no transport.
 */
import type {
  GuiControlValue,
  GuiInputCommand,
  GuiNodeContent,
} from "@ipp/client";
import { dispatchGuiAction, resolveGuiActionPath } from "./description.js";
import type { GuiActionListener } from "./components.js";

/** Controls that carry committed values and event callbacks. */
export type GuiControlKind = "button" | "checkbox" | "slider" | "textInput";

/** Supported headless actions per control kind, mirroring the .13 semantics. */
export type GuiControlAction =
  | "press"
  | "toggle"
  | "setScalar"
  | "setText"
  | "focus";

/** Momentary-press observation. Buttons store no value and no revision. */
export interface GuiPressEvent {
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly lifetime: number;
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
  readonly lifetime: number;
  readonly revision: number;
  readonly value: T;
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

/** Committed button outcome, mirroring `ButtonPressed{entity, node, lifetime}`. */
export interface GuiButtonPressedEffect {
  readonly kind: "buttonPressed";
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly lifetime: number;
  /** Runtime logical ancestor path, root-first including the target, when pinned. */
  readonly path?: readonly number[] | undefined;
  /** Routing frame, when the feeding publication carries ticks. */
  readonly sourceTick?: bigint | undefined;
  /** Application frame, when the feeding publication carries ticks. */
  readonly effectTick?: bigint | undefined;
}

/** Committed control outcome, mirroring `ControlCommitted`. */
export interface GuiControlCommittedEffect {
  readonly kind: "controlCommitted";
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly lifetime: number;
  readonly value: GuiControlValue;
  readonly revision: number;
  /** Runtime logical ancestor path, root-first including the target, when pinned. */
  readonly path?: readonly number[] | undefined;
  /** Routing frame, when the feeding publication carries ticks. */
  readonly sourceTick?: bigint | undefined;
  /** Application frame, when the feeding publication carries ticks. */
  readonly effectTick?: bigint | undefined;
}

/** Committed effects only; transient cursors are unrepresentable by design. */
export type GuiCommittedEffect =
  | GuiButtonPressedEffect
  | GuiControlCommittedEffect;

/** Whether one committed effect refreshes committed semantics.
 *
 * Mirrors the .13 rule: `ButtonPressed` and `ControlCommitted` name control
 * outcomes; focus, hover and scroll effects report transient cursors and
 * never change committed values, revisions, bounds or states.
 */
export function refreshesSemantics(kind: GuiCommittedEffect["kind"]): boolean {
  return kind === "buttonPressed" || kind === "controlCommitted";
}

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
  if (!isU32(effect.lifetime)) return false;
  switch (effect.kind) {
    case "buttonPressed":
      return isEffectMetadata(effect);
    case "controlCommitted":
      return (
        isU32(effect.revision) &&
        isControlValue(effect.value) &&
        isEffectMetadata(effect)
      );
    default:
      return false;
  }
}

/** Control kind for one structural content, or null for non-controls. */
export function controlKindForContent(
  content: GuiNodeContent,
): GuiControlKind | null {
  switch (content.kind) {
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

/** Supported headless actions for one control kind, mirroring .13. */
export function actionsForControlKind(
  kind: GuiControlKind,
): readonly GuiControlAction[] {
  switch (kind) {
    case "button":
      return ["press"];
    case "checkbox":
      return ["toggle", "focus"];
    case "slider":
      return ["setScalar", "focus"];
    case "textInput":
      return ["setText", "focus"];
  }
}

/** Human-readable name for one structural content, if any.
 *
 * Mirrors the .13 rule: button labels, text-input placeholders while
 * nonempty, otherwise none.
 */
export function nameForContent(content: GuiNodeContent): string | undefined {
  switch (content.kind) {
    case "button":
      return content.label;
    case "textInput":
      return content.placeholder.length > 0 ? content.placeholder : undefined;
    default:
      return undefined;
  }
}

/** Acknowledged listener record for one node. Control nodes carry their
 * control listeners; structural ancestors subscribe as `"container"` for
 * the `onAction` path only and never consume control effects. */
export interface GuiControlListenerRecord {
  /** Acknowledged node lifetime; effects naming another lifetime are stale. */
  readonly lifetime: number;
  readonly kind: GuiControlKind | "container";
  readonly name?: string | undefined;
  readonly onPress?: GuiPressListener | undefined;
  readonly onToggle?: GuiToggleListener | undefined;
  readonly onScalarCommit?: GuiScalarCommitListener | undefined;
  readonly onTextCommit?: GuiTextCommitListener | undefined;
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
  readonly onError?: (error: Error) => void;
}

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
 * For each effect, in order: skip unknown nodes, report-and-skip stale
 * lifetimes and value-kind mismatches, invoke the matching control callback
 * (observe-only; throws are isolated), then dispatch capture/bubble
 * `onAction` along the logical ancestor path via the frozen helper.
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
    if (record.lifetime !== effect.lifetime) {
      report(
        `Stale GUI control effect for node ${effect.node}: ` +
          `effect lifetime ${effect.lifetime} != acknowledged ${record.lifetime}`,
      );
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
    lifetime: number;
    name?: string | undefined;
    sourceTick?: bigint | undefined;
    effectTick?: bigint | undefined;
  } = {
    entity: effect.entity,
    rootIncarnation: effect.rootIncarnation,
    node: effect.node,
    lifetime: effect.lifetime,
  };
  if (record.name !== undefined) base.name = record.name;
  if (effect.sourceTick !== undefined) base.sourceTick = effect.sourceTick;
  if (effect.effectTick !== undefined) base.effectTick = effect.effectTick;
  switch (record.kind) {
    case "button": {
      if (effect.kind !== "buttonPressed") {
        report(
          `GUI button node ${effect.node} ignores controlCommitted effects`,
        );
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
      };
      return () => listener?.(event);
    }
    case "textInput": {
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
      };
      return () => listener?.(event);
    }
    case "container": {
      report(`GUI container node ${effect.node} ignores control effects`);
      return null;
    }
  }
}

/** Session-scoped target for conflicts, cancellations and scene fallback. */
export interface GuiObservationTarget {
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly lifetime: number;
}

/** Why a routed intent could not apply cleanly. Mirrors the core reason. */
export type GuiConflictReason =
  | {
      readonly kind: "revisionMismatch";
      readonly expected: number;
      readonly found: number;
    }
  | { readonly kind: "admissionFailed"; readonly reason: string }
  | { readonly kind: "touchArbitration"; readonly ownerPointer: number };

/** One arbitration or admission conflict, reported separately from effects. */
export interface GuiConflictObservation {
  readonly session: bigint;
  readonly sourceTick: bigint;
  readonly effectTick: bigint;
  readonly target?: GuiObservationTarget | undefined;
  readonly reason: GuiConflictReason;
}

/** Why a routed intent never applied. Never mixed with effects. */
export type GuiCancelReason =
  | "targetRemoved"
  | "targetHidden"
  | "sessionReplaced"
  | "gestureCancelled";

/** One routed input cancelled between routing and application. */
export interface GuiCancelObservation {
  readonly session: bigint;
  readonly sourceTick: bigint;
  readonly effectTick: bigint;
  readonly target?: GuiObservationTarget | undefined;
  readonly reason: GuiCancelReason;
}

/** Why routing reached no target. Mirrors the core reason. */
export type GuiUnhandledReason =
  | { readonly kind: "noPanelHit" }
  | { readonly kind: "blocked"; readonly entity: bigint }
  | { readonly kind: "staleTarget" }
  | { readonly kind: "noFocus" }
  | { readonly kind: "noCapture" }
  | { readonly kind: "notFocusable" }
  | { readonly kind: "notOwner" };

/** One well-formed input that reached no GUI target, for scene controls.
 * The complete input is preserved verbatim so scene fallback observes the
 * same positions, buttons, blockers and distances the router saw. */
export interface GuiUnhandledObservation {
  readonly session: bigint;
  readonly tick: bigint;
  readonly input: GuiInputCommand;
  readonly reason: GuiUnhandledReason;
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

/** One ordered observation batch: committed effects plus the records that
 * never accompany one. Each record is delivered at most once per feed; the
 * host guarantees observations drain exactly once. */
export interface GuiObservationBatch {
  readonly effects: readonly GuiCommittedEffect[];
  readonly conflicts?: readonly GuiConflictObservation[] | undefined;
  readonly cancellations?: readonly GuiCancelObservation[] | undefined;
  readonly unhandled?: readonly GuiUnhandledObservation[] | undefined;
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

/** Lifetime-fenced listener registry feeding committed observations.
 *
 * The reconciler subscribes each acknowledged control node once and drops
 * the record when the node is removed or the root unmounts; re-acknowledged
 * nodes resubscribe under their new lifetime, so delayed effects naming a
 * retired lifetime report as stale and skip. Dispatch itself stays
 * observe-only: exactly-once delivery rests on the host draining each
 * observation once, and momentary presses carry no idempotency key, so the
 * registry never dedupes — every fed record dispatches once.
 */
export class GuiEffectSubscriptions {
  private readonly records = new Map<string, GuiControlListenerRecord>();

  private key(entity: bigint, rootIncarnation: bigint, node: number): string {
    return `${entity}:${rootIncarnation}:${node}`;
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
    return this.records.delete(this.key(entity, rootIncarnation, node));
  }

  /** Drop every record on root unmount. */
  clear(): void {
    this.records.clear();
  }

  get size(): number {
    return this.records.size;
  }

  /** Live resolution over retained records for one dispatch. */
  resolution(
    parentOf: GuiCallbackResolution["parentOf"],
    onError?: (error: Error) => void,
  ): GuiCallbackResolution {
    const listeners = (
      entity: bigint,
      rootIncarnation: bigint,
      node: number,
    ): GuiControlListenerRecord | undefined =>
      this.records.get(this.key(entity, rootIncarnation, node));
    return onError === undefined
      ? { parentOf, listeners }
      : { parentOf, listeners, onError };
  }

  /** Dispatch committed effects through retained records. */
  feed(
    effects: readonly GuiCommittedEffect[],
    parentOf: GuiCallbackResolution["parentOf"],
    onError?: (error: Error) => void,
  ): GuiCallbackSummary {
    return dispatchControlEffects(effects, this.resolution(parentOf, onError));
  }

  /** Dispatch one ordered observation batch through retained records. */
  feedObservations(
    batch: GuiObservationBatch,
    parentOf: GuiCallbackResolution["parentOf"],
    sink: GuiObservationSink = {},
  ): GuiObservationSummary {
    return dispatchGuiObservations(
      batch,
      this.resolution(parentOf, sink.onError),
      sink,
    );
  }
}
