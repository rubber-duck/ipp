import type { ClientAssetSource } from "./types.js";

/** Unique identifier of a node within a GuiRoot component. */
export type GuiNodeId = number;

/** Fenced handle validating session, entity, root incarnation, node identity and node lifetime. */
export interface GuiNodeHandle {
  readonly session: bigint;
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly nodeId: GuiNodeId;
  readonly nodeLifetime: number;
}

/** Construct a fenced GuiNodeHandle with range checking. */
export function guiNodeHandle(
  session: bigint,
  entity: bigint,
  rootIncarnation: bigint,
  nodeId: GuiNodeId,
  nodeLifetime: number,
): GuiNodeHandle {
  if (session < 0n) throw new RangeError("Session must be a non-negative u64");
  if (entity < 0n) throw new RangeError("Entity must be a non-negative u64");
  if (rootIncarnation < 0n)
    throw new RangeError("Root incarnation must be a non-negative u64");
  if (!Number.isInteger(nodeId) || nodeId <= 0 || nodeId > 0xffffffff)
    throw new RangeError("Node identity must be a nonzero u32");
  if (
    !Number.isInteger(nodeLifetime) ||
    nodeLifetime < 0 ||
    nodeLifetime > 0xffffffff
  )
    throw new RangeError("Node lifetime must be a u32");
  return Object.freeze({
    session,
    entity,
    rootIncarnation,
    nodeId,
    nodeLifetime,
  });
}

export type GuiContainerKind =
  | "row"
  | "column"
  | "stack"
  | "padding"
  | "align"
  | "sizedBox"
  | "scrollView";

export type GuiNodeContent =
  | { kind: "container"; containerKind: GuiContainerKind }
  | { kind: "text"; text: string }
  | { kind: "drawing" }
  | { kind: "image"; size: readonly [number, number] }
  | { kind: "button"; label: string }
  | { kind: "checkbox"; checked: boolean }
  | { kind: "slider"; value: number; min: number; max: number; step: number }
  | { kind: "textInput"; text: string; placeholder: string };

export type GuiControlValue =
  | { kind: "none" }
  | { kind: "bool"; value: boolean }
  | { kind: "scalar"; value: number }
  | { kind: "text"; value: string };

export type GuiAssetSource = ClientAssetSource;

export interface GuiNodeStyle {
  enabled?: boolean;
  width?: number;
  height?: number;
  minWidth?: number;
  minHeight?: number;
  maxWidth?: number;
  maxHeight?: number;
  padding?: readonly [number, number, number, number];
  margin?: readonly [number, number, number, number];
  flex?: number;
  alignX?: number;
  alignY?: number;
  color?: readonly [number, number, number, number];
  backgroundColor?: readonly [number, number, number, number];
  opacity?: number;
  fontSize?: number;
  asset?: GuiAssetSource | null;
}

export interface GuiNodePatchStyle {
  enabled?: boolean;
  width?: number | null;
  height?: number | null;
  minWidth?: number | null;
  minHeight?: number | null;
  maxWidth?: number | null;
  maxHeight?: number | null;
  padding?: readonly [number, number, number, number] | null;
  margin?: readonly [number, number, number, number] | null;
  flex?: number | null;
  alignX?: number | null;
  alignY?: number | null;
  color?: readonly [number, number, number, number];
  backgroundColor?: readonly [number, number, number, number] | null;
  opacity?: number;
  fontSize?: number;
  asset?: GuiAssetSource | null;
}

export interface GuiNode {
  id: GuiNodeId;
  parent?: GuiNodeId;
  lifetime: number;
  children: readonly GuiNodeId[];
  content: GuiNodeContent;
}

/**
 * GuiRoot.nodes: structure plus committed control values. Style lives in named
 * node properties. A live root only accepts this through incremental edits.
 */
export interface GuiTree {
  nextId: number;
  rootNode?: GuiNodeId;
  nodes: readonly GuiNode[];
  /** One committed value per control node; omitted means none. */
  controls?: GuiControls;
}

/**
 * Committed value of one control node and the revision that produced it. A
 * node whose content stopped being a control keeps its revision with "none".
 */
export interface GuiControlState {
  id: GuiNodeId;
  revision: number;
  value: GuiControlValue;
}

/** Committed control values of a tree, in node identity order. */
export type GuiControls = readonly GuiControlState[];

