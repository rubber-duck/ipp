import type { DynamicValue } from "./dynamic-properties.js";
import type { RootBinding } from "./host-presentation.js";
import type {
  LifecycleDiagnosticQuery,
  LifecycleDiagnosticSample,
} from "./lifecycle-diagnostics.js";
import type {
  LifecycleWatchRequest,
  LifecycleWatchRecord,
} from "./lifecycle-types.js";
export type * from "./lifecycle-types.js";
import type {
  GuiAction,
  GuiFocusRecord,
  GuiPointerRecord,
  GuiObservationRequest,
  GuiObservationRecord,
} from "./gui-types.js";
export type {
  DynamicValue,
  DynamicPropertyKind,
  ShaderParameterKind,
  ShaderDefinition,
} from "./dynamic-properties.js";
/** Exact optional identities narrow their domain; omitted domain flags default to true. */
export interface LifecycleFilter {
  entities?: boolean;
  components?: boolean;
  assets?: boolean;
  entity?: bigint;
  component?: number;
  asset?: bigint;
}
export type LifecycleObservation =
  | {
      kind: "entity";
      entity: bigint;
      change: "created" | "metadataChanged" | "deleted";
    }
  | {
      kind: "component";
      entity: bigint;
      component: number;
      change: "inserted" | "updated" | "replaced" | "removed";
      previousIncarnation: bigint | null;
      incarnation: bigint | null;
    }
  | {
      kind: "asset";
      change: "statusChanged" | "graphicsInvalidated" | "removed";
      resource: AssetResourceSnapshot;
    };
export interface LifecyclePublication {
  subscription: bigint;
  /** Monotonic World sequence; matching filters can leave gaps. */
  sequence: bigint;
  /** Boundary where this effect was applied, possibly before the delivery frame. */
  tick: bigint;
  observation: LifecycleObservation;
}
export type LifecycleNotification = EventEnvelope & {
  kind: "change";
} & LifecyclePublication;
export interface LifecycleSubscription {
  readonly id: bigint;
  /** Releases queued and future observations at an ordered World boundary. */
  unsubscribe(): Promise<void>;
}

/**
 * Target-independent public values shared by generated clients and session handling.
 *
 * An entity reference is a concrete handle, a name defined by an earlier
 * command of the same logical batch, or a symbolic identifier: `alias` names an
 * entity created by `create`. Names resolve in command order and behave
 * exactly like the handle they name. `symbol` names the
 * live entity whose metadata carries that symbolic identifier when the command
 * applies, and rejects with `MissingSymbolicId` when none does; the batch
 * outcome's `symbols` reports the handle it resolved to.
 */
export type EntityRef =
  | { kind: "handle"; id: bigint }
  | { kind: "alias"; alias: number }
  | { kind: "symbol"; symbol: string };
export interface WorldReference {
  readonly id: bigint;
  readonly incarnation: bigint;
}
/** The World-level canvas of a World that selects the Canvas System; it lives
 * as long as that World. Build it with `canvasOutput(world)`. */
export interface CanvasOutputReference {
  readonly world: WorldReference;
  readonly kind: "canvas";
}
/** A Camera entity's view, fenced by its Camera component lifetime. */
export interface CameraOutputReference {
  readonly world: WorldReference;
  readonly kind: "camera";
  readonly entity: bigint;
  readonly incarnation: bigint;
}
export type OutputReference = CanvasOutputReference | CameraOutputReference;
export type FieldValue =
  | { kind: "world"; value: WorldReference | null }
  | { kind: "output"; value: OutputReference | null }
  | { kind: "dynamic"; value: DynamicValue }
  | { kind: "bool"; value: boolean }
  | { kind: "f32"; value: number }
  | { kind: "u32"; value: number }
  | { kind: "u64"; value: bigint }
  | { kind: "entity"; value: EntityRef }
  | { kind: "string"; value: string }
  | { kind: "bytes"; value: Uint8Array<ArrayBuffer> }
  /** A whole schema rows table in its row layout's table encoding. */
  | { kind: "rows"; value: Uint8Array<ArrayBuffer> }
  /** Clears an optional schema row property. */
  | { kind: "unset" };
