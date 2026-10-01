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
import type { WorldDescriptor, WorldManifest } from "./host-client.js";
import {
  readWorldReference,
  writeWorldReference,
  readOutputReference,
  writeOutputReference,
} from "./references.js";
import { WorldPersistenceHostClient } from "./world-persistence-client.js";
export type {
  WorldLoadOptions,
  WorldTransferOptions,
  WorldGraphDescriptor,
  WorldGraphLoadResult,
} from "./world-persistence-client.js";
export { WorldGraphLoadError } from "./world-persistence-client.js";
export { HostContractMismatchError } from "./host-contract.js";
export { BatchIdentities } from "./command-pages.js";
export {
  HostPresentation,
  PresentationError,
  CaptureTransferError,
} from "./host-presentation.js";
export type {
  RootBinding,
  PresentationSurface,
  PresentationView,
  PresentedFrame,
  PresentedCapture,
  PresentationViewport,
  PresentationFrameOptions,
  PresentationFailure,
} from "./host-presentation.js";
export type {
  WorldDescriptor,
  WorldManifest,
  WorldSelector,
  WorldCreateOptions,
  WorldCapacityHints,
  WorldCapacityHintsPatch,
} from "./host-client.js";
import { WorldSelectionRequiredError } from "./host-client.js";
export { WorldSelectionRequiredError };
import {
  ClientBase,
  validateOptions,
  type ConnectOptions,
  type WorkerConnectOptions,
  type WorldConnectOptions,
  type WorkerWorldConnectOptions,
} from "./client.js";
import { webSocketTransport, type MessageTransport } from "./transport.js";
import { workerTransport } from "./worker.js";
import type {
  ClientAssetSource,
  BatchOperationEffect,
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
  LifecycleWatchRecord,
  LifecycleFieldValue,
  LifecycleTarget,
  LifecycleTargetLifetime,
  LifecycleBaseline,
  EntityRef,
  WorldReference,
  OutputReference,
  ViewQueryTarget,
  ViewDescriptor,
  ViewViewport,
  EntityPlacement,
  FieldWrite,
  FieldValue,
  EntityMetadata,
  Command,
  Request,
  BatchOutcome,
  ComponentSnapshot,
  EntitySnapshot,
  EntityTreeNode,
  AssetResourceSnapshot,
  RenderDiagnostic,
  Response,
  ResponseBody,
  ComponentDescriptor,
  ComponentFieldValue,
  RowAssetValue,
  RowPropertyValue,
  RowsLayoutDescriptor,
  RowsInput,
  RowsTable,
} from "./types.js";
export type * from "./types.js";
export type {
  Client,
  ClientClosure,
  ConnectOptions,
  WorkerConnectOptions,
  WorldConnectOptions,
  WorkerWorldConnectOptions,
  ResourceUrlMapping,
  LogLevel,
} from "./client.js";
export { RequestNotSentError, RequestRejectedError } from "./client.js";

const ROW_PROPERTY_KINDS = [
  "f32",
  "i32",
  "u32",
  "bool",
  "vec2",
  "vec3",
  "vec4",
] as const;
function rowsLayout(
  component: ComponentDescriptor,
  field: string,
): RowsLayoutDescriptor {
  return (
    component.fields[field]?.rows ?? fail(`${field} is not a schema rows field`)
  );
}
/**
 * Field offset of one schema row property: `regionBase + slot * count + index`.
 * Slots are never reused; writes to dead or unallocated slots fail in the World.
 */
export function rowFieldOffset(
  component: ComponentDescriptor,
  field: string,
  slot: number,
  property: string,
): number {
  const layout = rowsLayout(component, field);
  const count = layout.properties.length;
  const index = layout.properties.findIndex(
    (candidate) => candidate.name === property,
  );
  if (index < 0) fail(`unknown row property ${field}.${property}`);
  if (
    !Number.isSafeInteger(slot) ||
    slot < 0 ||
    slot >= Math.floor(ROW_REGION_SPAN / count)
  )
    fail("row slot out of range");
  return layout.regionBase + slot * count + index;
}
/** Check a row text value against its property's UTF-8 byte bound. */
function rowText(
  field: string,
  property: RowsLayoutDescriptor["properties"][number],
  value: unknown,
): string {
  if (typeof value !== "string")
    fail(`row property ${field}.${property.name} requires text`);
  const bound = property.maxBytes ?? fail("text row property has no bound");
  if (new TextEncoder().encode(value).length > bound)
    fail(`row property ${field}.${property.name} exceeds ${bound} bytes`);
  const writer = new Writer(ROWS_VALUE_BYTES);
  writer.string(value, bound);
  return value;
}
function rowPropertyValue(
  field: string,
  property: RowsLayoutDescriptor["properties"][number],
  value: unknown,
): FieldValue {
  if (property.kind === "text")
    return { kind: "string", value: rowText(field, property, value) };
  if (
    property.kind !== "asset" &&
    !ROW_PROPERTY_KINDS.some((kind) => kind === property.kind)
  )
    fail("unsupported row property kind");
  if (
    ["vec2", "vec3", "vec4"].includes(property.kind)
      ? !Array.isArray(value)
      : Array.isArray(value)
  )
    fail(`row property ${field}.${property.name} has the wrong shape`);
  if (Array.isArray(value))
    for (const lane of value)
      if (typeof lane !== "number" || !Number.isFinite(Math.fround(lane)))
        fail("row vector requires finite numeric lanes");
  const dynamic = {
    kind: property.kind,
    value,
  } as import("./dynamic-properties.js").DynamicValue;
  return {
    kind: "dynamic",
    value: decodeDynamicValue(encodeDynamicValue(dynamic)),
  };
}
function rowObject(
  value: object,
  layout: RowsLayoutDescriptor,
): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    fail("row requires an object");
  for (const name of Object.keys(value))
    if (!layout.properties.some((property) => property.name === name))
      fail(`unknown row property ${name}`);
  return value as Record<string, unknown>;
}
/**
 * Encode a complete table without allocating or reusing slot identities.
 * Omitted optional properties remain absent; required properties must be supplied.
 * Replacing a table is a whole-value write, not a sparse patch or a lifetime merge.
 */
export function encodeRowsTable<Row extends object>(
  layout: RowsLayoutDescriptor,
  table: RowsInput<Row>,
): Uint8Array<ArrayBuffer> {
  const count = layout.properties.length;
  if (
    count === 0 ||
    count > ROW_PROPERTIES ||
    new Set(layout.properties.map((property) => property.name)).size !== count
  )
    fail("invalid rows layout");
  const nextSlot = uint(table.nextSlot, Math.floor(ROW_REGION_SPAN / count));
  const writer = new Writer(ROWS_VALUE_BYTES);
  writer.u32(nextSlot);
  writer.count(
    table.rows.size,
    Math.floor((ROWS_VALUE_BYTES - 8) / (4 + Math.ceil(count / 8))),
  );
  const entries = [...table.rows].sort(([left], [right]) => left - right);
  let previous = -1;
  for (const [slot, row] of entries) {
    uint(slot, nextSlot - 1);
    if (slot <= previous) fail("rows table slot order");
    previous = slot;
    const values = rowObject(row, layout);
    const mask = new Uint8Array(Math.ceil(count / 8));
    layout.properties.forEach((property, index) => {
      if (
        Object.hasOwn(values, property.name) &&
        values[property.name] !== undefined
      )
        mask[index >> 3]! |= 1 << (index & 7);
      else if (!property.optional) fail("rows table omits a required property");
    });
    writer.u32(slot);
    writer.raw(mask);
    for (const property of layout.properties) {
      if (
        !Object.hasOwn(values, property.name) ||
        values[property.name] === undefined
      )
        continue;
      const value = rowPropertyValue("table", property, values[property.name]);
      if (value.kind === "string")
        writer.string(value.value, property.maxBytes);
      else if (value.kind === "dynamic") {
        const bytes = encodeDynamicValue(value.value);
        if (value.value.kind === "asset") {
          writer.raw(bytes.subarray(1, 7));
          writer.count(bytes.length - 7, ROWS_VALUE_BYTES);
          writer.raw(bytes.subarray(7));
        } else writer.raw(bytes.subarray(1));
      } else fail("unsupported row property");
    }
  }
  return writer.finish();
}
/**
 * One `setField` per listed property in layout order: values replace a property
 * and `null` clears an optional one. Text is sent as a string value after its
 * UTF-8 byte length is checked against the property's bound.
 */
