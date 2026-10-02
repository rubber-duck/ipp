// Build actual target exports and shared production support for focused codec tests.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { assembleClientSupport } from "../../../packages/ipp-client/tools/assemble.mjs";

export async function generateClient(name, transformContract) {
  const root = resolve(import.meta.dirname, "../../..");
  const output = resolve(root, "target/protocol-client-tests", name);
  mkdirSync(output, { recursive: true });
  const contractPath = resolve(output, "native.contract");
  const exported = execFileSync(
    "cargo",
    [
      "run",
      "--quiet",
      "-p",
      "ipp-protocol",
      "--example",
      "export_contract",
      "--locked",
    ],
    { cwd: root, timeout: 180_000 },
  );
  writeFileSync(
    contractPath,
    transformContract ? transformContract(new Uint8Array(exported)) : exported,
  );
  execFileSync(
    "cargo",
    [
      "run",
      "--quiet",
      "-p",
      "ipp-schema-gen",
      "--locked",
      "--",
      contractPath,
      resolve(output, "generated.ts"),
    ],
    { cwd: root, timeout: 180_000 },
  );
  assembleClientSupport(output);
  execFileSync(
    process.execPath,
    [
      resolve(root, "node_modules/typescript/bin/tsc"),
      "--ignoreConfig",
      "--target",
      "ES2022",
      "--module",
      "NodeNext",
      "--moduleResolution",
      "NodeNext",
      "--strict",
      "--skipLibCheck",
      "--outDir",
      resolve(output, "js"),
      resolve(output, "generated.ts"),
      resolve(output, "generated-manifest.ts"),
    ],
    { cwd: root, stdio: "inherit", timeout: 30_000 },
  );
  return {
    codec: await import(pathToFileURL(resolve(output, "js/generated.js"))),
    hostProtocol: await import(
      pathToFileURL(resolve(output, "js/host-protocol.js"))
    ),
    hostPresentation: await import(
      pathToFileURL(resolve(output, "js/host-presentation.js"))
    ),
    manifest: await import(
      pathToFileURL(resolve(output, "js/generated-manifest.js"))
    ),
    logging: await import(pathToFileURL(resolve(output, "js/logging.js"))),
    source: readFileSync(resolve(output, "generated.ts"), "utf8"),
    manifestSource: readFileSync(
      resolve(output, "generated-manifest.ts"),
      "utf8",
    ),
  };
}

function scalarFieldContract(input) {
  let at = 16;
  const view = new DataView(input.buffer, input.byteOffset, input.byteLength);
  const u8 = () => view.getUint8(at++);
  const u16 = () => {
    const value = view.getUint16(at, true);
    at += 2;
    return value;
  };
  const u32 = () => {
    const value = view.getUint32(at, true);
    at += 4;
    return value;
  };
  const string = () => {
    const length = u32();
    const value = new TextDecoder().decode(input.slice(at, at + length));
    at += length;
    return value;
  };

  assertContract(u16() === 10, "target contract version");
  string();
  string();
  u8();
  // Rows bounds: region span, row fields, row properties, row text bytes.
  u32();
  u8();
  u16();
  u32();
  assertContract(u16() >= 1, "component fixture");
  u16();
  assertContract(string() === "Scalar", "scalar fixture");
  u32();
  u32();
  assertContract(u16() === 1, "scalar field fixture");
  const creation = at;
  assertContract(u8() === 1, "scalar creation fixture");
  assertContract(string() === "value", "scalar value fixture");
  u32();
  u32();
  u32();
  const kind = at;
  assertContract(u8() === 1, "scalar f32 kind");
  return { creation, kind };
}

export function noncreatableContract(input) {
  const { creation, kind } = scalarFieldContract(input);
  const contract = new Uint8Array(input.length - 4);
  contract.set(input.slice(0, kind + 1));
  contract[creation] = 0;
  contract.set(input.slice(kind + 5), kind + 1);
  return hashContract(contract);
}

