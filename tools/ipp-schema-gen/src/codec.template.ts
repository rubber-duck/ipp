import {
  encodeDynamicValue,
  decodeDynamicValue,
  decodeDynamicDescriptors,
} from "./dynamic-properties.js";
export {
  DynamicProperty,
  f32,
  i32,
  u32,
  bool,
  vec2,
  vec3,
  vec4,
  mat2,
  mat3,
  mat4,
  asset,
  texture2D,
  shaderParameterKind,
  encodeShaderDefinition,
} from "./dynamic-properties.js";
import type { WorldDescriptor } from "./host-client.js";
import { WorldPersistenceHostClient } from "./world-persistence-client.js";
export type {
  WorldLoadOptions,
  WorldTransferOptions,
} from "./world-persistence-client.js";
export type {
  WorldDescriptor,
  WorldSelector,
  WorldCreateOptions,
  WorldCapacityHints,
  WorldCapacityHintsPatch,
} from "./host-client.js";
import {
  ClientBase,
  // #if gui
  RequestRejectedError,
  // #endif
  validateOptions,
  type ConnectOptions,
  type WorkerConnectOptions,
} from "./client.js";
import { webSocketTransport, type MessageTransport } from "./transport.js";
import { workerTransport } from "./worker.js";
import type {
  ClientAssetSource,
  SystemCommand,
  AnimationPlaybackControl,
  AnimationPlaybackEvent,
  AnimationPlaybackEventPayload,
  AnimationControllerState,
  AnimationControllerSnapshot,
  AnimationControllerDescription,
  AnimationControllerTransition,
  AnimationControllerTransitionState,
  AnimationDriverDescription,
  AnimationDriverTarget,
  AnimationClipSource,
  AnimationValue,
  CameraMotion,
  CameraStateChangedEvent,
  CameraStateChangedPayload,
  GeometryPickQuery,
  CameraProjectQuery,
  CameraProjectResultEvent,
  CameraProjectResultPayload,
  SystemQuery,
  SystemQueryResult,
  GeometryPickResultPayload,
  GeometryPickResultEvent,
  GeometryPickHit,
  RenderStatePatch,
  RenderStateUpdatedEvent,
  RenderStateUpdatedPayload,
  LifecycleObservation,
  LifecyclePublication,
  EntityRef,
  FieldWrite,
  EntityMetadata,
  Command,
  Request,
  BatchOutcome,
  ComponentSnapshot,
  EntitySnapshot,
  AssetResourceSnapshot,
  RenderDiagnostic,
  Response,
  ResponseBody,
  StateOverlayAlias,
  ComponentDescriptor,
  StateOverlayLifecycleDiagnostic,
} from "./types.js";
export type * from "./types.js";
export type {
  Client,
  ConnectOptions,
  WorkerConnectOptions,
  ResourceUrlMapping,
  LogLevel,
} from "./client.js";
export { RequestNotSentError, RequestRejectedError } from "./client.js";

export const MAX_MESSAGE_BYTES = 1_048_576;
/** Entity references and command builders; submit commands through client.batch(). */
export const Entity = {
  handle(id: bigint): EntityRef {
    return { kind: "handle", id };
  },
  alias(alias: number): EntityRef {
    return { kind: "alias", alias };
  },
  create(alias: number, metadata: Partial<EntityMetadata> = {}): Command {
    return {
      kind: "create",
      alias,
      metadata: {
        symbolicId: metadata.symbolicId ?? null,
        classes: metadata.classes ?? [],
      },
    };
  },
  delete(entity: EntityRef): Command {
    return { kind: "delete", entity };
  },
  /** Replace the complete metadata value; this is not a partial patch. */
  setMetadata(entity: EntityRef, metadata: EntityMetadata): Command {
    return { kind: "setMetadata", entity, metadata };
  },
};

export class ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}
function fail(message: string): never {
  throw new ProtocolError(message);
}
function uint(value: number, max: number): number {
  if (!Number.isInteger(value) || value < 0 || value > max)
    fail("integer out of range");
  return value;
}
class Writer {
  private bytes = new Uint8Array(128);
  private at = 0;

  constructor(private maxBytes = MAX_MESSAGE_BYTES) {}