export interface FieldWrite {
  offset: number;
  value: FieldValue;
}
export interface EntityMetadata {
  symbolicId: string | null;
  classes: string[];
}
export interface EntityPlacement {
  parent: EntityRef | null;
  before: EntityRef | null;
}
export const FieldKind = {
  F32: 1,
  Entity: 2,
  U32: 3,
  U64: 4,
  String: 5,
  Bytes: 6,
  Bool: 7,
  Rows: 8,
  World: 12,
  Output: 13,
} as const;
export type FieldKind = (typeof FieldKind)[keyof typeof FieldKind];
/** Schema row property types: the dynamic property kinds without matrices. */
export type RowPropertyKind =
  | "f32"
  | "i32"
  | "u32"
  | "bool"
  | "vec2"
  | "vec3"
  | "vec4"
  | "asset"
  | "text";
export interface RowPropertyDescriptor {
  readonly name: string;
  readonly kind: RowPropertyKind;
  readonly optional: boolean;
  /** `rotation` marks a Vec4 quaternion interpolated as a rotation. */
  readonly hint: "none" | "rotation";
  /** UTF-8 byte bound, present exactly for `text` properties. */
  readonly maxBytes?: number;
}
/** Target-exported layout of one schema rows field; order defines property indices. */
export interface RowsLayoutDescriptor {
  /** Offset of slot 0, property 0; a property is `regionBase + slot * count + index`. */
  readonly regionBase: number;
  readonly properties: readonly RowPropertyDescriptor[];
}
/** Asset selection held by a schema row property. */
export interface RowAssetValue {
  kind: number;
  source: string;
  variant?: number;
}
export type RowPropertyValue =
  | number
  | boolean
  | string
  | readonly number[]
  | RowAssetValue;
/** A decoded schema rows table: live rows by never-reused slot, absent properties omitted. */
/** A complete table with caller-owned, monotonically allocated slot identities. */
export interface RowsInput<Row = Record<string, RowPropertyValue>> {
  readonly nextSlot: number;
  readonly rows: ReadonlyMap<number, Readonly<Row>>;
}

export interface RowsTable<Row = Record<string, RowPropertyValue>> {
  /** Lowest unallocated slot; lower slots without a row are dead. */
  nextSlot: number;
  rows: Map<number, Row>;
}
export interface FieldDescriptor {
  readonly offset: number;
  readonly kind: FieldKind;
  /** Present exactly for schema rows fields. */
  readonly rows?: RowsLayoutDescriptor;
}
export interface ComponentDescriptor {
  readonly id: number;
  /** Whether this compiled component permits default construction. */
  readonly creatable?: boolean;
  readonly dynamicProperties?: boolean;
  readonly fields: Readonly<Record<string, FieldDescriptor>>;
}
export type Command =
  | {
      kind: "setDynamicProperty";
      entity: EntityRef;
      component: number;
      name: string;
      value: DynamicValue;
    }
  | {
      kind: "removeDynamicProperty";
      entity: EntityRef;
      component: number;
      name: string;
    }
  | {
      kind: "create";
      alias: number;
      metadata: EntityMetadata;
      /** Bind a live entity with the same symbolic id instead of failing. */
      adopt?: boolean;
    }
  | { kind: "delete"; entity: EntityRef }
  | { kind: "placeEntity"; entity: EntityRef; placement: EntityPlacement }
  | { kind: "deleteSubtree"; root: EntityRef }
  | { kind: "detachWorldAttachment"; receipt: bigint }
  | { kind: "setMetadata"; entity: EntityRef; metadata: EntityMetadata }
  | {
      kind: "insertComponent";
      entity: EntityRef;
      component: number;
      fields: FieldWrite[];
      /** Write only these fields of an existing component, keeping its incarnation. */
      adopt?: boolean;
    }
  | {
      kind: "setField";
      entity: EntityRef;
      component: number;
      field: FieldWrite;
    }
  /**
   * Compare-and-set: write `field` only while it holds `expected`, a value of
   * the field's own type; otherwise the batch stops with `ValueMismatch`.
   */
  | {
      kind: "setFieldIf";
      entity: EntityRef;
      component: number;
      field: FieldWrite;
      expected: FieldValue;
    }
  | { kind: "removeComponent"; entity: EntityRef; component: number }
  /**
   * Apply a semantic action to the control component at `incarnation`. The
   * batch stops without effect with `StaleTarget`, `Unavailable`,
   * `UnsupportedAction` or `InvalidValue`; values change as component fields
   * and press, submit and focus changes are observed as GUI effects.
   */
  | {
      kind: "guiAction";
      entity: EntityRef;
      component: number;
      incarnation: bigint;
      action: GuiAction;
    };