export type GuiEdit =
  | {
      action: "insert";
      entity: bigint;
      rootIncarnation: bigint;
      id: GuiNodeId;
      parent?: GuiNodeId;
      index: number;
      content: GuiNodeContent;
      style?: GuiNodeStyle;
    }
  | {
      action: "update";
      handle: GuiNodeHandle;
      patch: {
        content?: GuiNodeContent;
        style?: GuiNodePatchStyle;
      };
    }
  | {
      action: "move";
      handle: GuiNodeHandle;
      parent?: GuiNodeId;
      index: number;
    }
  | {
      action: "remove";
      handle: GuiNodeHandle;
    }
  | {
      action: "setControlValue";
      handle: GuiNodeHandle;
      expectedRevision: number;
      value: GuiControlValue;
    };

/** Ordered GUI edit acknowledgement. A failed operation may retain partial effects. */
export type GuiEditBatchOutcome =
  | { readonly ok: true; readonly applied: number; readonly requests: number }
  | {
      readonly ok: false;
      readonly applied: number;
      /** Logical-batch control and edit requests sent before this outcome. */
      readonly requests: number;
      readonly error: {
        readonly kind: "runtime" | "admission";
        readonly reason: string;
      };
    };

export interface GuiInspectQuery {
  entity: bigint;
  nodeId?: GuiNodeId;
  maxDepth?: number;
  limit?: number;
}

/** Which physical button a pointer input carries. */
export type GuiPointerButton = "primary" | "secondary" | "auxiliary";

/** Non-text keys routable to the focused control. */
export type GuiKey =
  | "tab"
  | "enter"
  | "space"
  | "escape"
  | "backspace"
  | "delete"
  | "left"
  | "right"
  | "up"
  | "down"
  | "home"
  | "end";

/** GUI logical point: top-left origin, +X right, +Y down. */
export type GuiLogicalPoint = readonly [number, number];

/** Explicit scene blocker in World-space distance. */
export interface GuiInputBlocker {
  readonly entity: bigint;
  readonly distance: number;
}

/**
 * One ordered GUI input routed against the retained per-tick snapshot.
 * Positions and deltas are GUI logical units; an omitted panel selects the
 * overlay-nearest panel and an omitted panel distance counts as nearest.
 */
export type GuiInputCommand =
  | {
      kind: "pointerDown";
      pointer: number;
      panel?: bigint;
      position: GuiLogicalPoint;
      button: GuiPointerButton;
      blockers?: readonly GuiInputBlocker[];
      panelDistance?: number;
    }
  | {
      kind: "pointerUp";
      pointer: number;
      panel?: bigint;
      position: GuiLogicalPoint;
      button: GuiPointerButton;
      blockers?: readonly GuiInputBlocker[];
      panelDistance?: number;
    }
  | {
      kind: "pointerMove";
      pointer: number;
      panel?: bigint;
      position: GuiLogicalPoint;
      blockers?: readonly GuiInputBlocker[];
      panelDistance?: number;
    }
  | { kind: "pointerCancel"; pointer: number }
  | {
      kind: "scroll";
      panel?: bigint;
      position: GuiLogicalPoint;
      delta: GuiLogicalPoint;
      blockers?: readonly GuiInputBlocker[];
      panelDistance?: number;
    }
  | { kind: "key"; key: GuiKey; pressed: boolean }
  | { kind: "text"; text: string }
  | { kind: "focus"; handle: GuiNodeHandle }
  | { kind: "blur" }
  | { kind: "setTextSelection"; start: number; end: number }
  | { kind: "composition"; text: string; caretStart: number; caretEnd: number }
  | { kind: "commitComposition" }
  | { kind: "cancelComposition" };

export interface GuiInspectedNode {
  id: GuiNodeId;
  parent?: GuiNodeId;
  lifetime: number;
  controlRevision: number;
  children: readonly GuiNodeId[];
  content: GuiNodeContent;
  controlValue: GuiControlValue;
  style: GuiNodeStyle;
}

export interface GuiInspectResponse {
  rootEntity: bigint;
  rootIncarnation: bigint;
  nodes: readonly GuiInspectedNode[];
}

/** Committed button outcome, mirroring core ButtonPressed with ticks and path. */
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

/** Committed control outcome, mirroring core ControlCommitted. */
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

/** Authoritative routing disposition for one correlated GUI input. */
export interface GuiInputRoutingOutcome {
  /** Source tick whose current layout and camera routed the input. */
  readonly tick: bigint;
  /** Why no GUI target accepted the input; omitted when GUI handled it. */
  readonly unhandled?: GuiUnhandledReason | undefined;
}

/** One well-formed input that reached no GUI target, for scene controls. */
export interface GuiUnhandledObservation {
  readonly session: bigint;
  readonly tick: bigint;
  readonly input: GuiInputCommand;
  readonly reason: GuiUnhandledReason;
}

/** One ordered observation batch: committed effects plus the records that
 * never accompany one. Shapes mirror `@ipp/react/gui` callbacks so the
 * registry consumes client batches directly. */