  private reserve(n: number): DataView {
    const end = this.at + n;
    if (!Number.isSafeInteger(end) || end > this.maxBytes)
      fail("message limit");
    if (end > this.bytes.length) {
      const next = new Uint8Array(
        Math.min(this.maxBytes, Math.max(end, this.bytes.length * 2)),
      );
      next.set(this.bytes);
      this.bytes = next;
    }
    return new DataView(this.bytes.buffer, this.at, n);
  }
  u8(v: number): void {
    this.reserve(1).setUint8(0, uint(v, 255));
    this.at++;
  }
  boolean(v: boolean): void {
    if (typeof v !== "boolean") fail("boolean value required");
    this.u8(v ? 1 : 0);
  }
  u16(v: number): void {
    this.reserve(2).setUint16(0, uint(v, 65535), true);
    this.at += 2;
  }
  u32(v: number): void {
    this.reserve(4).setUint32(0, uint(v, 0xffffffff), true);
    this.at += 4;
  }
  u64(v: bigint): void {
    if (typeof v !== "bigint" || v < 0n || v > 0xffffffffffffffffn)
      fail("u64 out of range");
    this.reserve(8).setBigUint64(0, v, true);
    this.at += 8;
  }
  f32(v: number): void {
    if (!Number.isFinite(v) || !Number.isFinite(Math.fround(v)))
      fail("nonfinite f32");
    this.reserve(4).setFloat32(0, v, true);
    this.at += 4;
  }
  f64(v: number): void {
    if (!Number.isFinite(v) || v < 0) fail("invalid animation time");
    this.reserve(8).setFloat64(0, v, true);
    this.at += 8;
  }
  count(n: number, max: number): void {
    this.u32(uint(n, max));
  }
  raw(bytes: Uint8Array): void {
    this.reserve(bytes.length);
    this.bytes.set(bytes, this.at);
    this.at += bytes.length;
  }
  string(s: string, max = 65536): void {
    if (typeof s !== "string" || s.length > max) fail("string limit");
    // Reject lone surrogates instead of silently replacing authoring metadata.
    for (let i = 0; i < s.length; i++) {
      const code = s.charCodeAt(i);
      if (code >= 0xd800 && code <= 0xdbff) {
        const low = s.charCodeAt(++i);
        if (!(low >= 0xdc00 && low <= 0xdfff)) fail("invalid UTF-16");
      } else if (code >= 0xdc00 && code <= 0xdfff) fail("invalid UTF-16");
    }
    const bytes = new TextEncoder().encode(s);
    this.count(bytes.length, max);
    this.raw(bytes);
  }
  finish(): Uint8Array<ArrayBuffer> {
    return this.bytes.slice(0, this.at);
  }
}
class Reader {
  private at = 0;
  constructor(private bytes: Uint8Array) {
    if (bytes.byteLength > MAX_MESSAGE_BYTES) fail("message limit");
  }
  private take(n: number): DataView {
    if (this.at + n > this.bytes.length) fail("truncated message");
    const view = new DataView(
      this.bytes.buffer,
      this.bytes.byteOffset + this.at,
      n,
    );
    this.at += n;
    return view;
  }
  u8(): number {
    return this.take(1).getUint8(0);
  }
  boolean(): boolean {
    const value = this.u8();
    if (value > 1) fail("boolean encoding");
    return value === 1;
  }
  u16(): number {
    return this.take(2).getUint16(0, true);
  }
  u32(): number {
    return this.take(4).getUint32(0, true);
  }
  u64(): bigint {
    return this.take(8).getBigUint64(0, true);
  }
  f32(): number {
    const v = this.take(4).getFloat32(0, true);
    if (!Number.isFinite(v)) fail("nonfinite f32");
    return v;
  }
  f64(): number {
    const v = this.take(8).getFloat64(0, true);
    if (!Number.isFinite(v)) fail("nonfinite f64");
    return v;
  }
  count(max: number): number {
    const n = this.u32();
    if (n > max) fail("count limit");
    return n;
  }
  string(max = 65536): string {
    const n = this.count(max);
    const v = this.take(n);
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(v);
    } catch {
      return fail("invalid UTF-8");
    }
  }
  raw(n: number): Uint8Array<ArrayBuffer> {
    const view = this.take(n);
    return new Uint8Array(
      view.buffer,
      view.byteOffset,
      view.byteLength,
    ).slice();
  }
  done(): void {
    if (this.at !== this.bytes.length) fail("trailing bytes");
  }
}
export function bootstrap(): Uint8Array<ArrayBuffer> {
  const w = new Writer();
  w.raw(new Uint8Array([73, 80, 80, 66]));
  w.u32(PROTOCOL_VERSION);
  w.u64(SCHEMA_HASH);
  return w.finish();
}
export function acceptBootstrap(bytes: Uint8Array): bigint {
  if (bytes.length !== 24) fail("bootstrap reply length");
  const expected = bootstrap();
  for (let i = 0; i < 16; i++)
    if (bytes[i] !== expected[i]) fail("bootstrap compatibility mismatch");
  const session = new DataView(
    bytes.buffer,
    bytes.byteOffset + 16,
    8,
  ).getBigUint64(0, true);
  if (session === 0n) fail("invalid session");
  return session;
}
function writeRef(w: Writer, entity: EntityRef): void {
  switch (entity.kind) {
    case "handle":
      w.u8(WIRE.REF_HANDLE);
      w.u64(entity.id);
      break;
    case "alias":
      w.u8(WIRE.REF_ALIAS);
      w.u32(entity.alias);
      break;
    default:
      fail("unsupported reference");
  }
}
function writeMetadata(w: Writer, m: EntityMetadata): void {
  if (m.symbolicId === null) w.u8(WIRE.OPTION_NONE);
  else {
    w.u8(WIRE.OPTION_SOME);
    w.string(m.symbolicId);
  }
  w.count(m.classes.length, 256);
  for (const c of m.classes) w.string(c);
}
function writeField(w: Writer, f: FieldWrite): void {
  w.u32(f.offset);
  const value = f.value;
  switch (value.kind) {
    case "dynamic": {
      const bytes = encodeDynamicValue(value.value);
      w.u8(WIRE.VALUE_DYNAMIC);
      w.count(bytes.length, 65536);
      w.raw(bytes);
      break;
    }
    case "bool":
      w.u8(WIRE.VALUE_BOOL);
      w.boolean(value.value);
      break;
    case "f32":
      w.u8(WIRE.VALUE_F32);
      w.f32(value.value);
      break;
    case "string":
      w.u8(WIRE.VALUE_STRING);
      w.string(value.value);
      break;
    case "u32":
      w.u8(WIRE.VALUE_U32);
      w.u32(value.value);
      break;
    case "u64":
      w.u8(WIRE.VALUE_U64);
      w.u64(value.value);
      break;
    case "bytes":
      w.u8(WIRE.VALUE_BYTES);
      w.count(value.value.byteLength, 65536);
      w.raw(value.value);
      break;
    case "entity":
      w.u8(WIRE.VALUE_ENTITY);
      writeRef(w, value.value);
      break;
    default:
      fail("unsupported field value");
  }
}
function writeCommand(w: Writer, c: Command): void {
  switch (c.kind) {
    case "setDynamicProperty": {
      w.u8(WIRE.COMMAND_SET_DYNAMIC_PROPERTY);
      writeRef(w, c.entity);
      w.u16(c.component);
      w.string(c.name);
      const bytes = encodeDynamicValue(c.value);
      w.count(bytes.length, 65536);
      w.raw(bytes);
      break;
    }
    case "removeDynamicProperty":
      w.u8(WIRE.COMMAND_REMOVE_DYNAMIC_PROPERTY);
      writeRef(w, c.entity);
      w.u16(c.component);
      w.string(c.name);
      break;
    case "updateDynamicComponentStateOverlay":
      w.u8(WIRE.COMMAND_UPDATE_DYNAMIC_COMPONENT_STATE_OVERLAY);
      writeRef(w, c.owner);
      writeRef(w, c.overlay);
      w.count(Object.keys(c.properties).length, 65536);
      for (const [name, value] of Object.entries(c.properties)) {
        w.string(name);
        const bytes = encodeDynamicValue(value);
        w.count(bytes.length, 65536);
        w.raw(bytes);
      }
      w.count(c.clear.length, 65536);
      for (const name of c.clear) w.string(name);
      break;
    case "create":
      w.u8(WIRE.COMMAND_CREATE);
      w.u32(c.alias);
      writeMetadata(w, c.metadata);
      break;
    case "delete":
      w.u8(WIRE.COMMAND_DELETE);
      writeRef(w, c.entity);
      break;
    case "setMetadata":
      w.u8(WIRE.COMMAND_METADATA);
      writeRef(w, c.entity);
      writeMetadata(w, c.metadata);
      break;
    case "insertComponent":
      w.u8(WIRE.COMMAND_INSERT);
      writeRef(w, c.entity);
      w.u16(c.component);
      w.count(c.fields.length, 256);
      for (const f of c.fields) writeField(w, f);
      break;
    case "setField":
      w.u8(WIRE.COMMAND_SET);
      writeRef(w, c.entity);
      w.u16(c.component);
      writeField(w, c.field);
      break;
    case "removeComponent":
      w.u8(WIRE.COMMAND_REMOVE);
      writeRef(w, c.entity);
      w.u16(c.component);
      break;
    case "createStateOverlayOwner":
      w.u8(WIRE.COMMAND_CREATE_STATE_OVERLAY_OWNER);
      w.u32(c.alias);
      break;
    case "releaseStateOverlayOwner":
      w.u8(WIRE.COMMAND_RELEASE_STATE_OVERLAY_OWNER);
      writeRef(w, c.owner);
      break;
    case "attachEntityOverlayBinding":
      w.u8(WIRE.COMMAND_ATTACH_ENTITY_OVERLAY_BINDING);
      writeRef(w, c.owner);
      w.u32(c.alias);
      w.string(c.symbolicId);
      w.u8(
        c.mode === "owned"
          ? WIRE.ENTITY_OVERLAY_MODE_OWNED
          : c.mode === "bound"
            ? WIRE.ENTITY_OVERLAY_MODE_BOUND
            : fail("entity mode"),
      );
      break;
    case "releaseEntityOverlayBinding":
      w.u8(WIRE.COMMAND_RELEASE_ENTITY_OVERLAY_BINDING);
      writeRef(w, c.owner);
      writeRef(w, c.binding);
      break;
    case "attachComponentStateOverlay":
      w.u8(WIRE.COMMAND_ATTACH_COMPONENT_STATE_OVERLAY);
      writeRef(w, c.owner);
      writeRef(w, c.binding);
      w.u32(c.alias);
      w.u16(c.component);
      w.u8(
        c.mode === "auto"
          ? WIRE.COMPONENT_OVERLAY_MODE_AUTO
          : c.mode === "bound"
            ? WIRE.COMPONENT_OVERLAY_MODE_BOUND
            : c.mode === "owned"
              ? WIRE.COMPONENT_OVERLAY_MODE_OWNED
              : fail("component mode"),
      );
      w.count(c.fields.length, 256);
      for (const field of c.fields) writeField(w, field);
      break;
    case "updateComponentStateOverlay":
      w.u8(WIRE.COMMAND_UPDATE_COMPONENT_STATE_OVERLAY);
      writeRef(w, c.owner);
      writeRef(w, c.overlay);
      w.count(c.fields.length, 256);
      for (const field of c.fields) writeField(w, field);
      w.count(c.clear.length, 256);
      for (const offset of c.clear) w.u32(offset);
      break;
    case "releaseComponentStateOverlay":
      w.u8(WIRE.COMMAND_RELEASE_COMPONENT_STATE_OVERLAY);
      writeRef(w, c.owner);
      writeRef(w, c.overlay);
      break;
    default:
      fail("unsupported command");
  }
}
export function encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
  const w = new Writer(
    request.body.kind === "batch" || request.body.kind === "batchChunk"
      ? 128 * 1024
      : MAX_MESSAGE_BYTES,
  );
  if (request.session === 0n) fail("invalid session");
  if ((request.body.kind === "command") !== (request.requestId === 0n))
    fail("reserved request identity");
  w.u64(request.session);
  w.u64(request.requestId);
  const body = request.body;
  switch (body.kind) {
    // #if surfaces
    case "surface": {
      const bytes = encodeSurfaceEdit(body.edit);
      w.u8(WIRE.REQUEST_SURFACE);
      w.count(bytes.length, 65536);
      w.raw(bytes);
      break;
    }
    // #endif
    // #if gui
    case "gui": {
      const bytes = encodeGuiEdits(body.edits);
      w.u8(WIRE.REQUEST_GUI);
      w.boolean(body.batchId !== undefined);
      if (body.batchId !== undefined) w.u64(body.batchId);
      w.count(bytes.length, 1048576);
      w.raw(bytes);
      break;
    }
    case "guiInspect": {
      const bytes = encodeGuiInspectQuery(body.query);
      w.u8(WIRE.REQUEST_GUI_INSPECT);
      w.count(bytes.length, 65536);
      w.raw(bytes);
      break;
    }
    case "guiInput": {
      const bytes = encodeGuiInput(body.input);
      w.u8(WIRE.REQUEST_GUI_INPUT);
      w.count(bytes.length, 1048576);
      w.raw(bytes);
      break;
    }
    case "guiSemanticSnapshot": {
      const bytes = encodeGuiSemanticSnapshotQuery(body.query);
      w.u8(WIRE.REQUEST_GUI_SEMANTIC_SNAPSHOT);
      w.count(bytes.length, 65536);
      w.raw(bytes);
      break;
    }
    case "guiSemanticAction": {
      const bytes = encodeGuiSemanticAction(body.action);
      w.u8(WIRE.REQUEST_GUI_SEMANTIC_ACTION);
      w.count(bytes.length, 1048576);
      w.raw(bytes);
      break;
    }
    // #endif
    case "beginBatch":
      w.u8(WIRE.REQUEST_BEGIN_BATCH);
      break;
    case "batchChunk":
    case "batch":
      w.u8(
        body.kind === "batchChunk"
          ? WIRE.REQUEST_BATCH_CHUNK
          : WIRE.REQUEST_BATCH,
      );
      w.u64(body.batch.id);
      w.count(body.batch.operations.length, 256);
      for (const c of body.batch.operations) writeCommand(w, c);
      break;
    case "endBatch":
      w.u8(WIRE.REQUEST_END_BATCH);
      w.u64(body.batchId);
      break;
    case "subscribeLifecycle": {
      if (body.subscription === 0n) fail("zero lifecycle subscription");
      exactFields(body.filter, [
        "entities",
        "components",
        "assets",
        "entity",
        "component",
        "asset",
      ]);
      const filter = body.filter;
      let domains = 0;
      for (const [name, bit] of [
        ["entities", 1],
        ["components", 2],
      ] as const) {
        const selected = filter[name] ?? true;
        if (typeof selected !== "boolean") fail("lifecycle domain flag");
        if (selected) domains |= bit;
      }
      const assets = filter.assets;
      if (assets !== undefined && typeof assets !== "boolean")
        fail("lifecycle asset flag");
      if (assets ?? true) domains |= 4;
      if (domains === 0) fail("empty lifecycle domains");
      if (filter.entity === 0n || filter.component === 0 || filter.asset === 0n)
        fail("zero lifecycle filter identity");
      w.u8(WIRE.REQUEST_LIFECYCLE_SUBSCRIBE);
      w.u64(body.subscription);
      w.u16(domains);
      w.u64(filter.entity ?? 0n);
      w.u16(filter.component ?? 0);
      w.u64(filter.asset ?? 0n);
      break;
    }
    case "unsubscribeLifecycle":
      if (body.subscription === 0n) fail("zero lifecycle subscription");
      w.u8(WIRE.REQUEST_LIFECYCLE_UNSUBSCRIBE);
      w.u64(body.subscription);
      break;
    case "inspect":
      w.u8(WIRE.REQUEST_INSPECT);
      w.u8(
        {
          summary: WIRE.INSPECT_SUMMARY,
          entities: WIRE.INSPECT_ENTITIES,
          resources: WIRE.INSPECT_RESOURCES,
          controllers: WIRE.INSPECT_CONTROLLERS,
          renderDiagnostics: WIRE.INSPECT_RENDER_DIAGNOSTICS,
        }[body.collection] ?? fail("inspection collection"),
      );
      w.u64(body.after ?? 0n);
      w.u64(body.target ?? 0n);
      if (
        (body.limit ?? 256) < 1 ||
        (body.limit ?? 256) > 256 ||
        (body.target && body.after)
      )
        fail("inspection query");
      w.u16(body.limit ?? 256);
      break;
    case "animationController": {
      const command = body.command;
      switch (command.action) {
        case "create":
          exactFields(command, ["action", "description"]);
          w.u8(WIRE.REQUEST_CONTROLLER_CREATE);
          writeControllerDescription(w, command.description);
          break;
        case "update":
          exactFields(command, ["action", "id", "description"]);
          w.u8(WIRE.REQUEST_CONTROLLER_UPDATE);
          writeControllerId(w, command.id);
          writeControllerDescription(w, command.description);
          break;
        case "transition":
          exactFields(command, ["action", "id", "transition"]);
          w.u8(WIRE.REQUEST_CONTROLLER_TRANSITION);
          writeControllerId(w, command.id);
          writeControllerTransition(w, command.transition);
          break;
        case "delete":
          exactFields(command, ["action", "id"]);
          w.u8(WIRE.REQUEST_CONTROLLER_DELETE);
          writeControllerId(w, command.id);
          break;
        case "control":
          exactFields(command, ["action", "id", "control"]);
          w.u8(WIRE.REQUEST_CONTROLLER_CONTROL);
          writeControllerId(w, command.id);
          writePlaybackControl(w, command.control);
          break;
        default:
          fail("animation controller operation");
      }
      break;
    }
    case "command": {
      const command = body.command;
      if (command.type === "AnimationPlaybackCommand") {
        exactFields(command, ["type", "controller", "control"]);
        w.u8(WIRE.REQUEST_PLAYBACK);
        writeControllerId(w, command.controller);
        writePlaybackControl(w, command.control);
        break;
      }
      if (command.type === "CameraActivateCommand") {
        exactFields(command, ["type", "entity"]);
        w.u8(WIRE.REQUEST_CAMERA_ACTIVATE);
        w.u64(command.entity);
        break;
      }
      if (command.type === "CameraNavigateCommand") {
        exactFields(command, ["type", "motion"]);
        w.u8(WIRE.REQUEST_CAMERA_NAVIGATE);
        writeCameraMotion(w, command.motion);
        break;
      }
      if (command.type === "RenderStateUpdateCommand") {
        exactFields(command, ["type", "changes"]);
        w.u8(WIRE.REQUEST_RENDER_STATE_UPDATE);
        writeRenderStatePatch(w, command.changes);
        break;
      }
      fail("unsupported command");
    }
    case "query": {
      const query = body.query;
      if (query.type === "CameraProjectQuery") {
        exactFields(query, ["type", "x", "y", "width", "height", "plane"]);
        exactFields(query.plane, ["point", "normal"]);
        w.u8(WIRE.REQUEST_CAMERA_PROJECT);
        w.f32(query.x);
        w.f32(query.y);
        w.u32(query.width);
        w.u32(query.height);
        for (const vector of [query.plane.point, query.plane.normal]) {
          if (!Array.isArray(vector) || vector.length !== 3)
            fail("plane requires three coordinates");
          for (const value of vector) w.f32(value);
        }
        break;
      }
      if (query.type !== "GeometryPickQuery") fail("unsupported query");
      exactFields(query, [
        "type",
        "x",
        "y",
        "width",
        "height",
        "includeViewPlane",
      ]);
      w.u8(WIRE.REQUEST_GEOMETRY_PICK);
      w.f32(query.x);
      w.f32(query.y);
      w.u32(query.width);
      w.u32(query.height);
      w.boolean(
        query.includeViewPlane === undefined ? false : query.includeViewPlane,
      );
      break;
    }
    default:
      fail("unsupported request");
  }
  return w.finish();
}
function readMetadata(r: Reader): EntityMetadata {
  const tag = r.u8();
  let symbolicId: string | null;
  if (tag === WIRE.OPTION_NONE) symbolicId = null;
  else if (tag === WIRE.OPTION_SOME) symbolicId = r.string();
  else return fail("invalid option tag");
  const n = r.count(256);
  const classes: string[] = [];
  for (let i = 0; i < n; i++) classes.push(r.string());
  return { symbolicId, classes };
}
function readOutcome(r: Reader): BatchOutcome {
  const batchId = r.u64();
  const tick = r.u64();
  const tag = r.u8();
  if (tag !== WIRE.OUTCOME_SUCCESS && tag !== WIRE.OUTCOME_FAILURE)
    return fail("unsupported outcome");
  let error:
    | {
        scope: "operation" | "commit";
        operation: number | null;
        reason: string;
      }
    | undefined;
  if (tag === WIRE.OUTCOME_FAILURE) {
    const scopeTag = r.u8();
    const scope =
      scopeTag === WIRE.BATCH_ERROR_OPERATION
        ? "operation"
        : scopeTag === WIRE.BATCH_ERROR_COMMIT
          ? "commit"
          : fail("invalid batch error scope");
    const option = r.u8();
    const operation =
      option === WIRE.OPTION_NONE
        ? null
        : option === WIRE.OPTION_SOME
          ? r.u32()
          : fail("invalid operation option");
    error = { scope, operation, reason: r.string() };
  }
  {
    const n = r.count(4096);
    const aliases: { alias: number; id: bigint }[] = [];
    for (let i = 0; i < n; i++) aliases.push({ alias: r.u32(), id: r.u64() });
    const stateOverlays: StateOverlayAlias[] = [];
    const resourceCount = r.count(4096);
    for (let i = 0; i < resourceCount; i++) {
      const alias = r.u32();
      const id = r.u64();
      const tag = r.u8();
      const kind =
        tag === WIRE.STATE_OVERLAY_KIND_OWNER
          ? "owner"
          : tag === WIRE.STATE_OVERLAY_KIND_ENTITY_BINDING
            ? "entityOverlayBinding"
            : tag === WIRE.STATE_OVERLAY_KIND_COMPONENT
              ? "componentStateOverlay"
              : fail("resource kind");
      const option = r.u8();
      const entity =
        option === WIRE.OPTION_NONE
          ? null
          : option === WIRE.OPTION_SOME
            ? r.u64()
            : fail("resource entity option");
      if (id === 0n || (kind === "owner") !== (entity === null))
        fail("invalid resource acknowledgement");
      stateOverlays.push({ alias, id, kind, entity });
    }
    return error
      ? { batchId, tick, ok: false, error, aliases, stateOverlays }
      : { batchId, tick, ok: true, aliases, stateOverlays };
  }
}
const descriptors: readonly ComponentDescriptor[] = Object.values(components);
function readComponent(r: Reader): ComponentSnapshot {
  const component = r.u16();
  const descriptor = descriptors.find((c) => c.id === component);
  if (!descriptor) return fail("unsupported component");
  const n = r.count(65536);
  const entries = Object.entries(descriptor.fields);
  if (
    descriptor.dynamicProperties ? n < entries.length + 1 : n !== entries.length
  )
    fail("incomplete component inspection");
  let dynamicDescriptors:
    | ReturnType<typeof decodeDynamicDescriptors>
    | undefined;
  const properties: Record<
    string,
    import("./dynamic-properties.js").DynamicValue
  > = Object.create(null);
  const fields: Record<
    string,
    boolean | number | bigint | string | Uint8Array<ArrayBuffer>
  > = Object.create(null) as Record<
    string,
    boolean | number | bigint | string | Uint8Array<ArrayBuffer>
  >;
  for (let i = 0; i < n; i++) {
    const offset = r.u32();
    const kind = r.u8();
    if (offset >= 0x80000000 && descriptor.dynamicProperties) {
      if (
        offset === 0x80000000 &&
        kind === WIRE.SNAPSHOT_VALUE_BYTES &&
        !dynamicDescriptors
      ) {
        dynamicDescriptors = decodeDynamicDescriptors(r.raw(r.count(65536)));
        continue;
      }
      const property = dynamicDescriptors?.get(offset);
      if (
        !property ||
        kind !== WIRE.SNAPSHOT_VALUE_DYNAMIC ||
        Object.hasOwn(properties, property.name)
      )
        fail("invalid dynamic inspection");
      const value = decodeDynamicValue(r.raw(r.count(65536)));
      if (value.kind !== property!.kind)
        fail("dynamic inspection type mismatch");
      properties[property!.name] = value;
      continue;
    }
    const entry = entries.find(
      ([, f]) => f.offset === offset && f.kind === kind,
    );
    if (!entry || Object.hasOwn(fields, entry[0]))
      return fail("invalid inspected field");
    if (kind === WIRE.SNAPSHOT_VALUE_BOOL) fields[entry[0]] = r.boolean();
    else if (kind === WIRE.SNAPSHOT_VALUE_F32) fields[entry[0]] = r.f32();
    else if (kind === WIRE.SNAPSHOT_VALUE_U32) fields[entry[0]] = r.u32();
    else if (kind === WIRE.SNAPSHOT_VALUE_U64) fields[entry[0]] = r.u64();
    else if (kind === WIRE.SNAPSHOT_VALUE_STRING) fields[entry[0]] = r.string();
    else if (kind === WIRE.SNAPSHOT_VALUE_BYTES)
      fields[entry[0]] = r.raw(r.count(65536));
    else if (kind === WIRE.SNAPSHOT_VALUE_ENTITY) {
      if (r.u8() !== WIRE.SNAPSHOT_REF_HANDLE)
        fail("unresolved inspected alias");
      fields[entry[0]] = r.u64();
    } else return fail("unsupported inspected value");
  }
  if (
    Object.keys(fields).length !== entries.length ||
    (descriptor.dynamicProperties &&
      (!dynamicDescriptors ||
        dynamicDescriptors.size !== Object.keys(properties).length))
  )
    fail("incomplete component inspection");
  return descriptor.dynamicProperties
    ? { component, fields, properties }
    : { component, fields };
}
function readDiagnostics(r: Reader): StateOverlayLifecycleDiagnostic[] {
  const n = r.count(16384);
  const diagnostics: StateOverlayLifecycleDiagnostic[] = [];
  for (let i = 0; i < n; i++) {
    const owner = r.u64();
    const stateOverlay = r.u64();
    const entity = r.u64();
    const option = r.u8();
    const component =
      option === WIRE.OPTION_NONE
        ? null
        : option === WIRE.OPTION_SOME
          ? r.u16()
          : fail("diagnostic component option");
    const tag = r.u8();
    const reason =
      tag === WIRE.STATE_OVERLAY_ENTITY_DELETED
        ? "EntityDeleted"
        : tag === WIRE.STATE_OVERLAY_COMPONENT_REPLACED
          ? "ComponentReplaced"
          : tag === WIRE.STATE_OVERLAY_COMPONENT_REMOVED
            ? "ComponentRemoved"
            : fail("lifecycle reason");
    if (owner === 0n || stateOverlay === 0n || entity === 0n)
      fail("invalid diagnostic identity");
    diagnostics.push({ owner, stateOverlay, entity, component, reason });
  }
  return diagnostics;
}
function readResources(
  r: Reader,
  count: number,
  events = false,
): AssetResourceSnapshot[] {
  const resources: AssetResourceSnapshot[] = [];
  const ids = new Set<string>();
  for (let i = 0; i < count; i++) {
    const id = r.u64();
    const kind = r.u16();
    const source = r.string();
    const variant = r.u32();
    const tag = r.u8();
    const status =
      tag === WIRE.RESOURCE_UNLOADED
        ? "unloaded"
        : tag === WIRE.RESOURCE_START
          ? "start"
          : tag === WIRE.RESOURCE_PROGRESS
            ? "progress"
            : tag === WIRE.RESOURCE_LOADED
              ? "loaded"
              : tag === WIRE.RESOURCE_FAILED
                ? "failed"
                : undefined;
    if (id === 0n || kind === 0 || source.length === 0 || status === undefined)
      fail("resource identity or status");
    const identity = `${kind}/${id}/${variant}`;
    if (!events && (id === 0n || ids.has(identity))) fail("resource identity");
    ids.add(identity);
    const resource: AssetResourceSnapshot = {
      id,
      kind,
      source,
      variant,
      status,
      representation: {
        decoded: false,
        graphicsReady: null,
        sourceBytes: 0n,
        residentBytes: 0n,
        graphicsBytes: null,
      },
    };
    if (status === "progress") {
      resource.completed = r.u64();
      const hasTotal = r.u8();
      if (hasTotal !== WIRE.OPTION_NONE && hasTotal !== WIRE.OPTION_SOME)
        fail("invalid progress total");
      if (hasTotal === WIRE.OPTION_SOME) {
        resource.total = r.u64();
        if (resource.total < resource.completed) fail("invalid progress");
      }
    }
    if (status === "failed")
      resource.error = r.string(
        WIRE_LAYOUTS["resource-status-failed"].fields[1].limit,
      );
    const boolean = () => {
      const value = r.u8();
      if (value > 1) fail("representation boolean");
      return value === 1;
    };
    const optional = <T>(read: () => T): T | null => {
      const tag = r.u8();
      if (tag === WIRE.OPTION_NONE) return null;
      if (tag !== WIRE.OPTION_SOME) fail("representation option");
      return read();
    };
    resource.representation = {
      decoded: boolean(),
      graphicsReady: optional(boolean),
      sourceBytes: r.u64(),
      residentBytes: r.u64(),
      graphicsBytes: optional(() => r.u64()),
    };
    resources.push(resource);
  }
  return resources;
}
function exactFields(value: object, fields: string[]): void {
  if (
    value === null ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Reflect.ownKeys(value).some(
      (key) => typeof key !== "string" || !fields.includes(key),
    )
  )
    fail("unknown system input field");
}
function writeCameraMotion(w: Writer, motion: CameraMotion): void {
  if (motion === null || typeof motion !== "object")
    fail("camera motion required");
  switch (motion.kind) {
    case "rotate":
      exactFields(motion, ["kind", "yaw", "pitch"]);
      w.u8(WIRE.CAMERA_MOTION_ROTATE);
      w.f32(motion.yaw);
      w.f32(motion.pitch);
      break;
    case "pan":
      exactFields(motion, ["kind", "x", "y", "width", "height"]);
      w.u8(WIRE.CAMERA_MOTION_PAN);
      w.f32(motion.x);
      w.f32(motion.y);
      w.u32(motion.width);
      w.u32(motion.height);
      break;
    case "zoom":
      exactFields(motion, ["kind", "amount"]);
      w.u8(WIRE.CAMERA_MOTION_ZOOM);
      w.f32(motion.amount);
      break;
    default:
      fail("unsupported camera motion");
  }
}
function readCameraState(r: Reader): CameraStateChangedPayload {
  const mask = r.u16();
  if (mask !== 1) fail("camera state mask");
  return {
    type: "CameraStateChangedEvent",
    changes: { activeCamera: r.u64() },
  };
}