export interface PublicationReference {
  host: bigint;
  revision: bigint;
}
export interface ViewViewport {
  width: number;
  height: number;
  devicePixelRatio: number;
}
/** Historical CPU access never authorizes presentation or input. */
export type ViewQueryTarget =
  | { kind: "bound"; binding: RootBinding; publication?: PublicationReference }
  | { kind: "root"; output: OutputReference; expectedViewport: ViewViewport }
  | {
      kind: "publication";
      output: OutputReference;
      publication: PublicationReference;
      viewport: ViewViewport;
    };
/** Exact completed inputs used for a successful query, not a presentation fence. */
export interface ViewDescriptor {
  output: OutputReference;
  publication: PublicationReference;
  viewport: ViewViewport;
}
/** Read completed geometry through normalized top-left viewport coordinates. */
export interface GeometryPickQuery {
  type: "GeometryPickQuery";
  view: ViewQueryTarget;
  x: number;
  y: number;
  /** Include a camera-facing world plane through the hit; defaults to false. */
  includeViewPlane?: boolean;
}
/** A world-space plane; normal must be finite and nonzero. */
export interface WorldPlane {
  point: [number, number, number];
  normal: [number, number, number];
}
/** Project onto a plane, including viewport positions outside the canvas. */
export interface CameraProjectQuery {
  type: "CameraProjectQuery";
  view: ViewQueryTarget;
  x: number;
  y: number;
  plane: WorldPlane;
}

/** Deltas in the exact view domain; pixel rounding never supplies projection aspect. */
export type CameraViewMotion =
  | { kind: "rotate"; yaw: number; pitch: number }
  | { kind: "pan"; x: number; y: number }
  | { kind: "zoom"; amount: number };

/** Correlated root Camera mutation, fenced by binding generation and available source. */
export interface CameraNavigateRequest {
  binding: RootBinding;
  /** Omit for current completed state; an explicit source never falls back to latest. */
  publication?: PublicationReference;
  motion: CameraViewMotion;
}
export type CameraProjectResultPayload = {
  type: "CameraProjectResultEvent";
} & (
  | {
      ok: true;
      view: ViewDescriptor;
      position: [number, number, number] | null;
    }
  | { ok: false; error: string }
);
export type CameraProjectResultEvent = EventEnvelope &
  CameraProjectResultPayload;
export type SystemQuery = GeometryPickQuery | CameraProjectQuery;
export type SystemQueryResult =
  | GeometryPickResultEvent
  | CameraProjectResultEvent;