export function rowPatchCommands(
  entity: EntityRef,
  component: ComponentDescriptor,
  field: string,
  slot: number,
  patch: object,
): Command[] {
  return rowPatchFields(component, field, slot, patch).map((write) => ({
    kind: "setField",
    entity,
    component: component.id,
    field: write,
  }));
}
/** Sparse, detached field writes for component commands. */
export function rowPatchFields(
  component: ComponentDescriptor,
  field: string,
  slot: number,
  patch: object,
): FieldWrite[] {
  const layout = rowsLayout(component, field);
  const values = rowObject(patch, layout);
  rowFieldOffset(component, field, slot, layout.properties[0]?.name ?? "");
  const fields: FieldWrite[] = [];
  for (const property of layout.properties) {
    if (!Object.hasOwn(values, property.name)) continue;
    const value = values[property.name];
    if (value === undefined) continue;
    if (value === null && !property.optional)
      fail(`required row property ${field}.${property.name} cannot be cleared`);
    fields.push({
      offset: rowFieldOffset(component, field, slot, property.name),
      value:
        value === null
          ? { kind: "unset" }
          : rowPropertyValue(field, property, value),
    });
  }
  return fields;
}
/** Decode a schema rows table in its exported row layout. */
export function decodeRowsTable<Row = Record<string, RowPropertyValue>>(
  layout: RowsLayoutDescriptor,
  bytes: Uint8Array,
): RowsTable<Row> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  const take = (length: number): number => {
    if (at + length > bytes.length) fail("truncated rows table");
    const start = at;
    at += length;
    return start;
  };
  const u32 = (): number => view.getUint32(take(4), true);
  const f32 = (): number => {
    const value = view.getFloat32(take(4), true);
    if (!Number.isFinite(value)) fail("nonfinite row property");
    return value;
  };
  const count = layout.properties.length;
  const maskBytes = Math.ceil(count / 8);
  const nextSlot = u32();
  const live = u32();
  if (
    nextSlot > Math.floor(ROW_REGION_SPAN / count) ||
    live > (bytes.length - at) / (4 + maskBytes)
  )
    fail("rows table size");
  const rows = new Map<number, Row>();
  let previous = -1;
  for (let row = 0; row < live; row++) {
    const slot = u32();
    if (slot >= nextSlot || slot <= previous) fail("rows table slot order");
    previous = slot;
    const mask = take(maskBytes);
    if (count % 8 !== 0 && bytes[mask + maskBytes - 1]! >> (count % 8) !== 0)
      fail("rows table presence mask");
    const values: Record<string, RowPropertyValue> = {};
    layout.properties.forEach((property, index) => {
      if ((bytes[mask + (index >> 3)]! & (1 << (index & 7))) === 0) {
        if (!property.optional) fail("rows table omits a required property");
        return;
      }
      const kind = ROW_PROPERTY_KINDS.indexOf(
        property.kind as (typeof ROW_PROPERTY_KINDS)[number],
      );
      if (property.kind === "asset") {
        const type = view.getUint16(take(2), true);
        const variant = u32();
        const length = u32();
        const start = take(length);
        const asset: RowAssetValue = {
          kind: type,
          source: new TextDecoder("utf-8", { fatal: true }).decode(
            bytes.subarray(start, start + length),
          ),
          variant,
        };
        values[property.name] = asset;
      } else if (property.kind === "text") {
        const length = u32();
        if (length > (property.maxBytes ?? 0)) fail("row text bound");
        const start = take(length);
        values[property.name] = new TextDecoder("utf-8", {
          fatal: true,
        }).decode(bytes.subarray(start, start + length));
      } else if (property.kind === "i32")
        values[property.name] = view.getInt32(take(4), true);
      else if (property.kind === "u32") values[property.name] = u32();
      else if (property.kind === "bool") {
        const flag = u32();
        if (flag > 1) fail("invalid row boolean");
        values[property.name] = flag === 1;
      } else if (property.kind === "f32") values[property.name] = f32();
      else if (kind >= 4)
        values[property.name] = Array.from({ length: kind - 2 }, f32);
      else fail("unsupported row property kind");
    });
    rows.set(slot, values as Row);
  }
  if (at !== bytes.length) fail("trailing rows table bytes");
  return { nextSlot, rows };
}
/** Entity references and command builders; submit commands through client.batch(). */
export const Entity = {
  handle(id: bigint): EntityRef {
    return { kind: "handle", id };
  },
  alias(alias: number): EntityRef {
    return { kind: "alias", alias };
  },
  /** The live entity carrying this symbolic id when the command applies. */
  symbol(symbol: string): EntityRef {
    return { kind: "symbol", symbol };
  },
  /**
   * Create an entity under a batch alias. With `adopt`, a live entity that
   * already carries `metadata.symbolicId` is bound to the alias and receives
   * the metadata instead of failing; the outcome reports an `adopted` effect.
   */
  create(
    alias: number,
    metadata: Partial<EntityMetadata> = {},
    options: { adopt?: boolean } = {},
  ): Command {
    return {
      kind: "create",
      alias,
      metadata: {
        symbolicId: metadata.symbolicId ?? null,
        classes: metadata.classes ?? [],
      },
      adopt: options.adopt ?? false,
    };
  },
  /**
   * Compare-and-set: write `field` only while the stored field equals
   * `expected`, a value of the field's own type; otherwise the batch stops with
   * `ValueMismatch` and the write has no effect.
   */
  setFieldIf(
    entity: EntityRef,
    component: number,
    field: FieldWrite,
    expected: FieldValue,
  ): Command {
    return { kind: "setFieldIf", entity, component, field, expected };
  },
  /**
   * Apply a semantic action to the control component at `incarnation`. The
   * batch stops without effect with `StaleTarget`, `Unavailable`,
   * `UnsupportedAction` or `InvalidValue`.
   */
  guiAction(
    target: {
      entity: bigint | EntityRef;
      component: number;
      incarnation: bigint;
    },
    action: GuiAction,
  ): Command {
    const entity =
      typeof target.entity === "bigint"
        ? Entity.handle(target.entity)
        : target.entity;
    return {
      kind: "guiAction",
      entity,
      component: target.component,
      incarnation: target.incarnation,
      action,
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
/** Refuse a convenience connection before opening a transport it cannot use. */
function validateWorldOptions(options: WorldConnectOptions): void {
  if (options?.selectedSystems === undefined)
    throw new WorldSelectionRequiredError();
  validateOptions(options);
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
    this.reserve(1).setUint8(0, uint(v, 0xff));
    this.at++;
  }
  boolean(v: boolean): void {
    if (typeof v !== "boolean") fail("boolean value required");
    this.u8(v ? 1 : 0);
  }
  u16(v: number): void {
    this.reserve(2).setUint16(0, uint(v, 0xffff), true);
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
  string(s: string, max = FIELD_BYTES): void {
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
  string(max = FIELD_BYTES): string {
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
    case "symbol":
      w.u8(WIRE.REF_SYMBOL);
      w.string(entity.symbol);
      break;
    default:
      fail("unsupported reference");
  }
}
function writePlacement(w: Writer, placement: EntityPlacement): void {
  exactFields(placement, ["parent", "before"]);
  for (const reference of [placement.parent, placement.before]) {
    w.u8(reference === null ? WIRE.OPTION_NONE : WIRE.OPTION_SOME);
    if (reference !== null) writeRef(w, reference);
  }
}
function writeMetadata(w: Writer, m: EntityMetadata): void {
  if (m.symbolicId === null) w.u8(WIRE.OPTION_NONE);
  else {
    w.u8(WIRE.OPTION_SOME);
    w.string(m.symbolicId);
  }
  w.count(m.classes.length, METADATA_CLASSES);
  for (const c of m.classes) w.string(c);
}
function writeField(w: Writer, f: FieldWrite): void {
  w.u32(f.offset);
  writeFieldValue(w, f.value);
}
function writeFieldValue(w: Writer, value: FieldValue): void {
  switch (value.kind) {
    case "world":
      w.u8(WIRE.VALUE_WORLD);
      w.boolean(value.value !== null);
      if (value.value !== null) writeWorldReference(w, value.value);
      break;
    case "output":
      w.u8(WIRE.VALUE_OUTPUT);
      w.boolean(value.value !== null);
      if (value.value !== null) writeOutputReference(w, value.value);
      break;
    case "dynamic": {
      const bytes = encodeDynamicValue(value.value);
      w.u8(WIRE.VALUE_DYNAMIC);
      w.count(bytes.length, FIELD_BYTES);
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
      w.count(value.value.byteLength, FIELD_BYTES);
      w.raw(value.value);
      break;
    case "entity":
      w.u8(WIRE.VALUE_ENTITY);
      writeRef(w, value.value);
      break;
    case "rows":
      w.u8(WIRE.VALUE_ROWS);
      w.count(value.value.byteLength, ROWS_VALUE_BYTES);
      w.raw(value.value);
      break;
    case "unset":
      w.u8(WIRE.VALUE_UNSET);
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
      w.count(bytes.length, FIELD_BYTES);
      w.raw(bytes);
      break;
    }
    case "removeDynamicProperty":
      w.u8(WIRE.COMMAND_REMOVE_DYNAMIC_PROPERTY);
      writeRef(w, c.entity);
      w.u16(c.component);
      w.string(c.name);
      break;
    case "create":
      w.u8(WIRE.COMMAND_CREATE);
      w.u32(c.alias);
      writeMetadata(w, c.metadata);
      w.boolean(c.adopt ?? false);
      break;
    case "delete":
      w.u8(WIRE.COMMAND_DELETE);
      writeRef(w, c.entity);
      break;
    case "placeEntity":
      w.u8(WIRE.COMMAND_PLACE_ENTITY);
      writeRef(w, c.entity);
      writePlacement(w, c.placement);
      break;
    case "detachWorldAttachment":
      w.u8(WIRE.COMMAND_DETACH_ATTACHMENT_RECEIPT);
      w.u64(c.receipt);
      break;
    case "deleteSubtree":
      w.u8(WIRE.COMMAND_DELETE_SUBTREE);
      writeRef(w, c.root);
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
      w.count(c.fields.length, INSERT_FIELDS);
      for (const f of c.fields) writeField(w, f);
      w.boolean(c.adopt ?? false);
      break;
    case "setField":
      w.u8(WIRE.COMMAND_SET);
      writeRef(w, c.entity);
      w.u16(c.component);
      writeField(w, c.field);
      break;
    case "setFieldIf":
      w.u8(WIRE.COMMAND_SET_FIELD_IF);
      writeRef(w, c.entity);
      w.u16(c.component);
      writeField(w, c.field);
      writeFieldValue(w, c.expected);
      break;
    case "removeComponent":
      w.u8(WIRE.COMMAND_REMOVE);
      writeRef(w, c.entity);
      w.u16(c.component);
      break;
    case "guiAction":
      w.u8(WIRE.COMMAND_GUI_ACTION);
      writeRef(w, c.entity);
      w.u16(c.component);
      w.u64(c.incarnation);
      writeGuiAction(w, c.action);
      break;
    default:
      fail("unsupported command");
  }
}
export function encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
  const w = new Writer(
    request.body.kind === "submitBatch"
      ? COMMAND_PAGE_LIMITS.bytes
      : MAX_MESSAGE_BYTES,
  );
  if (request.session === 0n) fail("invalid session");
  // A batch is answered once, on its final page; earlier pages are uncorrelated.
  const uncorrelated =
    request.body.kind === "command" ||
    (request.body.kind === "submitBatch" && !request.body.last);
  if (uncorrelated !== (request.requestId === 0n))
    fail("reserved request identity");
  w.u64(request.session);
  w.u64(request.requestId);
  const body = request.body;
  switch (body.kind) {
    case "guiObservation": {
      w.u8(WIRE.REQUEST_GUI_OBSERVATION);
      const inner = new Writer();
      writeGuiObservation(inner, body.control);
      const bytes = inner.finish();
      w.count(bytes.length, GUI_OBSERVATION_CONTROL_BYTES);
      w.raw(bytes);
      break;
    }
    case "submitBatch":
      if (!Number.isInteger(body.batchId) || body.batchId < 0)
        fail("invalid batch identity");
      w.u8(WIRE.REQUEST_SUBMIT_BATCH);
      w.u32(body.batchId);
      w.u8(body.last ? 1 : 0);
      w.count(body.operations.length, COMMAND_PAGE_LIMITS.commands);
      for (const c of body.operations) writeCommand(w, c);
      break;
    case "lifecycleWatch": {
      const control = body.control;
      w.u8(WIRE.REQUEST_LIFECYCLE_WATCH);
      writeWorldReference(w, control.world);
      if (control.kind === "add") {
        w.u8(WIRE.LIFECYCLE_WATCH_ADD);
        if (control.targets.length === 0) fail("empty membership page");
        w.count(control.targets.length, LIFECYCLE_MEMBERS);
        for (const { target, kinds } of control.targets) {
          if (
            target.entity <= 0n ||
            !Number.isInteger(kinds) ||
            kinds <= 0 ||
            kinds > 255
          )
            fail("lifecycle target selection");
          const mask =
            target.kind === "entity"
              ? 7
              : target.kind === "component"
                ? 120
                : 128;
          if (kinds & ~mask) fail("lifecycle target kinds");
          w.u8(
            target.kind === "entity"
              ? WIRE.LIFECYCLE_WATCH_ENTITY
              : target.kind === "component"
                ? WIRE.LIFECYCLE_WATCH_COMPONENT
                : WIRE.LIFECYCLE_WATCH_VALUE,
          );
          w.u64(target.entity);
          if (target.kind !== "entity") {
            if (target.component <= 0) fail("lifecycle component");
            w.u16(target.component);
          }
          if (target.kind === "value") {
            if (target.fields.length === 0)
              fail("empty lifecycle value fields");
            w.count(target.fields.length, LIFECYCLE_VALUE_FIELDS);
            let previous = -1;
            for (const offset of target.fields) {
              if (offset <= previous) fail("unsorted lifecycle value fields");
              w.u32(offset);
              previous = offset;
            }
          }
          w.u8(kinds);
        }
      } else {
        w.u8(WIRE.LIFECYCLE_WATCH_REMOVE);
        if (control.output <= 0n || control.generations.length === 0)
          fail("lifecycle remove page");
        w.u64(control.output);
        w.count(control.generations.length, LIFECYCLE_MEMBERS);
        let previous = 0n;
        for (const generation of control.generations) {
          if (generation <= previous) fail("unsorted lifecycle members");
          w.u64(generation);
          previous = generation;
        }
      }
      break;
    }
    case "lifecycleDiagnostics": {
      w.u8(WIRE.REQUEST_LIFECYCLE_DIAGNOSTICS);
      writeWorldReference(w, body.query.world);
      if (
        body.query.output <= 0n ||
        body.query.world.id <= 0n ||
        body.query.world.incarnation <= 0n
      )
        fail("lifecycle diagnostic endpoint");
      w.u64(body.query.output);
      break;
    }
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
    case "attachmentReceipt":
      w.u8(WIRE.REQUEST_ATTACHMENT_RECEIPT);
      w.u64(body.receipt);
      w.u8(body.release ? 1 : 0);
      break;
    case "inspect":
      w.u8(WIRE.REQUEST_INSPECT);
      w.u8(
        // GUI and Canvas System queries are absent from builds without GUI or
        // Surfaces and fail here.
        (
          {
            summary: WIRE.INSPECT_SUMMARY,
            entities: WIRE.INSPECT_ENTITIES,
            resources: WIRE.INSPECT_RESOURCES,
            controllers: WIRE.INSPECT_CONTROLLERS,
            renderDiagnostics: WIRE.INSPECT_RENDER_DIAGNOSTICS,
            guiFocus: WIRE.INSPECT_GUI_FOCUS,
            guiPointers: WIRE.INSPECT_GUI_POINTERS,
            canvas: WIRE.INSPECT_CANVAS,
          } as Partial<Record<typeof body.collection, number>>
        )[body.collection] ?? fail("inspection collection"),
      );
      w.u64(body.after ?? 0n);
      w.u64(body.target ?? 0n);
      if (
        (body.limit ?? INSPECTION_PAGE) < 1 ||
        (body.limit ?? INSPECTION_PAGE) > INSPECTION_PAGE ||
        (body.target && body.after)
      )
        fail("inspection query");
      w.u16(body.limit ?? INSPECTION_PAGE);
      w.u16(0);
      break;
    case "inspectTree":
      if (
        (body.limit ?? ENTITY_TREE_PAGE) < 1 ||
        (body.limit ?? ENTITY_TREE_PAGE) > ENTITY_TREE_PAGE ||
        !Number.isInteger(body.limit ?? ENTITY_TREE_PAGE) ||
        (body.maxDepth ?? 0) < 0 ||
        (body.maxDepth ?? 0) > ENTITY_TREE_DEPTH ||
        !Number.isInteger(body.maxDepth ?? 0)
      )
        fail("entity tree query");
      w.u8(WIRE.REQUEST_INSPECT);
      w.u8(WIRE.INSPECT_ENTITY_TREE);
      w.u64(body.after ?? 0n);
      w.u64(body.root ?? 0n);
      w.u16(body.limit ?? ENTITY_TREE_PAGE);
      w.u16(body.maxDepth ?? 0);
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
    case "cameraNavigate": {
      const request = body.request;
      exactFields(request, ["binding", "publication", "motion"]);
      w.u8(WIRE.REQUEST_CAMERA_NAVIGATE);
      writeCameraBinding(w, request.binding);
      writeViewSource(w, request.publication);
      const motion = request.motion;
      if (motion.kind === "rotate") {
        exactFields(motion, ["kind", "yaw", "pitch"]);
        w.u32(0);
        w.f32(motion.yaw);
        w.f32(motion.pitch);
      } else if (motion.kind === "pan") {
        exactFields(motion, ["kind", "x", "y"]);
        w.u32(1);
        w.f32(motion.x);
        w.f32(motion.y);
      } else if (motion.kind === "zoom") {
        exactFields(motion, ["kind", "amount"]);
        w.u32(2);
        w.f32(motion.amount);
        w.f32(0);
      } else fail("camera motion");
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
      if (command.type === "RenderStateUpdateCommand") {
        exactFields(command, ["type", "changes"]);
        w.u8(WIRE.REQUEST_RENDER_STATE_UPDATE);
        writeRenderStatePatch(w, command.changes);
        break;
      }
      if (command.type === "CanvasStateUpdateCommand") {
        exactFields(command, ["type", "extent", "unitsPerMetre"]);
        const extent = command.extent;
        const density = command.unitsPerMetre;
        w.u8(WIRE.REQUEST_CANVAS_STATE_UPDATE);
        w.u16((extent !== undefined ? 1 : 0) | (density !== undefined ? 2 : 0));
        if (extent !== undefined) {
          if (!Array.isArray(extent) || extent.length !== 2)
            fail("canvas extent requires width and height");
          w.f32(extent[0]);
          w.f32(extent[1]);
        }
        if (density !== undefined) w.f32(density);
        break;
      }
      fail("unsupported command");
    }
    case "query": {
      const query = body.query;
      if (query.type === "CameraProjectQuery") {
        exactFields(query, ["type", "view", "x", "y", "plane"]);
        exactFields(query.plane, ["point", "normal"]);
        w.u8(WIRE.REQUEST_CAMERA_PROJECT);
        writeViewTarget(w, query.view);
        w.f32(query.x);
        w.f32(query.y);
        for (const vector of [query.plane.point, query.plane.normal]) {
          if (!Array.isArray(vector) || vector.length !== 3)
            fail("plane requires three coordinates");
          for (const value of vector) w.f32(value);
        }
        break;
      }
      if (query.type !== "GeometryPickQuery") fail("unsupported query");
      exactFields(query, ["type", "view", "x", "y", "includeViewPlane"]);
      w.u8(WIRE.REQUEST_GEOMETRY_PICK);
      writeViewTarget(w, query.view);
      w.f32(query.x);
      w.f32(query.y);
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
  const n = r.count(METADATA_CLASSES);
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
    const n = r.count(BATCH_OUTCOME_ALIASES);
    const aliases: { alias: number; id: bigint }[] = [];
    for (let i = 0; i < n; i++) aliases.push({ alias: r.u32(), id: r.u64() });
    const symbolCount = r.count(BATCH_OUTCOME_ALIASES);
    const symbols: { symbol: string; id: bigint }[] = [];
    for (let i = 0; i < symbolCount; i++) {
      const symbol = r.string();
      const id = r.u64();
      if (id === 0n) fail("zero resolved symbol");
      symbols.push({ symbol, id });
    }
    const effects: BatchOperationEffect[] = [];
    const count = r.count(ATTACHMENT_EFFECTS);
    for (let index = 0; index < count; index++) {
      const operation = r.u32();
      const tag = r.u8();
      if (tag === WIRE.OPERATION_ADOPTED) {
        effects.push({ operation, kind: "adopted" });
        continue;
      }
      const kind =
        tag === WIRE.ATTACHMENT_WRITTEN
          ? "written"
          : tag === WIRE.ATTACHMENT_DETACHED
            ? "detached"
            : tag === WIRE.ATTACHMENT_SUPERSEDED
              ? "superseded"
              : fail("invalid attachment effect");
      const id = r.u64();
      const parent = readWorldReference(r);
      const anchor = r.u64();
      const incarnation = r.u64();
      const revision = r.u64();
      const option = r.u8();
      const child =
        option === WIRE.OPTION_NONE
          ? null
          : option === WIRE.OPTION_SOME
            ? readWorldReference(r)
            : fail("invalid child option");
      effects.push({
        operation,
        kind,
        receipt: { id, parent, anchor, incarnation, revision, child },
      });
    }
    return error
      ? { batchId, tick, ok: false, error, aliases, symbols, effects }
      : { batchId, tick, ok: true, aliases, symbols, effects };
  }
}
const descriptors: readonly ComponentDescriptor[] = Object.values(components);
function readComponent(r: Reader): ComponentSnapshot {
  const component = r.u16();
  const descriptor = descriptors.find((c) => c.id === component);
  if (!descriptor) return fail("unsupported component");
  const n = r.count(INSPECTED_FIELDS);
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
  const fields: Record<string, ComponentFieldValue> = Object.create(
    null,
  ) as Record<string, ComponentFieldValue>;
  for (let i = 0; i < n; i++) {
    const offset = r.u32();
    const kind = r.u8();
    if (offset >= 0x80000000 && descriptor.dynamicProperties) {
      if (
        offset === 0x80000000 &&
        !dynamicDescriptors &&
        kind === WIRE.SNAPSHOT_VALUE_BYTES
      ) {
        dynamicDescriptors = decodeDynamicDescriptors(
          r.raw(r.count(INSPECTED_BYTES_LIMIT)),
        );
        continue;
      }
      const property = dynamicDescriptors?.get(offset);
      if (
        !property ||
        kind !== WIRE.SNAPSHOT_VALUE_DYNAMIC ||
        Object.hasOwn(properties, property.name)
      )
        fail("invalid dynamic inspection");
      const value = decodeDynamicValue(r.raw(r.count(FIELD_BYTES)));
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
    else if (kind === WIRE.SNAPSHOT_VALUE_WORLD)
      fields[entry[0]] = r.boolean() ? readWorldReference(r) : null;
    else if (kind === WIRE.SNAPSHOT_VALUE_OUTPUT)
      fields[entry[0]] = r.boolean() ? readOutputReference(r) : null;
    else if (kind === WIRE.SNAPSHOT_VALUE_F32) fields[entry[0]] = r.f32();
    else if (kind === WIRE.SNAPSHOT_VALUE_U32) fields[entry[0]] = r.u32();
    else if (kind === WIRE.SNAPSHOT_VALUE_U64) fields[entry[0]] = r.u64();
    else if (kind === WIRE.SNAPSHOT_VALUE_STRING) fields[entry[0]] = r.string();
    else if (kind === WIRE.SNAPSHOT_VALUE_BYTES)
      fields[entry[0]] = r.raw(r.count(INSPECTED_BYTES_LIMIT));
    else if (kind === WIRE.SNAPSHOT_VALUE_ROWS)
      fields[entry[0]] = decodeRowsTable(
        entry[1].rows ?? fail("inspected rows field has no layout"),
        r.raw(r.count(INSPECTED_ROWS_LIMIT)),
      );
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
    if (status === "failed") resource.error = r.string(RESOURCE_ERROR_BYTES);
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
function writeViewViewport(writer: Writer, viewport: ViewViewport): void {
  exactFields(viewport, ["width", "height", "devicePixelRatio"]);
  writer.u32(viewport.width);
  writer.u32(viewport.height);
  writer.f64(viewport.devicePixelRatio);
}

function writeViewTarget(writer: Writer, view: ViewQueryTarget): void {
  if (typeof view !== "object" || view === null) fail("view target required");
  if (view.kind === "bound") {
    exactFields(view, ["kind", "binding", "publication"]);
    writer.u8(WIRE.VIEW_BOUND);
    writeCameraBinding(writer, view.binding);
    writeViewSource(writer, view.publication);
  } else if (view.kind === "root") {
    exactFields(view, ["kind", "output", "expectedViewport"]);
    writer.u8(WIRE.VIEW_ROOT);
    writeOutputReference(writer, view.output);
    writeViewViewport(writer, view.expectedViewport);
  } else if (view.kind === "publication") {
    exactFields(view, ["kind", "output", "publication", "viewport"]);
    exactFields(view.publication, ["host", "revision"]);
    writer.u8(WIRE.VIEW_PUBLICATION);
    writeOutputReference(writer, view.output);
    writer.u64(view.publication.host);
    writer.u64(view.publication.revision);
    writeViewViewport(writer, view.viewport);
  } else fail("view target required");
}

function writeViewSource(
  writer: Writer,
  source: { host: bigint; revision: bigint } | undefined,
): void {
  writer.u8(source === undefined ? WIRE.OPTION_NONE : WIRE.OPTION_SOME);
  if (source !== undefined) {
    exactFields(source, ["host", "revision"]);
    writer.u64(source.host);
    writer.u64(source.revision);
  }
}

function writeCameraBinding(
  writer: Writer,
  binding: import("./host-presentation.js").RootBinding,
): void {
  exactFields(binding, ["output", "viewport", "generation"]);
  exactFields(binding.generation, ["host", "serial"]);
  writeOutputReference(writer, binding.output);
  writeViewViewport(writer, binding.viewport);
  writer.u64(binding.generation.host);
  writer.u64(binding.generation.serial);
}

function readViewDescriptor(reader: Reader): ViewDescriptor {
  return {
    output: readOutputReference(reader),
    publication: { host: reader.u64(), revision: reader.u64() },
    viewport: {
      width: reader.u32(),
      height: reader.u32(),
      devicePixelRatio: reader.f64(),
    },
  };
}

function readCameraProjection(r: Reader): CameraProjectResultPayload {
  const type = "CameraProjectResultEvent";
  const viewTag = r.u8();
  const view =
    viewTag === WIRE.OPTION_NONE
      ? null
      : viewTag === WIRE.OPTION_SOME
        ? readViewDescriptor(r)
        : fail("projection view option");
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
    if (view === null || error !== null) fail("invalid successful projection");
    return { type, view, ok, position };
  }
  if (view !== null || position !== null || error === null)
    fail("invalid failed projection");
  return { type, ok, error };
}

function readGeometryResult(r: Reader): GeometryPickResultPayload {
  const type = "GeometryPickResultEvent";
  const tag = r.u8();
  if (tag === WIRE.PICK_OUTCOME_FAILURE)
    return { type, ok: false, error: r.string() };
  if (tag !== WIRE.PICK_OUTCOME_MISS && tag !== WIRE.PICK_OUTCOME_HIT)
    return fail("geometry result tag");
  const view = readViewDescriptor(r);
  if (tag === WIRE.PICK_OUTCOME_MISS)
    return { type, view, ok: true, hit: null };
  const world = readWorldReference(r);
  const publication = { host: r.u64(), revision: r.u64() };
  const entity = r.u64();
  const incarnation = r.u64();
  const position: [number, number, number] = [r.f32(), r.f32(), r.f32()];
  const distance = r.f64();
  const part = r.u32();
  const path: GeometryPickHit["path"] = [];
  const count = r.count(Math.floor(MAX_MESSAGE_BYTES / 24));
  for (let index = 0; index < count; index++)
    path.push({ world: readWorldReference(r), anchor: r.u64() });
  const hit: GeometryPickHit = {
    world,
    publication,
    entity,
    incarnation,
    position,
    distance,
    part,
    path,
  };
  const planeTag = r.u8();
  if (planeTag === WIRE.OPTION_SOME) {
    hit.viewPlane = {
      point: [r.f32(), r.f32(), r.f32()],
      normal: [r.f32(), r.f32(), r.f32()],
    };
  } else if (planeTag !== WIRE.OPTION_NONE) fail("geometry view plane option");
  return { type, view, ok: true, hit };
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
function readCanvasStateRecord(
  r: Reader,
): import("./types.js").CanvasStateRecord {
  const state = {
    extent: [r.f32(), r.f32()] as const,
    unitsPerMetre: r.f32(),
  };
  return {
    state,
    evaluated: r.boolean()
      ? { extent: [r.f32(), r.f32()] as const, tick: r.u64() }
      : null,
  };
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
  const count = r.count(LIFECYCLE_PUBLICATIONS);
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

function readLifecycleTarget(r: Reader): LifecycleTarget {
  const tag = r.u8();
  const entity = r.u64();
  if (entity === 0n) fail("zero lifecycle target");
  if (tag === WIRE.LIFECYCLE_WATCH_ENTITY) return { kind: "entity", entity };
  if (
    tag !== WIRE.LIFECYCLE_WATCH_COMPONENT &&
    tag !== WIRE.LIFECYCLE_WATCH_VALUE
  )
    fail("lifecycle target tag");
  const component = r.u16();
  if (component === 0) fail("zero lifecycle component");
  if (tag === WIRE.LIFECYCLE_WATCH_COMPONENT)
    return { kind: "component", entity, component };
  const count = r.count(LIFECYCLE_VALUE_FIELDS);
  if (count === 0) fail("empty lifecycle value fields");
  const fields: number[] = [];
  for (let index = 0; index < count; index++) {
    const offset = r.u32();
    if (index > 0 && offset <= fields[index - 1]!)
      fail("unsorted lifecycle value fields");
    fields.push(offset);
  }
  return { kind: "value", entity, component, fields };
}

/** Decode one snapshot-encoded value of a value record by its kind tag. */
function readLifecycleFieldValue(r: Reader, kind: number): ComponentFieldValue {
  if (kind === WIRE.SNAPSHOT_VALUE_BOOL) return r.boolean();
  if (kind === WIRE.SNAPSHOT_VALUE_WORLD)
    return r.boolean() ? readWorldReference(r) : null;
  if (kind === WIRE.SNAPSHOT_VALUE_OUTPUT)
    return r.boolean() ? readOutputReference(r) : null;
  if (kind === WIRE.SNAPSHOT_VALUE_F32) return r.f32();
  if (kind === WIRE.SNAPSHOT_VALUE_U32) return r.u32();
  if (kind === WIRE.SNAPSHOT_VALUE_U64) return r.u64();
  if (kind === WIRE.SNAPSHOT_VALUE_STRING) return r.string();
  if (kind === WIRE.SNAPSHOT_VALUE_BYTES)
    return r.raw(r.count(INSPECTED_BYTES_LIMIT));
  if (kind === WIRE.SNAPSHOT_VALUE_ENTITY) {
    if (r.u8() !== WIRE.SNAPSHOT_REF_HANDLE) fail("unresolved observed entity");
    return r.u64();
  }
  return fail("unsupported observed value");
}

function readLifecycleWatch(r: Reader): LifecycleWatchRecord {
  const world = readWorldReference(r);
  const output = r.u64();
  if (output === 0n) fail("zero lifecycle endpoint");
  const tag = r.u8();
  if (tag === WIRE.LIFECYCLE_WATCH_ACK) {
    const actionTag = r.u8();
    const action =
      actionTag === WIRE.LIFECYCLE_WATCH_ADD
        ? "add"
        : actionTag === WIRE.LIFECYCLE_WATCH_REMOVE
          ? "remove"
          : fail("membership action");
    const option = r.u8();
    const cut =
      option === WIRE.OPTION_NONE
        ? null
        : option === WIRE.OPTION_SOME
          ? { sequence: r.u64(), tick: r.u64() }
          : fail("membership cut");
    const result = r.u8();
    if (result === WIRE.LIFECYCLE_MEMBERSHIP_CANCELLED) {
      if (cut !== null) fail("cancelled membership cut");
      return {
        kind: "ack",
        world,
        output,
        action,
        cut,
        result: { kind: "cancelled" },
      };
    }
    if (!cut) fail("missing applied membership cut");
    if (result === WIRE.LIFECYCLE_MEMBERSHIP_REJECTED) {
      const reason =
        (
          [
            "StaleWorld",
            "StaleSession",
            "StaleMember",
            "AlreadyActive",
            "TrackingEnded",
            "Capacity",
          ] as const
        )[r.u8()] ?? fail("membership rejection");
      return {
        kind: "ack",
        world,
        output,
        action,
        cut,
        result: { kind: "rejected", reason },
      };
    }
    if (result !== WIRE.LIFECYCLE_MEMBERSHIP_APPLIED) fail("membership result");
    const count = r.count(LIFECYCLE_MEMBERS);
    if (count === 0) fail("empty membership baseline");
    const baselines: LifecycleBaseline[] = [];
    let previous = 0n;
    for (let index = 0; index < count; index++) {
      const generation = r.u64();
      if (generation <= previous) fail("unsorted membership baseline");
      previous = generation;
      const target = readLifecycleTarget(r);
      const lifetimeTag = r.u8();
      let lifetime: LifecycleTargetLifetime;
      if (lifetimeTag === WIRE.LIFECYCLE_LIFETIME_ENTITY)
        lifetime = { kind: "entity", live: r.boolean() };
      else if (lifetimeTag === WIRE.LIFECYCLE_LIFETIME_COMPONENT) {
        const entityLive = r.boolean();
        const incarnation = r.u64() || null;
        if (!entityLive && incarnation !== null)
          fail("component on absent entity");
        lifetime = { kind: "component", entityLive, incarnation };
      } else if (lifetimeTag === WIRE.LIFECYCLE_LIFETIME_REMOVED)
        lifetime = { kind: "removed" };
      else fail("membership lifetime");
      // A value target reports its component's lifetime.
      const lifetimeKind = target.kind === "value" ? "component" : target.kind;
      if (
        (action === "remove") !== (lifetime.kind === "removed") ||
        (lifetime.kind !== "removed" && lifetime.kind !== lifetimeKind)
      )
        fail("membership baseline kind");
      baselines.push({ member: { output, generation }, target, lifetime });
    }
    return {
      kind: "ack",
      world,
      output,
      action,
      cut,
      result: { kind: "applied", baselines },
    };
  }
  if (tag === WIRE.LIFECYCLE_WATCH_VALUE_RECORD) {
    const generation = r.u64();
    const tick = r.u64();
    if (generation === 0n) fail("lifecycle value identity");
    const option = r.u8();
    let values: LifecycleFieldValue[] | null = null;
    if (option === WIRE.OPTION_SOME) {
      const count = r.count(LIFECYCLE_VALUE_FIELDS);
      values = [];
      for (let index = 0; index < count; index++) {
        const offset = r.u32();
        values.push({ offset, value: readLifecycleFieldValue(r, r.u8()) });
      }
    } else if (option !== WIRE.OPTION_NONE) fail("lifecycle value presence");
    return {
      kind: "value",
      world,
      output,
      member: { output, generation },
      tick,
      values,
    };
  }
  if (tag !== WIRE.LIFECYCLE_WATCH_EVENT) fail("lifecycle record tag");
  const generation = r.u64();
  const sequence = r.u64();
  const tick = r.u64();
  if (generation === 0n || sequence === 0n) fail("lifecycle event identity");
  const change = r.u8();
  const entity = r.u64();
  if (entity === 0n) fail("zero lifecycle entity");
  let observation: Exclude<LifecycleObservation, { kind: "asset" }>;
  if (
    change === WIRE.LIFECYCLE_ENTITY_CREATED ||
    change === WIRE.LIFECYCLE_ENTITY_METADATA_CHANGED ||
    change === WIRE.LIFECYCLE_ENTITY_DELETED
  ) {
    observation = {
      kind: "entity",
      entity,
      change:
        change === WIRE.LIFECYCLE_ENTITY_CREATED
          ? "created"
          : change === WIRE.LIFECYCLE_ENTITY_METADATA_CHANGED
            ? "metadataChanged"
            : "deleted",
    };
  } else {
    const component = r.u16();
    const previousIncarnation = r.u64() || null;
    const incarnation = r.u64() || null;
    if (component === 0) fail("zero lifecycle component");
    const kind =
      change === WIRE.LIFECYCLE_COMPONENT_INSERTED
        ? "inserted"
        : change === WIRE.LIFECYCLE_COMPONENT_UPDATED
          ? "updated"
          : change === WIRE.LIFECYCLE_COMPONENT_REPLACED
            ? "replaced"
            : change === WIRE.LIFECYCLE_COMPONENT_REMOVED
              ? "removed"
              : fail("lifecycle observation tag");
    if (
      (kind === "inserted") !== (previousIncarnation === null) ||
      (kind === "removed") !== (incarnation === null)
    )
      fail("lifecycle incarnation transition");
    observation = {
      kind: "component",
      entity,
      component,
      change: kind,
      previousIncarnation,
      incarnation,
    };
  }
  return {
    kind: "event",
    world,
    output,
    member: { output, generation },
    sequence,
    tick,
    observation,
  };
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
    tag === WIRE.RESPONSE_LIFECYCLE_EVENTS;
  if (tag === WIRE.RESPONSE_RESOURCES) unsolicited = true;
  if (tag === WIRE.RESPONSE_RENDER_STATE_UPDATED) unsolicited = true;
  if (tag === WIRE.RESPONSE_PLAYBACK) unsolicited = true;
  if (tag === WIRE.RESPONSE_BATCH_ABORTED) unsolicited = true;
  if (tag === WIRE.RESPONSE_LIFECYCLE_WATCH) unsolicited = requestId === 0n;
  if (tag === WIRE.RESPONSE_GUI_OBSERVATION) unsolicited = requestId === 0n;
  if (unsolicited !== (requestId === 0n)) fail("reserved response identity");
  let body: ResponseBody;
  if (tag === WIRE.RESPONSE_LIFECYCLE_WATCH) {
    const record = readLifecycleWatch(r);
    if (tick !== 0n || (record.kind !== "ack") !== (requestId === 0n))
      fail("lifecycle watch envelope");
    body = { kind: "lifecycleWatch", record };
  } else if (tag === WIRE.RESPONSE_LIFECYCLE_SUBSCRIPTION)
    body = { kind: "lifecycleSubscription" };
  else if (tag === WIRE.RESPONSE_LIFECYCLE_DIAGNOSTICS) {
    if (tick !== 0n) fail("lifecycle diagnostic tick");
    const world = readWorldReference(r);
    const output = r.u64();
    if (output === 0n || world.id === 0n || world.incarnation === 0n)
      fail("lifecycle diagnostic endpoint");
    const work = {
      lookups: r.u64(),
      recipientVisits: r.u64(),
      saturated: r.boolean(),
    };
    const traffic = {
      queuedEvents: r.u64(),
      queuedBytes: r.u64(),
      saturated: r.boolean(),
    };
    body = {
      kind: "lifecycleDiagnostics",
      sample: { world, output, work, traffic },
    };
  } else if (tag === WIRE.RESPONSE_GUI_OBSERVATION) {
    const inner = new Reader(r.raw(r.count(GUI_OBSERVATION_BYTES)));
    const record = readGuiObservation(inner);
    inner.done();
    if (tick !== 0n || (record.kind === "effect") !== (requestId === 0n))
      fail("GUI observation envelope");
    body = { kind: "guiObservation", record };
  } else if (tag === WIRE.RESPONSE_LIFECYCLE_EVENTS)
    body = { kind: "lifecycleEvents", events: readLifecycleEvents(r, tick) };
  else if (tag === WIRE.RESPONSE_BATCH)
    body = { kind: "batch", outcome: readOutcome(r) };
  else if (tag === WIRE.RESPONSE_ATTACHMENT_RECEIPT) {
    const receipt = r.u64();
    const value = r.u8();
    const state =
      value === WIRE.RECEIPT_PENDING
        ? "pending"
        : value === WIRE.RECEIPT_RETIRED
          ? "retired"
          : value === WIRE.RECEIPT_RELEASED
            ? "released"
            : fail("invalid attachment receipt state");
    body = { kind: "attachmentReceipt", receipt, state };
  } else if (tag === WIRE.RESPONSE_BATCH_ABORTED)
    body = { kind: "batchAborted", batchId: r.u64(), message: r.string() };
  else if (tag === WIRE.RESPONSE_RESOURCES) {
    const count = r.count(RESOURCE_EVENT_RECORDS);
    if (count === 0) fail("empty resource event");
    body = { kind: "resources", resources: readResources(r, count, true) };
  } else if (tag === WIRE.RESPONSE_RENDER_STATE_UPDATED)
    body = { kind: "event", event: readRenderStateChange(r) };
  else if (tag === WIRE.RESPONSE_GEOMETRY_PICK)
    body = { kind: "event", event: readGeometryResult(r) };
  else if (tag === WIRE.RESPONSE_CAMERA_NAVIGATED)
    body = { kind: "cameraNavigated" };
  else if (tag === WIRE.RESPONSE_CAMERA_PROJECT)
    body = { kind: "event", event: readCameraProjection(r) };
  else if (tag === WIRE.RESPONSE_CONTROLLER) {
    const id = r.u64();
    body = { kind: "animationController", id: id === 0n ? null : id };
  } else if (tag === WIRE.RESPONSE_PLAYBACK) {
    const count = r.count(PLAYBACK_EVENTS);
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
      message: r.string(FAILURE_MESSAGE_BYTES),
    };
  } else if (tag === WIRE.RESPONSE_FRAME) {
    const time = r.f64();
    if (time < 0) fail("negative simulation time");
    body = { kind: "frame", time };
  } else if (tag === WIRE.RESPONSE_ENTITY_TREE) {
    const time = r.f64();
    if (time < 0) fail("negative simulation time");
    const next = r.u64();
    const count = r.count(ENTITY_TREE_PAGE);
    const nodes: EntityTreeNode[] = [];
    for (let index = 0; index < count; index++) {
      const id = r.u64();
      const parent = r.u64();
      const low = r.u64();
      const high = r.u64();
      const depth = r.u16();
      if (id === 0n || depth > ENTITY_TREE_DEPTH)
        fail("invalid entity tree node");
      nodes.push({
        id,
        parent: parent === 0n ? null : parent,
        order: low | (high << 64n),
        depth,
      });
    }
    body = { kind: "entityTree", time, next, nodes };
  } else if (tag === WIRE.RESPONSE_INSPECT) {
    const time = r.f64();
    if (time < 0) fail("negative simulation time");
    const next = r.u64();
    const n = r.count(INSPECTION_PAGE);
    const entities: EntitySnapshot[] = [];
    for (let i = 0; i < n; i++) {
      const id = r.u64();
      const metadata = readMetadata(r);
      const parent = r.u64();
      const low = r.u64();
      const high = r.u64();
      const link = {
        parent: parent === 0n ? null : parent,
        order: low | (high << 64n),
      };
      const components: ComponentSnapshot[] = [];
      const componentCount = r.count(INSPECTED_COMPONENTS);
      for (let j = 0; j < componentCount; j++)
        components.push(readComponent(r));
      entities.push({ id, metadata, link, components });
    }
    const count = r.count(INSPECTION_PAGE);
    const resources: AssetResourceSnapshot[] = [];
    resources.push(...readResources(r, count));
    if (count !== resources.length) fail("resource count");
    const diagnosticCount = r.count(INSPECTION_PAGE);
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
    const controllerCount = r.count(INSPECTION_PAGE);
    const controllers: AnimationControllerSnapshot[] = [];
    for (let i = 0; i < controllerCount; i++)
      controllers.push({
        ...readControllerState(r),
        description: readControllerDescription(r),
        ...readControllerTransitionState(r),
      });
    body.controllers = controllers;
    const focusCount = r.count(INSPECTION_PAGE);
    const guiFocus: GuiFocusRecord[] = [];
    for (let i = 0; i < focusCount; i++)
      guiFocus.push({ target: readGuiTarget(r), visible: r.boolean() });
    body.guiFocus = guiFocus;
    const pointerCount = r.count(INSPECTION_PAGE);
    const guiPointers: GuiPointerRecord[] = [];
    for (let i = 0; i < pointerCount; i++)
      guiPointers.push({
        target: readGuiTarget(r),
        pointer: r.u64(),
        state: {
          hovered: r.boolean(),
          pressed: r.boolean(),
          captured: r.boolean(),
        },
      });
    body.guiPointers = guiPointers;
    body.canvas = r.boolean() ? readCanvasStateRecord(r) : null;
  } else if (tag === WIRE.RESPONSE_ERROR)
    body = { kind: "error", code: r.u16(), message: r.string() };
  else return fail("unsupported response");
  r.done();
  return { session, requestId, tick, body };
}

/** A concrete client for this generated target. */
export class IppClient extends ClientBase {
  protected readonly lifecyclePageMembers = LIFECYCLE_PREFERRED_PAGE_MEMBERS;
  protected readonly commandPageLimits = COMMAND_PAGE_LIMITS;
  private hostConnection!: IppHostClient;

  get host(): IppHostClient {
    return this.hostConnection;
  }

  override readonly schemaHash = SCHEMA_HASH;
  override readonly components = components;

  /** Submit an ordered system action without allocating a reply waiter. */
  sendCommand(command: SystemCommand): void {
    this.submitCommand(command);
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

  subscribeGuiEffects(
    listener: (effect: GuiObservedEffect) => void,
    options: GuiObservationOptions = {},
  ): Promise<GuiEffectSubscription> {
    return this.submitGuiSubscription(listener, options);
  }

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
    options: WorldConnectOptions,
  ): Promise<IppClient> {
    validateWorldOptions(options);
    return IppClient.connectTransport(webSocketTransport(url), options);
  }

  /** Creates and owns a temporary World with the named Systems. An absent
   * selection is refused before the World request is sent. */
  static connectTransport(
    transport: MessageTransport,
    options: WorldConnectOptions,
  ): Promise<IppClient> {
    return IppHostClient.connectTransport(transport, options).then(
      async (host) => {
        const signal = options.signal;
        const abort = () => {
          void host.close().catch(() => {});
        };
        const requireActive = () => {
          if (signal?.aborted) throw new Error("Connection aborted");
        };
        signal?.addEventListener("abort", abort, { once: true });
        try {
          requireActive();
          const created = await host.createWorld({
            temporary: true,
            selectedSystems: options.selectedSystems,
          });
          requireActive();
          const world = await host.openWorld(created.reference);
          requireActive();
          host.ownWorldConnection(world.session);
          return world;
        } catch (error) {
          await host.close().catch(() => {});
          requireActive();
          throw error;
        } finally {
          signal?.removeEventListener("abort", abort);
        }
      },
    );
  }

  static connectWorker(
    workerUrl: string | URL,
    wasmUrl: string | URL,
    options: WorkerWorldConnectOptions,
  ): Promise<IppClient> {
    validateWorldOptions(options);
    return IppClient.connectTransport(
      workerTransport(workerUrl, wasmUrl, MAX_MESSAGE_BYTES, options),
      options,
    );
  }

  /** Construct the World client after Host attachment, without another hello. */
  static attachTransport(
    transport: MessageTransport,
    session: bigint,
    world: WorldDescriptor,
    manifest: WorldManifest,
    reference: WorldReference,
    options: ConnectOptions,
    host: IppHostClient,
  ): IppClient {
    const client = new IppClient(transport, options);
    client.hostConnection = host;
    return client.initializeAttached(session, world, manifest, reference);
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
      "entityBindings",
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
    if (property.entityLink === true) {
      exactFields(property, ["entityLink"]);
      w.u8(WIRE.ANIMATION_TARGET_ENTITY_LINK);
    } else if (property.name !== undefined) {
      exactFields(property, ["component", "name"]);
      w.u8(WIRE.ANIMATION_TARGET_DYNAMIC);
      w.u16(property.component!);
      w.string(property.name);
    } else if (property.joints !== undefined) {
      exactFields(property, ["joints"]);
      w.u8(WIRE.ANIMATION_TARGET_JOINTS);
      w.count(property.joints.length, ANIMATION_TARGET_INDICES);
      for (const index of property.joints) w.u32(index);
    } else {
      exactFields(property, ["component", "offsets"]);
      w.u8(WIRE.ANIMATION_TARGET_PROPERTY);
      w.u16(property.component!);
      const indices = property.offsets ?? [];
      w.count(indices.length, ANIMATION_TARGET_INDICES);
      for (const index of indices) w.u32(index);
    }
    const bindings = driver.entityBindings;
    if (property.entityLink === true && bindings === undefined)
      fail("structural animation bindings required");
    w.count(bindings?.length ?? 0, MAX_MESSAGE_BYTES / 8);
    for (const entity of bindings ?? []) w.u64(entity);
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
    const kind = r.u8();
    let property: AnimationDriverTarget;
    if (kind === WIRE.ANIMATION_TARGET_ENTITY_LINK)
      property = { entityLink: true };
    else if (kind === WIRE.ANIMATION_TARGET_DYNAMIC)
      property = { component: r.u16(), name: r.string() };
    else if (kind === WIRE.ANIMATION_TARGET_PROPERTY) {
      const component = r.u16();
      const count = r.count(ANIMATION_TARGET_INDICES);
      const offsets: number[] = [];
      for (let j = 0; j < count; j++) offsets.push(r.u32());
      property = { component, offsets };
    } else if (kind === WIRE.ANIMATION_TARGET_JOINTS) {
      const count = r.count(ANIMATION_TARGET_INDICES);
      const joints: number[] = [];
      for (let j = 0; j < count; j++) joints.push(r.u32());
      property = { joints };
    } else return fail("animation target kind");
    const bindingCount = r.count(MAX_MESSAGE_BYTES / 8);
    const entityBindings: bigint[] = [];
    for (let j = 0; j < bindingCount; j++) entityBindings.push(r.u64());
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
      entityBindings,
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
  protected readonly graphMetadataPageSize = GRAPH_METADATA_PAGE;
  protected readonly graphBindingPageSize = GRAPH_BINDING_PAGE;
  protected hostTag(name: string): number {
    return WIRE[name as keyof typeof WIRE] ?? fail("Host contract tag");
  }
  protected hostLimit(name: string): number {
    return HOST_LIMITS[name] ?? fail("Host contract limit");
  }
  protected hostMagic(response: boolean): Uint8Array<ArrayBuffer> {
    return new Uint8Array(response ? HOST_RESPONSE_MAGIC : HOST_REQUEST_MAGIC);
  }
  override readonly schemaHash = SCHEMA_HASH;
  protected override readonly protocolRevision = PROTOCOL_VERSION;

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
      workerTransport(workerUrl, wasmUrl, MAX_MESSAGE_BYTES, options),
      options,
    );
  }

  protected override createWorldClient(
    transport: MessageTransport,
    session: bigint,
    world: WorldDescriptor,
    manifest: WorldManifest,
    reference: WorldReference,
  ): IppClient {
    return IppClient.attachTransport(
      transport,
      session,
      world,
      manifest,
      reference,
      this.options,
      this,
    );
  }
}