export function nonemptyBytesDefaultContract(input) {
  const { kind } = scalarFieldContract(input);
  const replacement = new Uint8Array([6, 3, 0, 0, 0, 1, 2, 3]);
  const contract = new Uint8Array(input.length + 3);
  contract.set(input.slice(0, kind));
  contract.set(replacement, kind);
  contract.set(input.slice(kind + 5), kind + replacement.length);

  return hashContract(contract);
}

/** Scalar row layout of the synthetic rows contract, in property index order. */
export const ROWS_FIXTURE_PROPERTIES = [
  { name: "weight", kind: "f32", tag: 1, optional: false, hint: "none" },
  { name: "offset", kind: "vec3", tag: 6, optional: true, hint: "none" },
  { name: "rotation", kind: "vec4", tag: 7, optional: true, hint: "rotation" },
  { name: "enabled", kind: "bool", tag: 4, optional: false, hint: "none" },
  { name: "texture", kind: "asset", tag: 12, optional: true, hint: "none" },
  { name: "delta", kind: "i32", tag: 2, optional: false, hint: "none" },
  { name: "count", kind: "u32", tag: 3, optional: true, hint: "none" },
  {
    name: "label",
    kind: "text",
    tag: 13,
    optional: true,
    hint: "none",
    maxBytes: 8,
  },
];

/**
 * Give the exported Scalar a second, rows-kind field `items` at offset 8, with
 * region base 0x1000_0000 and ROWS_FIXTURE_PROPERTIES, as a target would export a
 * `#[schema(rows)]` field. No production component declares rows yet.
 */
export function rowsFieldContract(input) {
  const { creation, kind } = scalarFieldContract(input);
  const text = new TextEncoder();
  const bytes = [];
  const u8 = (value) => bytes.push(value);
  const u16 = (value) => bytes.push(value & 0xff, value >> 8);
  const u32 = (value) => {
    for (let shift = 0; shift < 32; shift += 8)
      bytes.push((value >>> shift) & 0xff);
  };
  const string = (value) => {
    const encoded = text.encode(value);
    u32(encoded.length);
    bytes.push(...encoded);
  };
  string("items");
  u32(8);
  u32(24);
  u32(8);
  u8(8);
  u32(0x10000000);
  u16(ROWS_FIXTURE_PROPERTIES.length);
  for (const property of ROWS_FIXTURE_PROPERTIES) {
    string(property.name);
    u8(property.tag);
    u8(property.optional ? 1 : 0);
    u8(property.hint === "rotation" ? 1 : 0);
    if (property.kind === "text") u32(property.maxBytes);
  }
  u32(8);
  bytes.push(0, 0, 0, 0, 0, 0, 0, 0);

  const prefix = input.slice(0, kind + 5);
  const view = new DataView(
    prefix.buffer,
    prefix.byteOffset,
    prefix.byteLength,
  );
  view.setUint32(creation - 10, 32, true);
  view.setUint32(creation - 6, 8, true);
  view.setUint16(creation - 2, 2, true);
  const suffix = input.slice(kind + 5);
  const contract = new Uint8Array(prefix.length + bytes.length + suffix.length);
  contract.set(prefix);
  contract.set(bytes, prefix.length);
  contract.set(suffix, prefix.length + bytes.length);
  return hashContract(contract);
}

function hashContract(contract) {
  let hash = 0xcbf29ce484222325n;
  for (const byte of contract.slice(16))
    hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  new DataView(contract.buffer).setBigUint64(8, hash, true);
  return contract;
}

function assertContract(condition, message) {
  if (!condition) throw new Error(`invalid synthetic contract: ${message}`);
}