/** Sparse session settings; omitted fields preserve their committed values. */
export interface RenderStatePatch {
  showAllDebugGeometries?: boolean;
  debugGeometryColor?: [number, number, number];
  /** Finite nonnegative linear RGB ambient fill; zero disables it. */
  ambientLight?: [number, number, number];
}
export interface RenderStateUpdateCommand {
  type: "RenderStateUpdateCommand";
  changes: RenderStatePatch;
}
export interface RenderStateUpdatedPayload {
  type: "RenderStateUpdatedEvent";
  changes: RenderStatePatch;
}
export type RenderStateUpdatedEvent = EventEnvelope & RenderStateUpdatedPayload;
/** Logical extent and units-per-metre density of a World's canvas; every value
 * finite and positive. Defaults are a 1 x 1 extent and density 1. */
export interface CanvasState {
  extent: readonly [number, number];
  unitsPerMetre: number;
}
/** Sparse Canvas System update (Surface builds). Omitted values keep their
 * current values; an invalid update has no effect and no reply. */
export interface CanvasStateUpdateCommand {
  type: "CanvasStateUpdateCommand";
  extent?: readonly [number, number];
  unitsPerMetre?: number;
}
/** The Canvas inspection collection's one record. */
export interface CanvasStateRecord {
  state: CanvasState;
  /** Last evaluated logical extent and its tick; null before the first evaluation. */
  evaluated: { extent: readonly [number, number]; tick: bigint } | null;
}
export type SystemCommand =
  | AnimationPlaybackCommand
  | RenderStateUpdateCommand
  | CanvasStateUpdateCommand;
export interface EventEnvelope {
  session: bigint;
  requestId: bigint;
  tick: bigint;
}
export type GeometryPickHit = {
  world: WorldReference;
  publication: PublicationReference;
  entity: bigint;
  incarnation: bigint;
  path: { world: WorldReference; anchor: bigint }[];
  position: [number, number, number];
  distance: number;
  /** Present only when requested; normal is camera forward, not a surface normal. */
  viewPlane?: WorldPlane;
  /** Stable primitive index in the shared geometry definition. */
  part: number;
};
export type GeometryPickResultPayload = {
  type: "GeometryPickResultEvent";
} & (
  | { ok: true; view: ViewDescriptor; hit: GeometryPickHit | null }
  | { ok: false; error: string }
);
export type GeometryPickResultEvent = EventEnvelope & GeometryPickResultPayload;
export interface ClientAssetSource {
  kind: number;
  source: string;
  variant?: number;
}

export type RequestBody =
  | { kind: "cameraNavigate"; request: CameraNavigateRequest }
  | { kind: "lifecycleWatch"; control: LifecycleWatchRequest }
  | { kind: "lifecycleDiagnostics"; query: LifecycleDiagnosticQuery }
  | { kind: "guiObservation"; control: GuiObservationRequest }
  | {
      kind: "subscribeLifecycle";
      subscription: bigint;
      filter: LifecycleFilter;
    }
  | { kind: "unsubscribeLifecycle"; subscription: bigint }
  | { kind: "command"; command: SystemCommand }
  | { kind: "query"; query: SystemQuery }
  | { kind: "attachmentReceipt"; receipt: bigint; release: boolean }
  | {
      /**
       * One page of a batch under a client-assigned identity that is unique
       * among the connection's open batches. Only the final page carries a
       * request identity and is answered, with the whole batch's outcome.
       */
      kind: "submitBatch";
      batchId: number;
      last: boolean;
      operations: Command[];
    }
  | { kind: "animationController"; command: AnimationControllerCommand }
  | {
      kind: "inspectTree";
      root?: bigint;
      after?: bigint;
      limit?: number;
      maxDepth?: number;
    }
  | ({ kind: "inspect" } & InspectionQuery);