function readCameraProjection(r: Reader): CameraProjectResultPayload {
  const type = "CameraProjectResultEvent";
  const cameraTag = r.u8();
  const camera =
    cameraTag === WIRE.OPTION_NONE
      ? null
      : cameraTag === WIRE.OPTION_SOME
        ? r.u64()
        : fail("projection camera option");
  const ok = r.boolean();
  const positionTag = r.u8();
  const position: [number, number, number] | null =
    positionTag === WIRE.OPTION_NONE
      ? null
      : positionTag === WIRE.OPTION_SOME
        ? [r.f32(), r.f32(), r.f32()]
        : fail("projection position option");
  const errorTag = r.u8();
  const error =
    errorTag === WIRE.OPTION_NONE
      ? null
      : errorTag === WIRE.OPTION_SOME
        ? r.string()
        : fail("projection error option");
  if (ok) {
    if (camera === null || error !== null)
      fail("invalid successful projection");
    return { type, camera, ok, position };
  }
  if (position !== null || error === null) fail("invalid failed projection");
  return { type, camera, ok, error };
}

function readGeometryResult(r: Reader): GeometryPickResultPayload {
  const type = "GeometryPickResultEvent";
  const cameraTag = r.u8();
  const camera =
    cameraTag === WIRE.OPTION_NONE
      ? null
      : cameraTag === WIRE.OPTION_SOME
        ? r.u64()
        : fail("geometry camera option");
  const tag = r.u8();
  if (tag === WIRE.PICK_OUTCOME_FAILURE)
    return { type, camera, ok: false, error: r.string() };
  if (camera === null) fail("successful geometry result requires camera");
  if (tag === WIRE.PICK_OUTCOME_MISS)
    return { type, camera, ok: true, hit: null };
  if (tag !== WIRE.PICK_OUTCOME_HIT) return fail("geometry result tag");
  const entity = r.u64();
  const position: [number, number, number] = [r.f32(), r.f32(), r.f32()];
  const distance = r.f32();
  if (distance < 0) fail("negative geometry hit distance");
  const hit: GeometryPickHit = { entity, position, distance, part: r.u32() };
  const planeTag = r.u8();
  if (planeTag === WIRE.OPTION_SOME) {
    hit.viewPlane = {
      point: [r.f32(), r.f32(), r.f32()],
      normal: [r.f32(), r.f32(), r.f32()],
    };
  } else if (planeTag !== WIRE.OPTION_NONE) fail("geometry view plane option");
  return { type, camera, ok: true, hit };
}

