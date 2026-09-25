import type { ClientAssetSource, ComponentDescriptor } from "./types.js";

/** Unique identifier of a node within a GuiRoot component. */
export type GuiNodeId = number;

/**
 * Fenced handle validating session, entity, root incarnation and node identity.
 * Node identities are never reused within a root incarnation.
 */
export interface GuiNodeHandle {
  readonly session: bigint;
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly nodeId: GuiNodeId;
}

/** Construct a fenced GuiNodeHandle with range checking. */
export function guiNodeHandle(
  session: bigint,
  entity: bigint,
  rootIncarnation: bigint,
  nodeId: GuiNodeId,
): GuiNodeHandle {
  if (session < 0n) throw new RangeError("Session must be a non-negative u64");
  if (entity < 0n) throw new RangeError("Entity must be a non-negative u64");
  if (rootIncarnation < 0n)
    throw new RangeError("Root incarnation must be a non-negative u64");
  if (!Number.isInteger(nodeId) || nodeId <= 0 || nodeId > 0xffffffff)
    throw new RangeError("Node identity must be a nonzero u32");
  return Object.freeze({
    session,
    entity,
    rootIncarnation,
    nodeId,
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

/**
 * Node kind with its authored strings. Kind-specific scalars travel as
 * {@link GuiNodeValues} rows beside it.
 */
export type GuiNodeData =
  | { kind: "container"; containerKind: GuiContainerKind }
  | { kind: "text"; text: string }
  | { kind: "drawing" }
  | { kind: "image" }
  | { kind: "button"; label: string }
  | { kind: "checkbox" }
  | { kind: "slider" }
  | { kind: "textInput"; text: string; placeholder: string };

/**
 * Kind-specific scalars of one node, the `GuiRoot.node_data` row: image size
 * for images, the committed checkbox state, and slider value and range.
 * Presence must match the node kind. The encoding follows the contract's row
 * layout; keys are its property names in camelCase.
 */
export interface GuiNodeValues {
  imageSize?: readonly [number, number];
  checked?: boolean;
  value?: number;
  min?: number;
  max?: number;
  step?: number;
}

export type GuiControlValue =
  | { kind: "none" }
  | { kind: "bool"; value: boolean }
  | { kind: "scalar"; value: number }
  | { kind: "text"; value: string };

export type GuiAssetSource = ClientAssetSource;

/**
 * Authored node style, the `GuiRoot.node_style` row. The encoding follows the
 * contract's row layout; keys are its property names in camelCase.
 */
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
  /** Visual translation; moves paint and hit regions without reflow. */
  position?: readonly [number, number];
  /** Visual scale; moves paint and hit regions without reflow. */
  scale?: readonly [number, number];
  /** Handle of the root theme skinning this node. */
  theme?: number;
  /** Bound keyboard traversal from focused descendants to this subtree. */
  focusScope?: boolean;
}

/** Sparse style change; `null` clears an optional property. */
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
  position?: readonly [number, number];
  scale?: readonly [number, number];
  theme?: number | null;
  focusScope?: boolean;
}

/** Stable primitive part a skin styles. */
export type GuiBasePart =
  | "background"
  | "fill"
  | "label"
  | "icon"
  | "focusRing"
  | "scrollTrackX"
  | "scrollThumbX"
  | "scrollTrackY"
  | "scrollThumbY";

/** Interaction state qualifying a skin part. */
export type GuiPartState = "idle" | "hovered" | "pressed" | "disabled";

/** Checked variant qualifying a state-qualified skin part. */
export type GuiPartVariant = "checked" | "unchecked";

/**
 * Enumerated skin part identity: a base part, optionally qualified by a
 * state and, under a state, by a checked variant. The runtime resolves each
 * property through (part, state, variant), (part, state) and (part).
 */
export interface GuiPartId {
  readonly part: GuiBasePart;
  readonly state?: GuiPartState | undefined;
  readonly variant?: GuiPartVariant | undefined;
}

export const GUI_BASE_PARTS: readonly GuiBasePart[] = [
  "background",
  "fill",
  "label",
  "icon",
  "focusRing",
  "scrollTrackX",
  "scrollThumbX",
  "scrollTrackY",
  "scrollThumbY",
];

const GUI_PART_STATES: readonly GuiPartState[] = [
  "idle",
  "hovered",
  "pressed",
  "disabled",
];

/** Qualifiers per base part: the base, four states, and four states with
 * each of two variants. */
export const GUI_PART_QUALIFIERS = 13;

/** Dense wire index of one part identity, below 117. */
export function guiPartIndex(id: GuiPartId): number {
  const base = GUI_BASE_PARTS.indexOf(id.part);
  if (base < 0) throw new RangeError(`Unknown GUI base part ${id.part}`);
  if (id.state === undefined) {
    if (id.variant !== undefined)
      throw new RangeError("A GUI part variant requires a state");
    return base * GUI_PART_QUALIFIERS;
  }
  const state = GUI_PART_STATES.indexOf(id.state);
  if (state < 0) throw new RangeError(`Unknown GUI part state ${id.state}`);
  const variant =
    id.variant === undefined ? 0 : id.variant === "checked" ? 1 : 2;
  return base * GUI_PART_QUALIFIERS + 1 + state * 3 + variant;
}

/** Part identity at a dense wire index. */
export function guiPartFromIndex(index: number): GuiPartId {
  const part = GUI_BASE_PARTS[Math.floor(index / GUI_PART_QUALIFIERS)];
  if (!Number.isInteger(index) || index < 0 || part === undefined)
    throw new RangeError("GUI part index out of range");
  const qualifier = index % GUI_PART_QUALIFIERS;
  if (qualifier === 0) return { part };
  const state = GUI_PART_STATES[Math.floor((qualifier - 1) / 3)]!;
  const variant = (qualifier - 1) % 3;
  return variant === 0
    ? { part, state }
    : { part, state, variant: variant === 1 ? "checked" : "unchecked" };
}

/**
 * Appearance and motion of one skin part, a `GuiRoot.theme_parts` row
 * without its key. Keys are the contract's property names in camelCase.
 * Per-node part overrides accept only the appearance properties.
 */
export interface GuiPartValues {
  color?: readonly [number, number, number, number];
  opacity?: number;
  scale?: readonly [number, number];
  alignX?: number;
  asset?: GuiAssetSource;
  cornerRadius?: readonly [number, number];
  borderWidth?: number;
  borderColor?: readonly [number, number, number, number];
  fillMode?: number;
  gradientStart?: readonly [number, number];
  gradientEnd?: readonly [number, number];
  gradientColor0?: readonly [number, number, number, number];
  gradientColor1?: readonly [number, number, number, number];
  gradientRadius?: number;
  glowColor?: readonly [number, number, number, number];
  glowIntensity?: number;
  glowRadius?: number;
  glowFalloff?: number;
  motion?: GuiAssetSource;
  duration?: number;
  easing?: number;
  track?: number;
  time?: number;
}

/** Sparse part change; `null` clears a property. */
export type GuiPartPatch = {
  [K in keyof GuiPartValues]?: GuiPartValues[K] | null;
};

export interface GuiNode {
  id: GuiNodeId;
  parent?: GuiNodeId;
  children: readonly GuiNodeId[];
  data: GuiNodeData;
}

/**
 * GuiRoot.nodes: structure plus control records. Style and kind-specific
 * scalars live in the `node_style` and `node_data` rows. A live root only
 * accepts this through incremental edits.
 */
export interface GuiTree {
  nextId: number;
  rootNode?: GuiNodeId;
  nodes: readonly GuiNode[];
  /** One record per control node; omitted means none. */
  controls?: GuiControls;
}

/**
 * Control record of one node: the revision that produced its committed value
 * and, for a text input, the committed text. Checkbox and slider values live
 * in the node's `node_data` row. A node whose data stopped being a control
 * keeps its revision.
 */
export interface GuiControlRecord {
  id: GuiNodeId;
  revision: number;
  text?: string;
}

/** Control records of a tree, in node identity order. */
export type GuiControls = readonly GuiControlRecord[];

export type GuiEdit =
  | {
      action: "insert";
      entity: bigint;
      rootIncarnation: bigint;
      id: GuiNodeId;
      parent?: GuiNodeId;
      index: number;
      data: GuiNodeData;
      /** Kind-specific scalars; presence must match the kind. */
      values?: GuiNodeValues;
      style?: GuiNodeStyle;
    }
  | {
      action: "update";
      handle: GuiNodeHandle;
      patch: {
        data?: GuiNodeData;
        /**
         * Replacement authored scalars; omitted with a kind change, the new
         * kind starts empty. Committed values survive compatible edits.
         */
        values?: GuiNodeValues;
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
    }
  | {
      /** Create or patch one part of a root theme; referencing nodes
       * re-resolve without edits. */
      action: "updateTheme";
      entity: bigint;
      rootIncarnation: bigint;
      theme: number;
      part: GuiPartId;
      patch: GuiPartPatch;
    }
  | {
      /** Remove a root theme; referencing nodes resolve without it. */
      action: "removeTheme";
      entity: bigint;
      rootIncarnation: bigint;
      theme: number;
    }
  | {
      /** Patch one node's appearance overrides for a base part. */
      action: "updatePart";
      handle: GuiNodeHandle;
      part: GuiBasePart;
      patch: GuiPartPatch;
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

/**
 * Non-text keys routable to the focused control. `tab` and `backTab`
 * (Shift+Tab) traverse controls in tree order within the innermost focus
 * scope and enter the keyboard panel when nothing has focus.
 */
export type GuiKey =
  | "tab"
  | "backTab"
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
  | { kind: "text"; text: string; fence?: GuiTextFence }
  | { kind: "focus"; handle: GuiNodeHandle }
  | { kind: "blur" }
  | {
      kind: "setTextSelection";
      start: number;
      end: number;
      fence?: GuiTextFence;
    }
  | {
      kind: "composition";
      text: string;
      caretStart: number;
      caretEnd: number;
      fence?: GuiTextFence;
    }
  | { kind: "commitComposition"; fence?: GuiTextFence }
  | { kind: "cancelComposition"; fence?: GuiTextFence };

/**
 * Focus and text revision a native text buffer observed, copied from the
 * published {@link GuiTextFocusState}. The runtime rejects a stamped text,
 * selection or composition command as a conflict, without writing, when
 * the input context, focus generation or target moved, or when the
 * revision is newer than the text. Selections must name the current
 * revision exactly; insertions and composition may name an older revision
 * of the same focus generation. An external replacement of the focused
 * text moves the focus generation. Unstamped commands apply to the current
 * focus in ingress order, like keys.
 */
export interface GuiTextFence {
  readonly contextGeneration: bigint;
  readonly focusGeneration: bigint;
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly revision: number;
}

export interface GuiInspectedNode {
  id: GuiNodeId;
  parent?: GuiNodeId;
  controlRevision: number;
  children: readonly GuiNodeId[];
  data: GuiNodeData;
  /** Kind-specific scalars, including committed checkbox and slider values. */
  values: GuiNodeValues;
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
  /** Runtime logical ancestor path, root-first including the target, when pinned. */
  readonly path?: readonly number[] | undefined;
  /** Routing frame, when the feeding publication carries ticks. */
  readonly sourceTick?: bigint | undefined;
  /** Application frame, when the feeding publication carries ticks. */
  readonly effectTick?: bigint | undefined;
}

/**
 * What produced a committed control value: routed user input, a semantic
 * action, or an explicit revision-aware external replacement.
 */
export type GuiCommitSource = "user" | "semantic" | "external";

/** Committed control outcome, mirroring core ControlCommitted. */
export interface GuiControlCommittedEffect {
  readonly kind: "controlCommitted";
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly value: GuiControlValue;
  readonly revision: number;
  /** What produced the commit, when the feeding publication carries it. */
  readonly source?: GuiCommitSource | undefined;
  /** Runtime logical ancestor path, root-first including the target, when pinned. */
  readonly path?: readonly number[] | undefined;
  /** Routing frame, when the feeding publication carries ticks. */
  readonly sourceTick?: bigint | undefined;
  /** Application frame, when the feeding publication carries ticks. */
  readonly effectTick?: bigint | undefined;
}

/** Enter submitted a focused text input outside composition, mirroring core Submitted. */
export interface GuiSubmittedEffect {
  readonly kind: "submitted";
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  /** Committed text revision that was submitted. */
  readonly revision: number;
  /** Committed text at that revision. */
  readonly text: string;
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
  | GuiControlCommittedEffect
  | GuiSubmittedEffect;

/** Session-scoped target for conflicts, cancellations and scene fallback. */
export interface GuiObservationTarget {
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
}

/** Why a routed intent could not apply cleanly. Mirrors the core reason. */
export type GuiConflictReason =
  | {
      readonly kind: "revisionMismatch";
      readonly expected: number;
      readonly found: number;
    }
  | { readonly kind: "admissionFailed"; readonly reason: string }
  | { readonly kind: "touchArbitration"; readonly ownerPointer: number }
  | { readonly kind: "focusMismatch" };

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
  | { readonly kind: "notOwner" }
  | { readonly kind: "scrollUnconsumed" };

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
 * never accompany one. `@ipp/react/gui` consumes these batches directly. */
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
  readonly revision: number;
  readonly text: string;
  readonly selectionStart: number;
  readonly selectionEnd: number;
  readonly composition?: GuiTextCompositionState;
}

/** A `GuiRoot.node_style` property, named as in {@link GuiNodeStyle}. */
export type GuiNodeStyleProperty = keyof GuiNodeStyle;

/** A `GuiRoot.node_data` property, named as in {@link GuiNodeValues}. */
export type GuiNodeDataProperty = keyof GuiNodeValues;

function guiRowOffset(
  guiRoot: ComponentDescriptor,
  field: "node_style" | "node_data" | "theme_parts" | "part_state",
  slot: number,
  property: string,
): number {
  const layout = guiRoot.fields[field]?.rows;
  if (!layout) throw new RangeError(`GuiRoot.${field} is not a rows field`);
  const snake = property.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
  const index = layout.properties.findIndex(({ name }) => name === snake);
  if (index < 0) throw new RangeError(`Unknown GuiRoot.${field} ${property}`);
  const count = layout.properties.length;
  const nodeTable = field === "node_style" || field === "node_data";
  if (
    !Number.isInteger(slot) ||
    slot < (nodeTable ? 1 : 0) ||
    slot >= Math.floor(0x10000000 / count)
  )
    throw new RangeError(
      nodeTable
        ? "GUI node identity out of row range"
        : "GUI row slot out of range",
    );
  return layout.regionBase + slot * count + index;
}

/**
 * Field offset of one node style property, for animation targets, overlays
 * and field writes. The layout comes from the connected contract's GuiRoot
 * descriptor, so offsets follow the runtime the client talks to.
 */
export function guiNodeStyleOffset(
  guiRoot: ComponentDescriptor,
  node: GuiNodeId,
  property: GuiNodeStyleProperty,
): number {
  return guiRowOffset(guiRoot, "node_style", node, property);
}

/**
 * Field offset of one node data property. Only `imageSize` accepts writes
 * outside GUI commands; committed control values and the slider range are
 * command-owned.
 */
export function guiNodeDataOffset(
  guiRoot: ComponentDescriptor,
  node: GuiNodeId,
  property: GuiNodeDataProperty,
): number {
  return guiRowOffset(guiRoot, "node_data", node, property);
}

/** A `GuiRoot.part_state` property: an appearance override or one of the
 * live channels skin transitions animate. */
export type GuiPartStateProperty =
  | Exclude<
      keyof GuiPartValues,
      "motion" | "duration" | "easing" | "track" | "time"
    >
  | "liveColor"
  | "liveOpacity"
  | "liveScale"
  | "liveAlignX";

/**
 * Field offset of one `GuiRoot.theme_parts` property at a row slot, for
 * animation targets, overlays and field writes. Inspection reports each
 * theme's slots with its `theme` key.
 */
export function guiThemePartOffset(
  guiRoot: ComponentDescriptor,
  slot: number,
  property: keyof GuiPartValues,
): number {
  return guiRowOffset(guiRoot, "theme_parts", slot, property);
}

/**
 * Field offset of one `GuiRoot.part_state` property at a row slot. Skin
 * motion clips name a slot-0 live channel as each track's target hint; the
 * runtime binds the tracks to the transitioning node's own channels.
 */
export function guiPartStateOffset(
  guiRoot: ComponentDescriptor,
  slot: number,
  property: GuiPartStateProperty,
): number {
  return guiRowOffset(guiRoot, "part_state", slot, property);
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
  | "textInput"
  | "scrollView";

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
  parent?: number;
  role: GuiSemanticRole;
  name?: string;
  value: GuiControlValue;
  revision: number;
  bounds: [number, number, number, number];
  enabled: boolean;
  visible: boolean;
  available: boolean;
  /** Keyboard traversal from a focused descendant stays in this subtree. */
  focusScope: boolean;
  actions: GuiSemanticActionKind[];
  /** Committed scroll position of an evaluated ScrollView, in its local
   * logical units. */
  scroll?: GuiSemanticScroll;
}

/** Committed offset and largest offset of one ScrollView per axis. */
export interface GuiSemanticScroll {
  offset: [number, number];
  maxOffset: [number, number];
}

/** Observed input focus within the snapshotted panel, if any. */
export interface GuiSemanticFocus {
  id: number;
}

/** Bounded revision-fenced semantic snapshot of one panel. */
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

/** Semantic action request with node and revision fencing. */
export interface GuiSemanticActionRequest {
  entity: bigint;
  rootIncarnation: bigint;
  node: number;
  expectedRevision: number;
  action: GuiSemanticAction;
}