export interface Request {
  session: bigint;
  requestId: bigint;
  body: RequestBody;
}
/** A failed batch may have applied operations and allocated the returned identities. */
export type BatchOutcome = {
  batchId: bigint;
  tick: bigint;
  aliases: { alias: number; id: bigint }[];
  /** Handles symbolic references resolved to, once per distinct symbol and handle. */
  symbols: { symbol: string; id: bigint }[];
  /** Applied per-operation effects in operation order. */
  effects: BatchOperationEffect[];
} & (
  | { ok: true }
  | {
      ok: false;
      error: {
        scope: "operation" | "commit";
        operation: number | null;
        reason: string;
      };
    }
);
export interface AttachmentReceipt {
  readonly id: bigint;
  readonly parent: WorldReference;
  readonly anchor: bigint;
  readonly incarnation: bigint;
  readonly revision: bigint;
  readonly child: WorldReference | null;
}
export interface AttachmentEffect {
  readonly operation: number;
  readonly kind: "written" | "detached" | "superseded";
  readonly receipt: AttachmentReceipt;
}
/**
 * An adopting `create` bound an existing entity, or an adopting
 * `insertComponent` wrote an existing component in place. An adopting operation
 * that created or inserted reports no effect.
 */
export interface AdoptionEffect {
  readonly operation: number;
  readonly kind: "adopted";
}
export type BatchOperationEffect = AttachmentEffect | AdoptionEffect;
export type ComponentFieldValue =
  | WorldReference
  | OutputReference
  | null
  | boolean
  | number
  | bigint
  | string
  | Uint8Array<ArrayBuffer>
  | RowsTable;
export interface ComponentSnapshot {
  component: number;
  properties?: Record<string, DynamicValue>;
  fields: Record<string, ComponentFieldValue>;
}
/** One entity's stored state: its link and its components in registry order. */
export interface EntitySnapshot {
  id: bigint;
  metadata: EntityMetadata;
  link: { parent: bigint | null; order: bigint };
  components: ComponentSnapshot[];
}
/** Current typed source demand, independent of world batch acknowledgement. */
export interface AssetRepresentationStatus {
  decoded: boolean;
  graphicsReady: boolean | null;
  sourceBytes: bigint;
  residentBytes: bigint;
  graphicsBytes: bigint | null;
}
export interface AssetResourceSnapshot {
  representation: AssetRepresentationStatus;
  id: bigint;
  kind: number;
  source: string;
  variant: number;
  status: "unloaded" | "start" | "progress" | "loaded" | "failed";
  completed?: bigint;
  total?: bigint;
  error?: string;
}
/** Per-entity compatibility failures; a shared decoded resource may remain ready. */
export interface RenderDiagnostic {
  entity: bigint;
  reason: string;
}
export interface InspectionQuery {
  /** `guiFocus` and `guiPointers` are GUI System queries, present in GUI
   * builds; `canvas` is the Canvas System query, present in Surface builds. */
  collection:
    | "summary"
    | "entities"
    | "resources"
    | "controllers"
    | "renderDiagnostics"
    | "guiFocus"
    | "guiPointers"
    | "canvas";
  after?: bigint;
  target?: bigint;
  limit?: number;
}
export interface InspectionPage extends Inspection {
  next: bigint;
}

export interface EntityTreeNode {
  id: bigint;
  parent: bigint | null;
  order: bigint;
  depth: number;
}
export interface EntityTreePage {
  tick: bigint;
  time: number;
  next: bigint;
  nodes: readonly EntityTreeNode[];
}
export interface EntityTreeQuery {
  root?: bigint;
  after?: bigint;
  limit?: number;
  maxDepth?: number;
}

export interface Inspection {
  tick: bigint;
  time: number;
  entities: EntitySnapshot[];
  resources: readonly AssetResourceSnapshot[];
  renderDiagnostics: readonly RenderDiagnostic[];
  controllers?: readonly AnimationControllerSnapshot[];
  /** GUI builds: logical focus. */
  guiFocus?: readonly GuiFocusRecord[];
  /** GUI builds: live pointer feedback. */
  guiPointers?: readonly GuiPointerRecord[];
  /** Surface builds: the World canvas's state, when the query read it. */
  canvas?: CanvasStateRecord | null;
}
export interface RuntimeFailure {
  scope: "draw" | "resource" | "context" | "world";
  faulted: boolean;
  message: string;
}