const TAG_SPACES = {
  1: "value",
  2: "request",
  3: "command",
  4: "reference",
  5: "response",
  6: "outcome",
  8: "option",
  13: "resource-status",
  15: "snapshot-value",
  16: "snapshot-reference",
  18: "geometry-pick-outcome",
  19: "lifecycle-observation",
  20: "batch-error-scope",
  21: "runtime-failure-scope",
  22: "inspection-collection",
  23: "host-request",
  24: "host-response",
  25: "world-selector",
  26: "animation-target",
  27: "playback-control",
  28: "playback-state",
  29: "playback-event",
  32: "output-kind",
  33: "view-target",
  34: "operation-effect",
  46: "gui-physical-request",
  47: "gui-physical-event",
  48: "gui-physical-response",
  49: "gui-physical-button",
  50: "gui-physical-key",
  51: "gui-native-edit",
  52: "gui-physical-disposition",
  53: "gui-action",
  54: "output-target",
};

export function manifestVariant(client, name) {
  const descriptor = client.manifest.WIRE_TAG_LAYOUTS[name];
  if (!descriptor) throw new Error(`unknown manifest tag ${name}`);
  return {
    space: TAG_SPACES[descriptor.space],
    value: client.codec.WIRE[name],
  };
}

export function encodeManifestLayout(client, name, values) {
  const descriptor = client.manifest.WIRE_LAYOUTS[name];
  if (!descriptor) throw new Error(`unknown manifest layout ${name}`);
  const fields = descriptor.fields;
  const chunks = [];
  for (const field of fields) {
    if (!Object.hasOwn(values, field.name))
      throw new Error(`missing ${name}.${field.name}`);
    const value = values[field.name];
    if (field.encoding === "bool") chunks.push(integer(value ? 1 : 0, 1));
    else if (field.encoding === "masked") {
      if (Boolean(values.mask & field.limit) !== (value !== null))
        throw new Error("manifest presence mask");
      if (value !== null) chunks.push(nested(client, field.target, value));
    } else if (field.encoding === "u16") chunks.push(integer(value, 2));
    else if (field.encoding === "u32") chunks.push(integer(value, 4));
    else if (field.encoding === "u64") chunks.push(integer(value, 8));
    else if (field.encoding === "finite-f32") chunks.push(float(value, 4));
    else if (field.encoding === "nonnegative-finite-f64")
      chunks.push(float(value, 8));
    else if (field.encoding === "utf8") {
      const bytes = new TextEncoder().encode(value);
      if (bytes.length > field.limit) throw new Error("manifest utf8 limit");
      chunks.push(integer(bytes.length, 4), bytes);
    } else if (field.encoding === "bytes") {
      if (!(value instanceof Uint8Array) || value.length > field.limit)
        throw new Error("manifest byte limit");
      chunks.push(integer(value.length, 4), value);
    } else if (field.encoding === "named")
      chunks.push(nested(client, field.target, value));
    else if (["list", "u8-counted-list"].includes(field.encoding)) {
      if (!Array.isArray(value) || value.length > field.limit)
        throw new Error("manifest list limit");
      chunks.push(integer(value.length, field.encoding === "list" ? 4 : 1));
      for (const item of value) chunks.push(nested(client, field.target, item));
    } else if (field.encoding === "option") {
      chunks.push(
        integer(
          value === null
            ? client.codec.WIRE.OPTION_NONE
            : client.codec.WIRE.OPTION_SOME,
          1,
        ),
      );
      if (value !== null) chunks.push(nested(client, field.target, value));
    } else if (field.encoding === "variant") {
      if (value.space !== field.target)
        throw new Error(`manifest variant space ${name}.${field.name}`);
      chunks.push(integer(value.value, 1));
    } else if (field.encoding === "union") {
      chunks.push(nested(client, field.target, value));
    } else throw new Error(`unknown manifest encoding ${field.encoding}`);
  }
  return { layout: name, bytes: concatenate(chunks) };
}