function writeRenderStatePatch(w: Writer, patch: RenderStatePatch): void {
  if (patch === null || typeof patch !== "object" || Array.isArray(patch))
    fail("render state patch required");
  if (
    Reflect.ownKeys(patch).some(
      (key) =>
        key !== "showAllDebugGeometries" &&
        key !== "debugGeometryColor" &&
        key !== "ambientLight",
    )
  )
    fail("unknown render state field");
  const show = Object.hasOwn(patch, "showAllDebugGeometries");
  const color = Object.hasOwn(patch, "debugGeometryColor");
  const ambient = Object.hasOwn(patch, "ambientLight");
  w.u16((show ? 1 : 0) | (color ? 2 : 0) | (ambient ? 4 : 0));
  if (show) w.boolean(patch.showAllDebugGeometries as boolean);
  if (color) {
    const rgb = patch.debugGeometryColor;
    if (!Array.isArray(rgb) || rgb.length !== 3)
      fail("render state color requires RGB");
    for (const channel of rgb) {
      if (typeof channel !== "number" || channel < 0 || channel > 1)
        fail("render state color range");
      w.f32(channel);
    }
  }
  if (ambient) {
    const rgb = patch.ambientLight;
    if (!Array.isArray(rgb) || rgb.length !== 3)
      fail("ambient light requires RGB");
    for (const channel of rgb) {
      if (typeof channel !== "number" || channel < 0)
        fail("ambient light range");
      w.f32(channel);
    }
  }
}
function readRenderStatePatch(r: Reader): RenderStatePatch {
  const mask = r.u16();
  if ((mask & ~7) !== 0) fail("render state mask");
  const changes: RenderStatePatch = {};
  if (mask & 1) changes.showAllDebugGeometries = r.boolean();
  if (mask & 2) {
    changes.debugGeometryColor = [r.f32(), r.f32(), r.f32()];
    if (changes.debugGeometryColor.some((value) => value < 0 || value > 1))
      fail("render state color range");
  }
  if (mask & 4) {
    changes.ambientLight = [r.f32(), r.f32(), r.f32()];
    if (changes.ambientLight.some((value) => value < 0))
      fail("ambient light range");
  }
  return changes;
}
function readRenderStateChange(r: Reader): RenderStateUpdatedPayload {
  const changes = readRenderStatePatch(r);
  if (Object.keys(changes).length === 0) fail("empty render state change");
  return { type: "RenderStateUpdatedEvent", changes };
}