export type ResponseBody =
  | { kind: "cameraNavigated" }
  | { kind: "lifecycleWatch"; record: LifecycleWatchRecord }
  | { kind: "lifecycleDiagnostics"; sample: LifecycleDiagnosticSample }
  | { kind: "guiObservation"; record: GuiObservationRecord }
  | ({ kind: "runtimeFailure" } & RuntimeFailure)
  | { kind: "lifecycleSubscription" }
  | { kind: "lifecycleEvents"; events: LifecyclePublication[] }
  | { kind: "animationController"; id: bigint | null }
  | { kind: "playback"; events: readonly AnimationPlaybackEventPayload[] }
  | {
      kind: "event";
      event:
        | GeometryPickResultPayload
        | CameraProjectResultPayload
        | RenderStateUpdatedPayload;
    }
  | { kind: "batch"; outcome: BatchOutcome }
  | {
      kind: "attachmentReceipt";
      receipt: bigint;
      state: "pending" | "retired" | "released";
    }
  | { kind: "batchAborted"; batchId: bigint; message: string }
  | { kind: "frame"; time: number }
  | { kind: "resources"; resources: readonly AssetResourceSnapshot[] }
  | {
      kind: "inspect";
      next: bigint;
      time: number;
      entities: EntitySnapshot[];
      resources: readonly AssetResourceSnapshot[];
      renderDiagnostics: readonly RenderDiagnostic[];
      controllers?: readonly AnimationControllerSnapshot[];
      guiFocus?: readonly GuiFocusRecord[];
      guiPointers?: readonly GuiPointerRecord[];
      canvas?: CanvasStateRecord | null;
    }
  | {
      kind: "entityTree";
      next: bigint;
      time: number;
      nodes: readonly EntityTreeNode[];
    }
  | { kind: "error"; code: number; message: string };
export interface Response {
  session: bigint;
  requestId: bigint;
  tick: bigint;
  body: ResponseBody;
}

export type { ViewportLimits } from "./presentation.js";

/** World-local controller state, observed at the inspection/event tick. */
export interface AnimationControllerState {
  id: bigint;
  state: "stopped" | "playing" | "paused" | "completed";
  time: number;
}
export type AnimationDriverTarget =
  | {
      entityLink: true;
      component?: never;
      name?: never;
      offsets?: never;
      joints?: never;
    }
  | {
      component: number;
      name: string;
      offsets?: never;
      joints?: never;
      entityLink?: never;
    }
  | {
      component: number;
      offsets: readonly number[];
      name?: never;
      joints?: never;
      entityLink?: never;
    }
  | {
      joints: readonly number[];
      component?: never;
      offsets?: never;
      name?: never;
      entityLink?: never;
    };
export interface AnimationDriverDescription {
  source: string;
  variant?: number;
  track: number;
  target: bigint;
  property: AnimationDriverTarget;
  entityBindings?: readonly bigint[];
  weight?: number;
  additive?: boolean;
  referenceTime?: number;
  /** Repeat the source clip using this controller's shared clock. */
  repeat?: boolean;
}
export interface AnimationControllerDescription {
  drivers: readonly AnimationDriverDescription[];
  speed?: number;
  looping?: boolean;
}
export type AnimationTransitionEasing = "linear" | "smoothstep";
export type AnimationTransitionStartTime =
  | { policy: "restart" | "preserve" | "matchPhase" }
  | { policy: "seek"; time: number };