function nested(client, target, value) {
  if (target === "bool") return integer(value ? 1 : 0, 1);
  if (["u8", "u16", "u32", "u64", "utf8-65536"].includes(target)) {
    if (target === "u8") return integer(value, 1);
    if (target === "u16") return integer(value, 2);
    if (target === "u32") return integer(value, 4);
    if (target === "u64") return integer(value, 8);
    const bytes = new TextEncoder().encode(value);
    return concatenate([integer(bytes.length, 4), bytes]);
  }
  if (target === "empty") {
    if (value instanceof Uint8Array && value.length === 0) return value;
    throw new Error("nonempty manifest placeholder");
  }
  if (!value || !(value.bytes instanceof Uint8Array))
    throw new Error(`manifest nested value for ${target}`);
  if (value.layout === target) return value.bytes;
  const tag = Object.values(client.manifest.WIRE_TAG_LAYOUTS).find(
    (entry) =>
      entry.layout === value.layout && TAG_SPACES[entry.space] === target,
  );
  if (!tag)
    throw new Error(`manifest nested layout ${value.layout} for ${target}`);
  return value.bytes;
}

function integer(value, width) {
  const bytes = new Uint8Array(width);
  const view = new DataView(bytes.buffer);
  if (width === 1) view.setUint8(0, value);
  else if (width === 2) view.setUint16(0, value, true);
  else if (width === 4) view.setUint32(0, value, true);
  else view.setBigUint64(0, BigInt(value), true);
  return bytes;
}

function float(value, width) {
  const bytes = new Uint8Array(width);
  const view = new DataView(bytes.buffer);
  if (width === 4) view.setFloat32(0, value, true);
  else view.setFloat64(0, value, true);
  return bytes;
}

function concatenate(chunks) {
  const bytes = new Uint8Array(
    chunks.reduce((length, chunk) => length + chunk.length, 0),
  );
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.length;
  }
  return bytes;
}

/** Controlled Host attachment for focused World-codec tests; real Hosts have separate suites. */
/** The Host's reply to the hello: the contract's own revision and hash and
 * the connection identity, as a matching Host announces them. */
export function hostAnnouncement(codec, connection = 7n) {
  const reply = new Uint8Array(24);
  reply.set([73, 80, 80, 66]);
  const view = new DataView(reply.buffer);
  view.setUint32(4, codec.PROTOCOL_VERSION, true);
  view.setBigUint64(8, codec.SCHEMA_HASH, true);
  view.setBigUint64(16, connection, true);
  return reply;
}

/** The hello every connection opens with. */
export const HELLO = Uint8Array.of(73, 80, 80, 66);

export function replyToHostCreate(bytes, events, session = 7n) {
  if (String.fromCharCode(...bytes.subarray(0, 4)) !== "IPPH") return false;
  const requestTag = bytes[24];
  assert.ok([2, 3, 6].includes(requestTag), "Expected create, open or detach");
  const descriptor = Buffer.alloc(8 + 4 + 4 + 16 + 4 + 4);
  let at = 0;
  descriptor.writeBigUInt64LE(1n, at);
  at += 8;
  descriptor.writeUInt32LE(4, at);
  at += 4;
  descriptor.write("test", at);
  at += 4;
  descriptor.writeBigUInt64LE(1n, at);
  at += 16;
  descriptor.writeUInt32LE(256, at);
  const reference = Buffer.alloc(16);
  reference.writeBigUInt64LE(1n, 0);
  reference.writeBigUInt64LE(1n, 8);
  const sessionManifest = Buffer.alloc(8 + 4 + 4 + 4 + 1);
  sessionManifest.writeBigUInt64LE(session, 0);
  sessionManifest.writeUInt32LE(1, 16);
  const body =
    requestTag === 2
      ? Buffer.concat([descriptor, reference])
      : requestTag === 3
        ? Buffer.concat([descriptor, sessionManifest, reference])
        : Buffer.alloc(0);
  const reply = new Uint8Array(25 + body.length);
  reply.set(bytes.subarray(0, 24));
  reply[3] = 65;
  reply[24] = requestTag === 2 ? 12 : requestTag === 3 ? 2 : 4;
  reply.set(body, 25);
  events.message(reply);
  return true;
}