function readLifecycleEvents(
  r: Reader,
  deliveryTick: bigint,
): LifecyclePublication[] {
  const count = r.count(128);
  if (count === 0) fail("empty lifecycle events");
  const events: LifecyclePublication[] = [];
  for (let index = 0; index < count; index++) {
    const subscription = r.u64();
    const sequence = r.u64();
    const tick = r.u64();
    if (subscription === 0n || sequence === 0n || tick > deliveryTick)
      fail("invalid lifecycle identity or tick");
    const tag = r.u8();
    let observation: LifecycleObservation;
    if (
      tag === WIRE.LIFECYCLE_ENTITY_CREATED ||
      tag === WIRE.LIFECYCLE_ENTITY_METADATA_CHANGED ||
      tag === WIRE.LIFECYCLE_ENTITY_DELETED
    ) {
      const entity = r.u64();
      if (entity === 0n) fail("zero lifecycle entity");
      observation = {
        kind: "entity",
        entity,
        change:
          tag === WIRE.LIFECYCLE_ENTITY_CREATED
            ? "created"
            : tag === WIRE.LIFECYCLE_ENTITY_METADATA_CHANGED
              ? "metadataChanged"
              : "deleted",
      };
    } else if (
      tag === WIRE.LIFECYCLE_COMPONENT_INSERTED ||
      tag === WIRE.LIFECYCLE_COMPONENT_UPDATED ||
      tag === WIRE.LIFECYCLE_COMPONENT_REPLACED ||
      tag === WIRE.LIFECYCLE_COMPONENT_REMOVED
    ) {
      const entity = r.u64();
      const component = r.u16();
      const previous = r.u64();
      const current = r.u64();
      if (entity === 0n || component === 0) fail("invalid lifecycle component");
      observation = {
        kind: "component",
        entity,
        component,
        change:
          tag === WIRE.LIFECYCLE_COMPONENT_INSERTED
            ? "inserted"
            : tag === WIRE.LIFECYCLE_COMPONENT_UPDATED
              ? "updated"
              : tag === WIRE.LIFECYCLE_COMPONENT_REPLACED
                ? "replaced"
                : "removed",
        previousIncarnation: previous || null,
        incarnation: current || null,
      };
    } else if (
      tag === WIRE.LIFECYCLE_ASSET_GRAPHICS_INVALIDATED ||
      tag === WIRE.LIFECYCLE_ASSET_STATUS_CHANGED ||
      tag === WIRE.LIFECYCLE_ASSET_REMOVED
    )
      observation = {
        kind: "asset",
        change:
          tag === WIRE.LIFECYCLE_ASSET_GRAPHICS_INVALIDATED
            ? "graphicsInvalidated"
            : tag === WIRE.LIFECYCLE_ASSET_STATUS_CHANGED
              ? "statusChanged"
              : "removed",
        resource: readResources(r, 1, true)[0]!,
      };
    else fail("unknown lifecycle observation");
    events.push({ subscription, sequence, tick, observation });
  }
  return events;
}