export interface AnimationControllerTransition {
  /** Numeric, quaternion, and pose drivers may transition; discrete or structural tracks reject. */
  description: AnimationControllerDescription;
  duration: number;
  easing?: AnimationTransitionEasing;
  startTime?: AnimationTransitionStartTime;
}
export interface AnimationControllerTransitionState {
  duration: number;
  elapsed: number;
  easing: AnimationTransitionEasing;
  pending: boolean;
}
export interface AnimationControllerSnapshot extends AnimationControllerState {
  description: AnimationControllerDescription;
  transition?: AnimationControllerTransitionState;
}
export type AnimationControllerCommand =
  | { action: "create"; description: AnimationControllerDescription }
  | {
      action: "update";
      id: bigint;
      description: AnimationControllerDescription;
    }
  | {
      action: "transition";
      id: bigint;
      transition: AnimationControllerTransition;
    }
  | { action: "delete"; id: bigint }
  | { action: "control"; id: bigint; control: AnimationPlaybackControl };
export type AnimationPlaybackControl =
  | { action: "play" | "pause" | "stop" | "restart" }
  | { action: "seek"; time: number }
  | { action: "playAtSpeed"; speed: number };
export interface AnimationPlaybackCommand {
  type: "AnimationPlaybackCommand";
  controller: bigint;
  control: AnimationPlaybackControl;
}
export interface AnimationPlaybackEventPayload {
  controller: AnimationControllerState;
  kind:
    | "started"
    | "paused"
    | "stopped"
    | "completed"
    | "invalidated"
    | "failed";
  reason: string | null;
}
export type AnimationPlaybackEvent = AnimationPlaybackEventPayload &
  EventEnvelope;
export type AnimationValue =
  | Exclude<FieldValue, { kind: "entity" }>
  | { kind: "entity"; value: bigint }
  | { kind: "rotation"; value: readonly [number, number, number, number] }
  | {
      kind: "entityPlacement";
      value: { parent: number | null; before: number | null };
    }
  | { kind: "pose"; value: readonly AnimationJointTransform[] };
export interface AnimationJointTransform {
  translation?: readonly [number, number, number];
  rotation?: readonly [number, number, number, number];
  scale?: readonly [number, number, number];
}
export type AnimationInterpolation =
  | { kind: "step" | "linear" }
  | {
      kind: "bezier";
      time1: number;
      value1: AnimationValue;
      time2: number;
      value2: AnimationValue;
    };
export interface AnimationKeyframe {
  time: number;
  value: AnimationValue;
  interpolation?: AnimationInterpolation;
}
export type AnimationTrack = {
  keys: readonly AnimationKeyframe[];
} & (
  | {
      property:
        | {
            component: number;
            offsets: readonly number[];
            name?: never;
            entityLink?: never;
          }
        | {
            component: number;
            name: string;
            offsets?: never;
            entityLink?: never;
          };
      joints?: never;
      entityLink?: never;
    }
  | { joints: readonly number[]; property?: never; entityLink?: never }
  | {
      property: {
        entityLink: true;
        name?: never;
        component?: never;
        offsets?: never;
      };
      joints?: never;
    }
);
export interface AnimationClipSource {
  duration: number;
  tracks: readonly AnimationTrack[];
}

/** Primitive placement uses entity-local metres and an xyzw quaternion. */
export interface ShapePlacement {
  translation?: readonly [number, number, number];
  rotation?: readonly [number, number, number, number];
  scale?: readonly [number, number, number];
}

/** Shared geometry definition for BoundingGeometry and PickingGeometry. */
export type BoundingShape =
  | {
      type: "box";
      min: readonly [number, number, number];
      max: readonly [number, number, number];
      transform?: ShapePlacement;
    }
  | {
      type: "sphere";
      radius: number;
      center?: readonly [number, number, number];
      transform?: ShapePlacement;
    }
  | {
      type: "pill";
      radius: number;
      start?: readonly [number, number, number];
      end?: readonly [number, number, number];
      joints?: readonly [number, number];
      transform?: ShapePlacement;
    }
  | { type: "compound"; parts: readonly BoundingShape[] };

/** Geometry encoding supplied by a target-generated client module. */
export type GeometryEncoder = (shape: BoundingShape) => Uint8Array<ArrayBuffer>;