export interface GuiObservationBatch {
  readonly effects: readonly GuiCommittedEffect[];
  readonly conflicts?: readonly GuiConflictObservation[] | undefined;
  readonly cancellations?: readonly GuiCancelObservation[] | undefined;
  readonly unhandled?: readonly GuiUnhandledObservation[] | undefined;
  /** Authoritative native editable state; null means the context lost text focus. */
  readonly textFocus?: GuiTextFocusState | null | undefined;
}

export interface GuiTextCompositionState {
  readonly text: string;
  readonly caretStart: number;
  readonly caretEnd: number;
}

export interface GuiTextFocusState {
  readonly session: bigint;
  readonly contextGeneration: bigint;
  readonly focusGeneration: bigint;
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly lifetime: number;
  readonly revision: number;
  readonly text: string;
  readonly selectionStart: number;
  readonly selectionEnd: number;
  readonly composition?: GuiTextCompositionState;
}

export type GuiProperty =
  | "enabled"
  | "width"
  | "height"
  | "min_width"
  | "min_height"
  | "max_width"
  | "max_height"
  | "padding"
  | "margin"
  | "flex"
  | "align_x"
  | "align_y"
  | "color"
  | "background_color"
  | "opacity"
  | "font_size"
  | "asset"
  | "position"
  | "scale";

export type GuiPartProperty =
  | "color"
  | "opacity"
  | "scale"
  | "asset"
  | "corner_radius"
  | "border_width"
  | "border_color"
  | "fill_mode"
  | "gradient_start"
  | "gradient_end"
  | "gradient_color0"
  | "gradient_color1"
  | "gradient_radius"
  | "glow_color"
  | "glow_intensity"
  | "glow_radius"
  | "glow_falloff"
  | "motion"
  | "duration"
  | "easing"
  | "track"
  | "time";

/** Target name for ordinary property animation and StateOverlay commands on a node style. */
export function guiProperty(id: GuiNodeId, property: GuiProperty): string {
  if (!Number.isInteger(id) || id <= 0 || id > 0xffffffff)
    throw new RangeError("GUI node identity must be a nonzero u32");
  return `node_${id}_${property}`;
}

/** Target name for ordinary property animation and StateOverlay commands on a named node skin part. */
export function guiPartProperty(
  id: GuiNodeId,
  part: string,
  property: GuiPartProperty,
): string {
  if (!Number.isInteger(id) || id <= 0 || id > 0xffffffff)
    throw new RangeError("GUI node identity must be a nonzero u32");
  if (!/^[A-Za-z0-9_]+$/.test(part))
    throw new RangeError("Part name must be a nonempty ASCII identifier");
  return `node_${id}_part_${part}_${property}`;
}

/** Machine-observer role for one semantic snapshot node. */
export type GuiSemanticRole =
  | "container"
  | "text"
  | "drawing"
  | "image"
  | "button"
  | "checkbox"
  | "slider"
  | "textInput";

/** Machine-actionable capability advertised by one snapshot node. */
export type GuiSemanticActionKind =
  | "press"
  | "toggle"
  | "setScalar"
  | "setText"
  | "focus";

/** One machine-observer node: identity, role, value, bounds and actions. */
export interface GuiSemanticNode {
  id: number;
  lifetime: number;
  parent?: number;
  role: GuiSemanticRole;
  name?: string;
  value: GuiControlValue;
  revision: number;
  bounds: [number, number, number, number];
  enabled: boolean;
  visible: boolean;
  available: boolean;
  actions: GuiSemanticActionKind[];
}

/** Observed input focus within the snapshotted panel, if any. */
export interface GuiSemanticFocus {
  id: number;
  lifetime: number;
}

/** Bounded lifetime/revision-fenced semantic snapshot of one panel. */
export interface GuiSemanticTree {
  entity: bigint;
  rootIncarnation: bigint;
  evaluationTick: bigint;
  nodes: GuiSemanticNode[];
  focused?: GuiSemanticFocus;
}

/** Bounded snapshot query for one panel (depth 1..32, nodes 1..256). */
export interface GuiSemanticSnapshotQuery {
  entity: bigint;
  maxDepth?: number;
  limit?: number;
}

/** Machine action addressed to one snapshot node revision. */
export type GuiSemanticAction =
  | { kind: "press" }
  | { kind: "toggle" }
  | { kind: "setScalar"; value: number }
  | { kind: "setText"; value: string }
  | { kind: "focus" };

/** Semantic action request with lifetime and revision fencing. */
export interface GuiSemanticActionRequest {
  entity: bigint;
  rootIncarnation: bigint;
  node: number;
  lifetime: number;
  expectedRevision: number;
  action: GuiSemanticAction;
}