export function decodeResponse(
  bytes: Uint8Array,
  expectedSession: bigint,
): Response {
  const r = new Reader(bytes);
  const session = r.u64();
  if (session === 0n || session !== expectedSession) fail("session mismatch");
  const requestId = r.u64();
  const tick = r.u64();
  const tag = r.u8();
  let unsolicited =
    tag === WIRE.RESPONSE_RUNTIME_FAILURE ||
    tag === WIRE.RESPONSE_FRAME ||
    tag === WIRE.RESPONSE_LIFECYCLE_EVENTS ||
    tag === WIRE.RESPONSE_LIFECYCLE_OVERFLOW;
  if (tag === WIRE.RESPONSE_STATE_OVERLAY_LIFECYCLE) unsolicited = true;
  // #if gui
  if (tag === WIRE.RESPONSE_GUI_OBSERVATIONS) unsolicited = true;
  if (tag === WIRE.RESPONSE_GUI_UNHANDLED) unsolicited = true;
  // #endif
  if (tag === WIRE.RESPONSE_RESOURCES) unsolicited = true;
  if (tag === WIRE.RESPONSE_RENDER_STATE_UPDATED) unsolicited = true;
  if (tag === WIRE.RESPONSE_CAMERA_STATE_CHANGED) unsolicited = true;
  if (tag === WIRE.RESPONSE_PLAYBACK) unsolicited = true;
  if (tag === WIRE.RESPONSE_BATCH_ABORTED) unsolicited = true;
  if (unsolicited !== (requestId === 0n)) fail("reserved response identity");
  let body: ResponseBody;
  if (tag === WIRE.RESPONSE_LIFECYCLE_SUBSCRIPTION)
    body = { kind: "lifecycleSubscription" };
  // #if surfaces
  else if (tag === WIRE.RESPONSE_SURFACE) body = { kind: "surface" };
  // #endif
  // #if gui
  else if (tag === WIRE.RESPONSE_GUI) {
    const applied = r.u32();
    const failed = r.boolean();
    body = {
      kind: "gui",
      outcome: failed
        ? {
            ok: false,
            applied,
            requests: 1,
            error: { kind: "runtime", reason: r.string() },
          }
        : { ok: true, applied, requests: 1 },
    };
  } else if (tag === WIRE.RESPONSE_GUI_INSPECT) {
    const len = r.count(1048576);
    body = {
      kind: "guiInspect",
      response: decodeGuiInspectResponse(r.raw(len)),
    };
  } else if (tag === WIRE.RESPONSE_GUI_INPUT) {
    const routingTick = r.u64();
    const reason = r.u16();
    const blocker = r.boolean() ? r.u64() : undefined;
    let outcome: GuiInputRoutingOutcome;
    if (reason === 0) {
      if (blocker !== undefined) fail("GUI handled input blocker");
      outcome = { tick: routingTick };
    } else {
      let unhandled: GuiInputRoutingOutcome["unhandled"];
      if (reason === 1) unhandled = { kind: "noPanelHit" };
      else if (reason === 2 && blocker !== undefined && blocker !== 0n)
        unhandled = { kind: "blocked", entity: blocker };
      else if (reason === 3) unhandled = { kind: "staleTarget" };
      else if (reason === 4) unhandled = { kind: "noFocus" };
      else if (reason === 5) unhandled = { kind: "noCapture" };
      else if (reason === 6) unhandled = { kind: "notFocusable" };
      else if (reason === 7) unhandled = { kind: "notOwner" };
      else return fail("GUI input routing reason");
      if (reason !== 2 && blocker !== undefined)
        fail("GUI input routing blocker");
      outcome = { tick: routingTick, unhandled };
    }
    body = { kind: "guiInput", outcome };
  } else if (tag === WIRE.RESPONSE_GUI_SEMANTIC_SNAPSHOT) {
    const len = r.count(1048576);
    body = {
      kind: "guiSemanticSnapshot",
      snapshot: decodeGuiSemanticSnapshot(r.raw(len)),
    };
  } else if (tag === WIRE.RESPONSE_GUI_OBSERVATIONS) {
    const len = r.count(1048576);
    body = {
      kind: "guiObservations",
      observations: readGuiObservations(r.raw(len)),
    };
  } else if (tag === WIRE.RESPONSE_GUI_UNHANDLED) {
    const len = r.count(1048576);
    body = {
      kind: "guiUnhandledInputs",
      inputs: readGuiUnhandledInputs(r.raw(len)),
    };
  }
  // #endif
  else if (tag === WIRE.RESPONSE_LIFECYCLE_EVENTS)
    body = { kind: "lifecycleEvents", events: readLifecycleEvents(r, tick) };
  else if (tag === WIRE.RESPONSE_LIFECYCLE_OVERFLOW) {
    const dropped = r.u64();
    if (dropped === 0n) fail("empty lifecycle overflow");
    body = { kind: "lifecycleOverflow", dropped };
  } else if (tag === WIRE.RESPONSE_BATCH)
    body = { kind: "batch", outcome: readOutcome(r) };
  else if (tag === WIRE.RESPONSE_BATCH_STARTED)
    body = { kind: "batchStarted", batchId: r.u64() };
  else if (tag === WIRE.RESPONSE_BATCH_FINISHED)
    body = { kind: "batchFinished", batchId: r.u64() };
  else if (tag === WIRE.RESPONSE_BATCH_ABORTED)
    body = { kind: "batchAborted", batchId: r.u64(), message: r.string() };
  else if (tag === WIRE.RESPONSE_CAMERA_STATE_CHANGED)
    body = { kind: "event", event: readCameraState(r) };
  else if (tag === WIRE.RESPONSE_RESOURCES) {
    const count = r.count(128);
    if (count === 0) fail("empty resource event");
    body = { kind: "resources", resources: readResources(r, count, true) };
  } else if (tag === WIRE.RESPONSE_RENDER_STATE_UPDATED)
    body = { kind: "event", event: readRenderStateChange(r) };
  else if (tag === WIRE.RESPONSE_GEOMETRY_PICK)
    body = { kind: "event", event: readGeometryResult(r) };
  else if (tag === WIRE.RESPONSE_CAMERA_PROJECT)
    body = { kind: "event", event: readCameraProjection(r) };
  else if (tag === WIRE.RESPONSE_STATE_OVERLAY_LIFECYCLE)
    body = { kind: "lifecycle", diagnostics: readDiagnostics(r) };
  else if (tag === WIRE.RESPONSE_CONTROLLER) {
    const id = r.u64();
    body = { kind: "animationController", id: id === 0n ? null : id };
  } else if (tag === WIRE.RESPONSE_PLAYBACK) {
    const count = r.count(1024);
    const events: AnimationPlaybackEventPayload[] = [];
    for (let i = 0; i < count; i++) {
      const controller = readControllerState(r);
      const kind = (
        {
          [WIRE.PLAYBACK_EVENT_STARTED]: "started",
          [WIRE.PLAYBACK_EVENT_PAUSED]: "paused",
          [WIRE.PLAYBACK_EVENT_STOPPED]: "stopped",
          [WIRE.PLAYBACK_EVENT_COMPLETED]: "completed",
          [WIRE.PLAYBACK_EVENT_INVALIDATED]: "invalidated",
          [WIRE.PLAYBACK_EVENT_FAILED]: "failed",
        } as const
      )[r.u32()];
      if (!kind) fail("playback event kind");
      const reason = r.string();
      events.push({ controller, kind, reason: reason || null });
    }
    body = { kind: "playback", events };
  } else if (tag === WIRE.RESPONSE_RUNTIME_FAILURE) {
    const tag = r.u8();
    const scope =
      tag === WIRE.FAILURE_DRAW
        ? "draw"
        : tag === WIRE.FAILURE_RESOURCE
          ? "resource"
          : tag === WIRE.FAILURE_CONTEXT
            ? "context"
            : tag === WIRE.FAILURE_WORLD
              ? "world"
              : fail("invalid runtime failure scope");
    const faulted = r.u8();
    if (faulted > 1) fail("invalid fault state");
    body = {
      kind: "runtimeFailure",
      scope,
      faulted: faulted === 1,
      message: r.string(2048),
    };
  } else if (tag === WIRE.RESPONSE_FRAME) {
    const time = r.f64();
    if (time < 0) fail("negative simulation time");
    body = { kind: "frame", time };
  } else if (tag === WIRE.RESPONSE_INSPECT) {
    const time = r.f64();
    if (time < 0) fail("negative simulation time");
    const next = r.u64();
    const n = r.count(256);
    const entities: EntitySnapshot[] = [];
    for (let i = 0; i < n; i++) {
      const id = r.u64();
      const metadata = readMetadata(r);
      const base: ComponentSnapshot[] = [];
      const effective: ComponentSnapshot[] = [];
      for (const list of [base, effective]) {
        const n = r.count(256);
        for (let j = 0; j < n; j++) list.push(readComponent(r));
      }
      entities.push({ id, metadata, base, effective });
    }
    const count = r.count(CAPABILITIES.assets ? 0xffff_ffff : 0);
    const resources: AssetResourceSnapshot[] = [];
    resources.push(...readResources(r, count));
    if (count !== resources.length) fail("resource count");
    const diagnosticCount = r.count(CAPABILITIES.spatial ? 16384 : 0);
    const renderDiagnostics: RenderDiagnostic[] = [];
    for (let i = 0; i < diagnosticCount; i++) {
      const entity = r.u64();
      if (entity === 0n) fail("render diagnostic entity");
      renderDiagnostics.push({ entity, reason: r.string() });
    }
    if (diagnosticCount !== renderDiagnostics.length)
      fail("render diagnostic count");
    body = {
      kind: "inspect",
      next,
      time,
      entities,
      resources,
      renderDiagnostics,
    };
    const controllerCount = r.count(16384);
    const controllers: AnimationControllerSnapshot[] = [];
    for (let i = 0; i < controllerCount; i++)
      controllers.push({
        ...readControllerState(r),
        description: readControllerDescription(r),
        ...readControllerTransitionState(r),
      });
    body.controllers = controllers;
  } else if (tag === WIRE.RESPONSE_ERROR)
    body = { kind: "error", code: r.u16(), message: r.string() };
  else return fail("unsupported response");
  r.done();
  return { session, requestId, tick, body };
}

/** A concrete client for this generated target and capability selection. */
export class IppClient extends ClientBase {
  private hostConnection!: IppHostClient;

  get host(): IppHostClient {
    return this.hostConnection;
  }

  override readonly schemaHash = SCHEMA_HASH;
  override readonly components = components;
  override readonly capabilities = CAPABILITIES;

  /** Submit an ordered system action without allocating a reply waiter. */
  sendCommand(command: SystemCommand): void {
    this.submitCommand(command);
  }

  onCameraStateChanged(
    listener: (event: CameraStateChangedEvent) => void,
  ): () => void {
    return this.addCameraStateListener(listener);
  }

