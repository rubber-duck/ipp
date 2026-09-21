import type { DynamicValue } from "./dynamic-properties.js";
import type { SurfaceEdit } from "./surface-types.js";
import type {
  GuiEdit,
  GuiEditBatchOutcome,
  GuiInputCommand,
  GuiInputRoutingOutcome,
  GuiInspectQuery,
  GuiInspectResponse,
  GuiObservationBatch,
  GuiSemanticActionRequest,
  GuiSemanticSnapshotQuery,
  GuiSemanticTree,
  GuiUnhandledObservation,
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
export type LifecycleNotification = EventEnvelope &
  (
    | ({ kind: "change" } & LifecyclePublication)
    | { kind: "overflow"; subscription: bigint; dropped: bigint }
  );
export interface LifecycleSubscription {
  readonly id: bigint;
  /** Releases queued and future observations at an ordered World boundary. */
  unsubscribe(): Promise<void>;
}

/** Target-independent public values shared by generated clients and session handling. */
export type EntityRef =
  | { kind: "handle"; id: bigint }
  | { kind: "alias"; alias: number };
export type FieldValue =
  | { kind: "dynamic"; value: DynamicValue }
  | { kind: "bool"; value: boolean }
  | { kind: "f32"; value: number }
  | { kind: "u32"; value: number }
  | { kind: "u64"; value: bigint }
  | { kind: "entity"; value: EntityRef }
  | { kind: "string"; value: string }
  | { kind: "bytes"; value: Uint8Array<ArrayBuffer> };
export interface FieldWrite {
  offset: number;
  value: FieldValue;
}
export interface EntityMetadata {
  symbolicId: string | null;
  classes: string[];
}
/** Generational core resources; aliases refer only to this batch's earlier operations. */
export type StateOverlayRef =
  | { kind: "handle"; id: bigint }
  | { kind: "alias"; alias: number };
export type EntityOverlayMode = "owned" | "bound";
export type ComponentOverlayMode = "auto" | "bound" | "owned";
export interface StateOverlayAlias {
  alias: number;
  id: bigint;
  kind: "owner" | "entityOverlayBinding" | "componentStateOverlay";
  entity: bigint | null;
}
export interface StateOverlayLifecycleDiagnostic {
  owner: bigint;
  stateOverlay: bigint;
  entity: bigint;
  component: number | null;
  reason: "EntityDeleted" | "ComponentReplaced" | "ComponentRemoved";
}
export const FieldKind = {
  F32: 1,
  Entity: 2,
  U32: 3,
  U64: 4,
  String: 5,
  Bytes: 6,
  Bool: 7,
} as const;
export type FieldKind = (typeof FieldKind)[keyof typeof FieldKind];
export interface FieldDescriptor {
  readonly offset: number;
  readonly kind: FieldKind;
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
      kind: "updateDynamicComponentStateOverlay";
      owner: StateOverlayRef;
      overlay: StateOverlayRef;
      properties: Record<string, DynamicValue>;
      clear: string[];
    }
  | { kind: "create"; alias: number; metadata: EntityMetadata }
  | { kind: "delete"; entity: EntityRef }
  | { kind: "setMetadata"; entity: EntityRef; metadata: EntityMetadata }
  | {
      kind: "insertComponent";
      entity: EntityRef;
      component: number;
      fields: FieldWrite[];
    }
  | {
      kind: "setField";
      entity: EntityRef;
      component: number;
      field: FieldWrite;
    }
  | { kind: "removeComponent"; entity: EntityRef; component: number }
  | { kind: "createStateOverlayOwner"; alias: number }
  | { kind: "releaseStateOverlayOwner"; owner: StateOverlayRef }
  | {
      kind: "attachEntityOverlayBinding";
      owner: StateOverlayRef;
      alias: number;
      symbolicId: string;
      mode: EntityOverlayMode;
    }
  | {
      kind: "releaseEntityOverlayBinding";
      owner: StateOverlayRef;
      binding: StateOverlayRef;
    }
  | {
      kind: "attachComponentStateOverlay";
      owner: StateOverlayRef;
      binding: StateOverlayRef;
      alias: number;
      component: number;
      mode: ComponentOverlayMode;
      fields: FieldWrite[];
    }
  | {
      kind: "updateComponentStateOverlay";
      owner: StateOverlayRef;
      overlay: StateOverlayRef;
      fields: FieldWrite[];
      clear: number[];
    }
  | {
      kind: "releaseComponentStateOverlay";
      owner: StateOverlayRef;
      overlay: StateOverlayRef;
    };

/** Select a live Camera entity at the ordered mutation boundary. */
export interface CameraActivateCommand {
  type: "CameraActivateCommand";
  entity: bigint;
}
/** Camera-local rotation radians, normalized viewport pan, or logarithmic zoom out. */
export type CameraMotion =
  | { kind: "rotate"; yaw: number; pitch: number }
  | { kind: "pan"; x: number; y: number; width: number; height: number }
  | { kind: "zoom"; amount: number };
export interface CameraNavigateCommand {
  type: "CameraNavigateCommand";
  motion: CameraMotion;
}
/** Read current geometry through normalized top-left viewport coordinates. */
export interface GeometryPickQuery {
  type: "GeometryPickQuery";
  x: number;
  y: number;
  width: number;
  height: number;
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
  x: number;
  y: number;
  width: number;
  height: number;
  plane: WorldPlane;
}
export type CameraProjectResultPayload = {
  type: "CameraProjectResultEvent";
  camera: bigint | null;
} & (
  | { ok: true; position: [number, number, number] | null }
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
export type SystemCommand =
  | AnimationPlaybackCommand
  | CameraActivateCommand
  | CameraNavigateCommand
  | RenderStateUpdateCommand;
export interface EventEnvelope {
  session: bigint;
  requestId: bigint;
  tick: bigint;
}
export interface CameraStatePatch {
  activeCamera?: bigint;
}
export interface CameraStateChangedPayload {
  type: "CameraStateChangedEvent";
  changes: CameraStatePatch;
}
export type CameraStateChangedEvent = EventEnvelope & CameraStateChangedPayload;
export type GeometryPickHit = {
  entity: bigint;
  position: [number, number, number];
  distance: number;
  /** Present only when requested; normal is camera forward, not a surface normal. */
  viewPlane?: WorldPlane;
  /** Stable primitive index in the shared geometry definition. */
  part: number;
};
export type GeometryPickResultPayload = {
  type: "GeometryPickResultEvent";
  camera: bigint | null;
} & ({ ok: true; hit: GeometryPickHit | null } | { ok: false; error: string });
export type GeometryPickResultEvent = EventEnvelope & GeometryPickResultPayload;
export interface ClientAssetSource {
  kind: number;
  source: string;
  variant?: number;
}

export type RequestBody =
  | { kind: "surface"; edit: SurfaceEdit }
  | { kind: "gui"; batchId?: bigint; edits: readonly GuiEdit[] }
  | { kind: "guiInspect"; query: GuiInspectQuery }
  | { kind: "guiInput"; input: GuiInputCommand }
  | { kind: "guiSemanticSnapshot"; query: GuiSemanticSnapshotQuery }
  | { kind: "guiSemanticAction"; action: GuiSemanticActionRequest }
  | {
      kind: "subscribeLifecycle";
      subscription: bigint;
      filter: LifecycleFilter;
    }
  | { kind: "unsubscribeLifecycle"; subscription: bigint }
  | { kind: "command"; command: SystemCommand }
  | { kind: "query"; query: SystemQuery }
  | { kind: "beginBatch" }
  | { kind: "batchChunk"; batch: { id: bigint; operations: Command[] } }
  | { kind: "endBatch"; batchId: bigint }
  | { kind: "batch"; batch: { id: bigint; operations: Command[] } }
  | { kind: "animationController"; command: AnimationControllerCommand }
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
  stateOverlays: StateOverlayAlias[];
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
export type ComponentFieldValue =
  | boolean
  | number
  | bigint
  | string
  | Uint8Array<ArrayBuffer>;
export interface ComponentSnapshot {
  component: number;
  properties?: Record<string, DynamicValue>;
  fields: Record<string, ComponentFieldValue>;
}
export interface EntitySnapshot {
  id: bigint;
  metadata: EntityMetadata;
  base: ComponentSnapshot[];
  effective: ComponentSnapshot[];
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
  collection:
    | "summary"
    | "entities"
    | "resources"
    | "controllers"
    | "renderDiagnostics";
  after?: bigint;
  target?: bigint;
  limit?: number;
}
export interface InspectionPage extends Inspection {
  next: bigint;
}

export interface Inspection {
  tick: bigint;
  time: number;
  entities: EntitySnapshot[];
  resources: readonly AssetResourceSnapshot[];
  renderDiagnostics: readonly RenderDiagnostic[];
  controllers?: readonly AnimationControllerSnapshot[];
}
export interface RuntimeFailure {
  scope: "draw" | "resource" | "context" | "world";
  faulted: boolean;
  message: string;
}

export type ResponseBody =
  | { kind: "surface" }
  | { kind: "gui"; outcome: GuiEditBatchOutcome }
  | { kind: "guiInspect"; response: GuiInspectResponse }
  | { kind: "guiInput"; outcome: GuiInputRoutingOutcome }
  | { kind: "guiSemanticSnapshot"; snapshot: GuiSemanticTree }
  | { kind: "guiObservations"; observations: GuiObservationBatch }
  | { kind: "guiUnhandledInputs"; inputs: GuiUnhandledObservation[] }
  | ({ kind: "runtimeFailure" } & RuntimeFailure)
  | { kind: "lifecycleSubscription" }
  | { kind: "lifecycleEvents"; events: LifecyclePublication[] }
  | { kind: "lifecycleOverflow"; dropped: bigint }
  | { kind: "animationController"; id: bigint | null }
  | { kind: "playback"; events: readonly AnimationPlaybackEventPayload[] }
  | {
      kind: "event";
      event:
        | CameraStateChangedPayload
        | GeometryPickResultPayload
        | CameraProjectResultPayload
        | RenderStateUpdatedPayload;
    }
  | { kind: "batchStarted"; batchId: bigint }
  | { kind: "batch"; outcome: BatchOutcome }
  | { kind: "batchFinished"; batchId: bigint }
  | { kind: "batchAborted"; batchId: bigint; message: string }
  | { kind: "frame"; time: number }
  | { kind: "resources"; resources: readonly AssetResourceSnapshot[] }
  | { kind: "lifecycle"; diagnostics: StateOverlayLifecycleDiagnostic[] }
  | {
      kind: "inspect";
      next: bigint;
      time: number;
      entities: EntitySnapshot[];
      resources: readonly AssetResourceSnapshot[];
      renderDiagnostics: readonly RenderDiagnostic[];
      controllers?: readonly AnimationControllerSnapshot[];
    }
  | { kind: "error"; code: number; message: string };
export interface Response {
  session: bigint;
  requestId: bigint;
  tick: bigint;
  body: ResponseBody;
}

export type { FrameCapture, ClientPresentation } from "./presentation.js";

/** World-local controller state, observed at the inspection/event tick. */
export interface AnimationControllerState {
  id: bigint;
  state: "stopped" | "playing" | "paused" | "completed";
  time: number;
}
export type AnimationDriverTarget =
  | { component: number; name: string; offsets?: never; joints?: never }
  | {
      component: number;
      offsets: readonly number[];
      name?: never;
      joints?: never;
    }
  | {
      joints: readonly number[];
      component?: never;
      offsets?: never;
      name?: never;
    };
export interface AnimationDriverDescription {
  source: string;
  variant?: number;
  track: number;
  target: bigint;
  property: AnimationDriverTarget;
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
        | { component: number; offsets: readonly number[]; name?: never }
        | { component: number; name: string; offsets?: never };
      joints?: never;
    }
  | { joints: readonly number[]; property?: never }
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