  query(query: GeometryPickQuery): Promise<GeometryPickResultEvent>;
  query(query: CameraProjectQuery): Promise<CameraProjectResultEvent>;
  query(query: SystemQuery): Promise<SystemQueryResult>;
  query(query: SystemQuery): Promise<SystemQueryResult> {
    return this.submitQuery(query);
  }

  registerAsset(
    resource: ClientAssetSource,
    bytes: ArrayBuffer,
  ): Promise<void> {
    return this.submitClientAsset(resource, bytes);
  }

  releaseAsset(resource: ClientAssetSource): Promise<void> {
    return this.submitClientAsset(resource);
  }

  /** Publish immutable bytes through the provider data plane. */
  createAsset(
    kind: number,
    bytes: ArrayBuffer,
    variant = 0,
  ): Promise<ClientAssetSource> {
    return this.submitNewAsset(kind, bytes, variant);
  }

  /** Observe resource lifecycle changes without polling. */
  onResourceChange(
    listener: (resource: AssetResourceSnapshot) => void,
  ): () => void {
    return this.addResourceListener(listener);
  }

  /** Observe each committed settings change, including runtime-originated updates. */
  onRenderStateUpdated(
    listener: (event: RenderStateUpdatedEvent) => void,
  ): () => void {
    return this.addRenderStateListener(listener);
  }

  encodeAnimationClip(clip: AnimationClipSource): Uint8Array<ArrayBuffer> {
    return encodeAnimationClip(clip);
  }

  // #if surfaces
  /** Apply one ordered item edit and await its committed outcome. */
  async editSurface(edit: SurfaceEdit): Promise<void> {
    await this.submitSurface(edit);
  }

  encodeSurfaceItems(collection: SurfaceCollection): Uint8Array<ArrayBuffer> {
    return encodeSurfaceItems(collection);
  }

  decodeSurfaceItems(bytes: Uint8Array): SurfaceCollection {
    return decodeSurfaceItems(bytes);
  }
  // #endif
  // #if gui
  /** Apply one ordered GUI edit and await its committed outcome. */
  async editGui(edit: GuiEdit): Promise<void> {
    const outcome = await this.editGuiBatch([edit]);
    if (!outcome.ok)
      throw new Error(`GUI edit rejected: ${outcome.error.reason}`);
  }

  /** Apply ordered GUI edits in exact byte-bounded request pages. */
  async editGuiBatch(edits: readonly GuiEdit[]): Promise<GuiEditBatchOutcome> {
    const snapshot = structuredClone(edits);
    const pages = planGuiEditBatches(snapshot);
    const submit = (operation?: symbol) =>
      this.submitGuiPages(pages, operation);
    return await this.queueAutomaticWorldBatch(pages.length === 1, submit);
  }

  /** Apply one GUI buffer under a Host-issued logical batch identity. */
  async editGuiBatchChunk(
    batchId: bigint,
    edits: readonly GuiEdit[],
  ): Promise<GuiEditBatchOutcome> {
    if (batchId === 0n) throw new RangeError("GUI batch identity is zero");
    return await this.submitGui(edits, batchId);
  }

  private async submitGuiPages(
    pages: readonly (readonly GuiEdit[])[],
    automaticOperation?: symbol,
  ): Promise<GuiEditBatchOutcome> {
    let applied = 0;
    let requests = 0;
    if (pages.length === 1) {
      try {
        return await this.submitGui(pages[0]!, undefined, automaticOperation);
      } catch (error) {
        if (error instanceof RequestRejectedError)
          return {
            ok: false,
            applied: 0,
            requests: 1,
            error: { kind: "admission", reason: error.message },
          };
        throw error;
      }
    }

    let batchId: bigint;
    try {
      batchId = await this.beginBatch();
      requests += 1;
    } catch (error) {
      if (error instanceof RequestRejectedError)
        return {
          ok: false,
          applied: 0,
          requests: 1,
          error: { kind: "admission", reason: error.message },
        };
      throw error;
    }
    for (const page of pages) {
      let outcome: GuiEditBatchOutcome;
      try {
        outcome = await this.editGuiBatchChunk(batchId, page);
      } catch (error) {
        if (error instanceof RequestRejectedError)
          return {
            ok: false,
            applied,
            requests: requests + 1,
            error: { kind: "admission", reason: error.message },
          };
        throw error;
      }
      requests += outcome.requests;
      if (
        outcome.applied > page.length ||
        (!outcome.ok && outcome.applied === page.length)
      )
        throw new Error("Invalid GUI applied-prefix acknowledgement");
      applied += outcome.applied;
      if (!outcome.ok) return { ...outcome, applied, requests };
      if (outcome.applied !== page.length)
        throw new Error("Invalid GUI applied-prefix acknowledgement");
    }
    try {
      await this.endBatch(batchId);
      requests += 1;
    } catch (error) {
      if (error instanceof RequestRejectedError)
        return {
          ok: false,
          applied,
          requests: requests + 1,
          error: { kind: "admission", reason: error.message },
        };
      throw error;
    }
    return { ok: true, applied, requests };
  }

  /** Inspect a GUI root or subtree bounded by maxDepth and limit. */
  async inspectGui(query: GuiInspectQuery): Promise<GuiInspectResponse> {
    return await this.submitGuiInspect(query);
  }

  /** Route one ordered GUI input and await its authoritative disposition. */
  override async submitGuiInput(
    input: GuiInputCommand,
  ): Promise<GuiInputRoutingOutcome> {
    return await super.submitGuiInput(input);
  }

  /** Observe a bounded lifetime/revision-fenced semantic snapshot. */
  async semanticSnapshot(
    query: GuiSemanticSnapshotQuery,
  ): Promise<GuiSemanticTree> {
    return await this.submitGuiSemanticSnapshot(query);
  }

  /** Dispatch one semantic action through the validated control policy. */
  async semanticAction(action: GuiSemanticActionRequest): Promise<void> {
    await this.submitGuiSemanticAction(action);
  }

  encodeGuiTree(tree: GuiTree): Uint8Array<ArrayBuffer> {
    return encodeGuiTree(tree);
  }

  decodeGuiTree(bytes: Uint8Array): GuiTree {
    return decodeGuiTree(bytes);
  }
  // #endif

  async createAnimationController(
    description: AnimationControllerDescription,
  ): Promise<bigint> {
    const id = await this.submitAnimationController({
      action: "create",
      description,
    });
    if (id === null) throw new Error("Missing created controller identity");
    return id;
  }

  async updateAnimationController(
    id: bigint,
    description: AnimationControllerDescription,
  ): Promise<void> {
    await this.submitAnimationController({ action: "update", id, description });
  }

  async transitionAnimationController(
    id: bigint,
    transition: AnimationControllerTransition,
  ): Promise<void> {
    await this.submitAnimationController({
      action: "transition",
      id,
      transition,
    });
  }

  async deleteAnimationController(id: bigint): Promise<void> {
    await this.submitAnimationController({ action: "delete", id });
  }

  async controlAnimationController(
    id: bigint,
    control: AnimationPlaybackControl,
  ): Promise<void> {
    await this.submitAnimationController({ action: "control", id, control });
  }

  playback(controller: bigint, control: AnimationPlaybackControl): void {
    this.submitCommand({
      type: "AnimationPlaybackCommand",
      controller,
      control,
    });
  }

  onPlaybackEvent(
    listener: (event: AnimationPlaybackEvent) => void,
  ): () => void {
    return this.addPlaybackListener(listener);
  }

  static connectWebSocket(
    url: string,
    options: ConnectOptions = {},
  ): Promise<IppClient> {
    validateOptions(options);
    return IppClient.connectTransport(webSocketTransport(url), options);
  }

  static connectTransport(
    transport: MessageTransport,
    options: ConnectOptions = {},
  ): Promise<IppClient> {
    return IppHostClient.connectTransport(transport, options).then(
      async (host) => {
        try {
          const world = await host.createWorld({ temporary: true });
          host.ownWorldConnection();
          return world;
        } catch (error) {
          await host.close().catch(() => {});
          throw error;
        }
      },
    );
  }

  static connectWorker(
    workerUrl: string | URL,
    wasmUrl: string | URL,
    options: WorkerConnectOptions = {},
  ): Promise<IppClient> {
    validateOptions(options);
    return IppClient.connectTransport(
      workerTransport(workerUrl, wasmUrl, options),
      options,
    );
  }

  /** Construct the World client after Host attachment, without another bootstrap. */
  static attachTransport(
    transport: MessageTransport,
    session: bigint,
    world: WorldDescriptor,
    options: ConnectOptions,
    host: IppHostClient,
  ): IppClient {
    const client = new IppClient(transport, options);
    client.hostConnection = host;
    return client.initializeAttached(session, world);
  }

  protected override bootstrap(): Uint8Array<ArrayBuffer> {
    return bootstrap();
  }

  protected override acceptBootstrap(bytes: Uint8Array): bigint {
    return acceptBootstrap(bytes);
  }

  protected override encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
    return encodeRequest(request);
  }

  protected override decodeResponse(
    bytes: Uint8Array,
    session: bigint,
  ): Response {
    return decodeResponse(bytes, session);
  }
}

function writeControllerId(w: Writer, id: bigint): void {
  if (id === 0n) fail("zero animation controller");
  w.u64(id);
}

function writePlaybackControl(
  w: Writer,
  control: AnimationPlaybackControl,
): void {
  exactFields(
    control,
    control.action === "seek"
      ? ["action", "time"]
      : control.action === "playAtSpeed"
        ? ["action", "speed"]
        : ["action"],
  );
  const action = {
    play: WIRE.PLAYBACK_CONTROL_PLAY,
    pause: WIRE.PLAYBACK_CONTROL_PAUSE,
    stop: WIRE.PLAYBACK_CONTROL_STOP,
    seek: WIRE.PLAYBACK_CONTROL_SEEK,
    restart: WIRE.PLAYBACK_CONTROL_RESTART,
    playAtSpeed: WIRE.PLAYBACK_CONTROL_PLAY_AT_SPEED,
  }[control.action];
  if (action === undefined) fail("playback control");
  w.u32(action);
  w.f64(control.action === "seek" ? control.time : 0);
  w.f32(control.action === "playAtSpeed" ? control.speed : 0);
}

function writeControllerDescription(
  w: Writer,
  description: AnimationControllerDescription,
): void {
  exactFields(description, ["drivers", "speed", "looping"]);
  w.f32(description.speed ?? 1);
  w.boolean(description.looping ?? false);
  w.u32(description.drivers.length);
  for (const driver of description.drivers) {
    exactFields(driver, [
      "source",
      "variant",
      "track",
      "target",
      "property",
      "weight",
      "additive",
      "referenceTime",
      "repeat",
    ]);
    w.string(driver.source);
    w.u32(driver.variant ?? 0);
    w.u32(driver.track);
    w.u64(driver.target);
    const property = driver.property;
    const joints = property.joints;
    exactFields(
      property,
      property.name !== undefined
        ? ["component", "name"]
        : joints === undefined
          ? ["component", "offsets"]
          : ["joints"],
    );
    w.u32(
      property.name !== undefined
        ? WIRE.ANIMATION_TARGET_DYNAMIC
        : joints === undefined
          ? WIRE.ANIMATION_TARGET_PROPERTY
          : WIRE.ANIMATION_TARGET_JOINTS,
    );
    w.u16(joints === undefined ? property.component! : 0);
    const indices = joints ?? property.offsets ?? [];
    if (joints !== undefined && !CAPABILITIES.skeletalAnimation)
      fail("unsupported joint target");
    w.count(indices.length, 4096);
    for (const index of indices) w.u32(index);
    w.string(property.name ?? "");
    w.f32(driver.weight ?? 1);
    w.boolean(driver.additive ?? false);
    w.f32(driver.referenceTime ?? 0);
    w.boolean(driver.repeat ?? false);
  }
}

function writeControllerTransition(
  w: Writer,
  transition: AnimationControllerTransition,
): void {
  exactFields(transition, ["description", "duration", "easing", "startTime"]);
  writeControllerDescription(w, transition.description);
  w.f64(transition.duration);
  const easing = {
    linear: WIRE.ANIMATION_TRANSITION_LINEAR,
    smoothstep: WIRE.ANIMATION_TRANSITION_SMOOTHSTEP,
  }[transition.easing ?? "linear"];
  if (easing === undefined) fail("animation transition easing");
  w.u32(easing);
  const startTime = transition.startTime ?? { policy: "restart" as const };
  exactFields(
    startTime,
    startTime.policy === "seek" ? ["policy", "time"] : ["policy"],
  );
  const policy = {
    restart: WIRE.ANIMATION_TRANSITION_RESTART,
    preserve: WIRE.ANIMATION_TRANSITION_PRESERVE,
    matchPhase: WIRE.ANIMATION_TRANSITION_MATCH_PHASE,
    seek: WIRE.ANIMATION_TRANSITION_SEEK,
  }[startTime.policy];
  if (policy === undefined) fail("animation transition start time");
  w.u32(policy);
  w.f64(startTime.policy === "seek" ? startTime.time : 0);
}

function readControllerDescription(r: Reader): AnimationControllerDescription {
  const speed = r.f32();
  const looping = r.boolean();
  const count = r.u32();
  const drivers: AnimationDriverDescription[] = [];
  for (let i = 0; i < count; i++) {
    const source = r.string();
    const variant = r.u32();
    const track = r.u32();
    const target = r.u64();
    const kind = r.u32();
    const component = r.u16();
    const count = r.count(4096);
    const indices: number[] = [];
    for (let j = 0; j < count; j++) indices.push(r.u32());
    let property: AnimationDriverTarget;
    const name = r.string();
    if (kind !== WIRE.ANIMATION_TARGET_DYNAMIC && name !== "")
      fail("unexpected dynamic property name");
    if (kind === WIRE.ANIMATION_TARGET_DYNAMIC && indices.length === 0)
      property = { component, name };
    else if (kind === WIRE.ANIMATION_TARGET_PROPERTY)
      property = { component, offsets: indices };
    else if (
      kind === WIRE.ANIMATION_TARGET_JOINTS &&
      component === 0 &&
      CAPABILITIES.skeletalAnimation
    )
      property = { joints: indices };
    else return fail("animation target kind");
    const weight = r.f32();
    const additive = r.boolean();
    const referenceTime = r.f32();
    const repeat = r.boolean();
    if (
      !source ||
      target === 0n ||
      weight < 0 ||
      weight > 1 ||
      referenceTime < 0
    )
      fail("invalid animation driver");
    drivers.push({
      source,
      variant,
      track,
      target,
      property,
      weight,
      additive,
      referenceTime,
      repeat,
    });
  }
  return { speed, looping, drivers };
}

function readControllerTransitionState(r: Reader): {
  transition?: AnimationControllerTransitionState;
} {
  const option = r.u8();
  if (option === WIRE.OPTION_NONE) return {};
  if (option !== WIRE.OPTION_SOME) fail("animation transition option");
  const duration = r.f64();
  const elapsed = r.f64();
  const easing = (
    {
      [WIRE.ANIMATION_TRANSITION_LINEAR]: "linear",
      [WIRE.ANIMATION_TRANSITION_SMOOTHSTEP]: "smoothstep",
    } as const
  )[r.u32()];
  const pending = r.boolean();
  if (!easing || duration < 0 || elapsed < 0 || elapsed > duration)
    fail("invalid animation transition state");
  return { transition: { duration, elapsed, easing, pending } };
}

function readControllerState(r: Reader): AnimationControllerState {
  const id = r.u64();
  const state = (
    {
      [WIRE.PLAYBACK_STATE_STOPPED]: "stopped",
      [WIRE.PLAYBACK_STATE_PLAYING]: "playing",
      [WIRE.PLAYBACK_STATE_PAUSED]: "paused",
      [WIRE.PLAYBACK_STATE_COMPLETED]: "completed",
    } as const
  )[r.u32()];
  const time = r.f64();
  if (id === 0n || !state || time < 0) fail("invalid controller state");
  return { id, state, time };
}

const GeneratedHostClientBase = WorldPersistenceHostClient;

/** Connect to a Host, then discover, create, load or attach a World explicitly. */
export class IppHostClient extends GeneratedHostClientBase<IppClient> {
  protected hostTag(name: string): number {
    return WIRE[name as keyof typeof WIRE] ?? fail("Host contract tag");
  }
  protected hostMagic(response: boolean): Uint8Array<ArrayBuffer> {
    const hex =
      WIRE_CONVENTIONS[response ? "host-response-magic" : "host-request-magic"];
    return new Uint8Array(
      hex.match(/../g)!.map((byte) => Number.parseInt(byte, 16)),
    );
  }
  readonly schemaHash = SCHEMA_HASH;
  readonly capabilities = CAPABILITIES;

  static connectWebSocket(
    url: string,
    options: ConnectOptions = {},
  ): Promise<IppHostClient> {
    validateOptions(options);
    return IppHostClient.connectTransport(webSocketTransport(url), options);
  }

  static connectTransport(
    transport: MessageTransport,
    options: ConnectOptions = {},
  ): Promise<IppHostClient> {
    return new IppHostClient(transport, options).initialize();
  }

  static connectWorker(
    workerUrl: string | URL,
    wasmUrl: string | URL,
    options: WorkerConnectOptions = {},
  ): Promise<IppHostClient> {
    validateOptions(options);
    return IppHostClient.connectTransport(
      workerTransport(workerUrl, wasmUrl, options),
      options,
    );
  }

  protected override bootstrap(): Uint8Array<ArrayBuffer> {
    return bootstrap();
  }

  protected override acceptBootstrap(bytes: Uint8Array): bigint {
    return acceptBootstrap(bytes);
  }

  protected override createWorldClient(
    transport: MessageTransport,
    session: bigint,
    world: WorldDescriptor,
  ): IppClient {
    return IppClient.attachTransport(
      transport,
      session,
      world,
      this.options,
      this,
    );
  }
}
