// Focused ownership, codec and correlation tests; no runtime/render integration claim.
import assert from "node:assert/strict";
import test from "node:test";
import { generateClient, replyToHostCreate } from "./generated-client.mjs";

const { codec, source } = await generateClient("mesh", []);
const minimal = await generateClient("mesh-minimal");
const key = { kind: 1, asset: 0x7fffffffffffffffn, variant: 3 };
const stats = { sourceBytes: 664, residentBytes: 648 };

async function connect({
  parts = true,
  selected = codec,
  timeoutMs = 1000,
} = {}) {
  let events;
  const sent = [];
  const transport = {
    start(value) {
      events = value;
      events.ready();
    },
    send(bytes) {
      if (replyToHostCreate(bytes, events)) return;
      sent.push([bytes]);
      if (sent.length === 1) {
        const reply = new Uint8Array(24);
        reply.set(bytes);
        new DataView(reply.buffer).setBigUint64(16, 7n, true);
        events.message(reply);
      }
    },
    async close() {},
  };
  if (parts)
    transport.sendParts = (parts) => {
      sent.push(parts);
    };
  const client = await selected.IppClient.connectTransport(transport, {
    timeoutMs,
  });
  return { client, sent, emit: (bytes) => events.message(bytes) };
}

test("baseline generation exposes standard scene descriptors and codecs", () => {
  assert.deepEqual(codec.CAPABILITIES, {
    snapshot: true,
    animation: true,
    assets: true,
    stateOverlays: true,
    spatial: true,
    textures: true,
    builtinAssets: false,
    picking: true,
    debugGeometry: true,
    pbr: true,
    shadows: false,
    skeletalAnimation: false,
    meshPoses: false,
    particles: false,
    surfaces: false,
    gui: false,
  });
  assert.deepEqual(minimal.codec.CAPABILITIES, {
    snapshot: true,
    animation: true,
    assets: true,
    stateOverlays: true,
    spatial: true,
    textures: true,
    builtinAssets: false,
    picking: true,
    debugGeometry: true,
    pbr: true,
    shadows: false,
    skeletalAnimation: false,
    meshPoses: false,
    particles: false,
    surfaces: false,
    gui: false,
  });
  for (const name of ["Transform", "UnlitMaterial", "MeshInstance"])
    assert.equal(name in minimal.codec, true);
  for (const method of ["registerAsset", "createAsset", "onResourceChange"]) {
    assert.equal(method in codec.IppClient.prototype, true);
    assert.equal(method in minimal.codec.IppClient.prototype, true);
  }
  for (const text of ["REQUEST_UPLOAD_ASSET"])
    assert.equal(minimal.source.includes(text), false, text);
  for (const name of [
    "VALUE_F32",
    "VALUE_ENTITY",
    "VALUE_U32",
    "VALUE_U64",
    "VALUE_STRING",
    "VALUE_BYTES",
  ])
    assert.equal(name in minimal.codec.WIRE, true, name);
  assert.deepEqual(minimal.codec.WIRE_LAYOUTS["value-bytes"], {
    capability: "base",
    fields: [
      { name: "tag", encoding: "variant", limit: 0, target: "value" },
      { name: "value", encoding: "bytes", limit: 65536, target: "" },
    ],
  });
  assert.match(source, /WorkerConnectOptions/);
  assert.match(source, /workerTransport\(workerUrl, wasmUrl, options\)/);
  const mesh = codec.MeshInstance.insert(codec.Entity.alias(1), {
    source: "ipp://mesh/cube?width=1&height=1&length=1",

    variant: key.variant,
  });
  assert.deepEqual(mesh.fields, [
    {
      offset: codec.components.MeshInstance.fields.source.offset,
      value: {
        kind: "string",
        value: "ipp://mesh/cube?width=1&height=1&length=1",
      },
    },
    {
      offset: codec.components.MeshInstance.fields.variant.offset,
      value: { kind: "u32", value: key.variant },
    },
  ]);
  const request = {
    session: 7n,
    requestId: 1n,
    body: { kind: "batch", batch: { id: 1n, operations: [mesh] } },
  };
  assert.ok(codec.encodeRequest(request).length > 0);
  assert.ok(
    minimal.codec.encodeRequest(request).length > 0,
    "baseline field codecs use the same component contract",
  );
  for (const value of [-1, 0x1_00000000, 1.5, NaN]) {
    request.body.batch.operations = [
      codec.MeshInstance.setVariant(codec.Entity.alias(1), value),
    ];
    assert.throws(() => codec.encodeRequest(request), /integer out of range/);
  }
});

test("inspection decodes typed scene fields by target offset without losing u64 precision", () => {
  const bytes = new Uint8Array(256);
  const view = new DataView(bytes.buffer);
  let at = 0;
  const u8 = (value) => {
    view.setUint8(at, value);
    at += 1;
  };
  const u16 = (value) => {
    view.setUint16(at, value, true);
    at += 2;
  };
  const u32 = (value) => {
    view.setUint32(at, value, true);
    at += 4;
  };
  const u64 = (value) => {
    view.setBigUint64(at, value, true);
    at += 8;
  };
  u64(7n);
  u64(1n);
  u64(5n);
  u8(codec.WIRE.RESPONSE_INSPECT);
  view.setFloat64(at, 0.5, true);
  at += 8;
  u64(0n);
  u32(1);
  u64(0x1_00000001n);
  u8(0);
  u32(0);
  const descriptor = codec.components.MeshInstance;
  const mesh = {
    source: "https://example.test/é.ippm",

    variant: 0xffffffff,
  };
  for (const _layer of ["base", "effective"]) {
    u32(1);
    u16(descriptor.id);
    u32(2);
    for (const [name, field] of Object.entries(descriptor.fields)) {
      u32(field.offset);
      u8(field.kind);
      if (field.kind === 5) {
        const encoded = new TextEncoder().encode(mesh[name]);
        u32(encoded.length);
        bytes.set(encoded, at);
        at += encoded.length;
      } else if (field.kind === 4) u64(mesh[name]);
      else u32(mesh[name]);
    }
  }
  u32(0);
  u32(0);
  u32(0); // Baseline animation controller snapshots.
  const response = codec.decodeResponse(bytes.slice(0, at), 7n);
  assert.equal(response.body.kind, "inspect");
  const entity = response.body.entities[0];
  for (const layer of ["base", "effective"])
    assert.deepEqual({ ...entity[layer][0].fields }, mesh);
});

test("source command strings use owned kind5 with strict bounded UTF8", () => {
  const request = (source) => ({
    session: 7n,
    requestId: 1n,
    body: {
      kind: "batch",
      batch: {
        id: 1n,
        operations: [
          codec.MeshInstance.setSource(codec.Entity.alias(1), source),
        ],
      },
    },
  });
  for (const source of ["", "https://example.test/é.ippm", "é".repeat(32768)]) {
    const bytes = codec.encodeRequest(request(source));
    const sourceBytes = new TextEncoder().encode(source);
    assert.deepEqual(
      bytes.slice(-sourceBytes.length || bytes.length),
      sourceBytes,
    );
    assert.equal(bytes[41], codec.WIRE.VALUE_STRING);
    assert.equal(
      new DataView(bytes.buffer).getUint32(42, true),
      sourceBytes.length,
    );
  }
  for (const invalid of ["é".repeat(32769), "\ud800", "\udfff", 4n])
    assert.throws(() => codec.encodeRequest(request(invalid)));
});

test("resource inspection preserves typed status, stable handles and bounds", () => {
  const records = [
    {
      id: 1n,
      kind: 1,
      source: "https://example.test/é.ippm",

      variant: 0xffffffff,
      status: "start",
      representation: {
        decoded: false,
        graphicsReady: null,
        sourceBytes: 0n,
        residentBytes: 0n,
        graphicsBytes: null,
      },
    },
    {
      id: 2n,
      kind: 1,
      source: "ipp://mesh/cube?width=1&height=1&length=1",

      variant: 0,
      status: "loaded",
      representation: {
        decoded: false,
        graphicsReady: null,
        sourceBytes: 0n,
        residentBytes: 0n,
        graphicsBytes: null,
      },
    },
    {
      id: 3n,
      kind: 1,
      source: "unknown://mesh",

      variant: 0,
      status: "failed",
      representation: {
        decoded: false,
        graphicsReady: null,
        sourceBytes: 0n,
        residentBytes: 0n,
        graphicsBytes: null,
      },
      error: "Unsupported provider",
    },
  ];
  function encode(records, diagnostics = []) {
    const bytes = new Uint8Array(1024);
    const view = new DataView(bytes.buffer);
    let at = 0;
    const u8 = (v) => {
      view.setUint8(at, v);
      at += 1;
    };
    const u32 = (v) => {
      view.setUint32(at, v, true);
      at += 4;
    };
    const u64 = (v) => {
      view.setBigUint64(at, v, true);
      at += 8;
    };
    const string = (v) => {
      const b = new TextEncoder().encode(v);
      u32(b.length);
      bytes.set(b, at);
      at += b.length;
    };
    u64(7n);
    u64(1n);
    u64(9n);
    u8(3);
    view.setFloat64(at, 0.5, true);
    at += 8;
    u64(0n);
    u32(0);
    u32(records.length);
    for (const record of records) {
      u64(record.id);
      view.setUint16(at, record.kind, true);
      at += 2;
      string(record.source);
      u32(record.variant);
      u8(
        ["unloaded", "start", "progress", "loaded", "failed"].indexOf(
          record.status,
        ),
      );
      if (record.status === "failed") string(record.error);
      u8(0);
      u8(0);
      u64(0n);
      u64(0n);
      u8(0);
    }
    u32(diagnostics.length);
    for (const diagnostic of diagnostics) {
      u64(diagnostic.entity);
      string(diagnostic.reason);
    }
    u32(0); // Baseline animation controller snapshots.
    return bytes.slice(0, at);
  }
  const diagnostics = [{ entity: 0x100000001n, reason: "InvalidAsset" }];
  assert.deepEqual(
    codec.decodeResponse(encode(records, diagnostics), 7n).body
      .renderDiagnostics,
    diagnostics,
  );
  assert.deepEqual(
    minimal.codec.decodeResponse(encode([]), 7n).body.renderDiagnostics,
    [],
  );
  assert.deepEqual(
    minimal.codec.decodeResponse(encode([], diagnostics), 7n).body
      .renderDiagnostics,
    diagnostics,
  );
  assert.throws(() =>
    codec.decodeResponse(
      encode([], [{ entity: 0n, reason: "InvalidAsset" }]),
      7n,
    ),
  );
  const bytes = encode(records);
  assert.deepEqual(codec.decodeResponse(bytes, 7n).body.resources, records);
  for (let n = 0; n < bytes.length; n++)
    assert.throws(() => codec.decodeResponse(bytes.slice(0, n), 7n));
  for (const bad of [
    { id: 0n },
    { kind: 0 },
    { source: "" },
    { status: "other" },
  ])
    assert.throws(() =>
      codec.decodeResponse(encode([{ ...records[0], ...bad }]), 7n),
    );
  assert.throws(
    () => codec.decodeResponse(encode([records[0], records[0]]), 7n),
    /resource identity/,
  );
  assert.deepEqual(
    minimal.codec.decodeResponse(bytes, 7n).body.resources,
    records,
  );
  assert.deepEqual(
    minimal.codec.decodeResponse(encode([]), 7n).body.resources,
    [],
  );
  const oversized = encode([]);
  new DataView(oversized.buffer).setUint32(45, 257, true);
  assert.throws(() => codec.decodeResponse(oversized, 7n));
});

test("resource event decoder validates event bounds, sessions and baseline resource support", async () => {
  assert.equal(codec.WIRE.RESPONSE_RESOURCES, 9);
  for (const symbol of ["RESPONSE_RESOURCES", "readResources"])
    assert.equal(minimal.source.includes(symbol), true);
  assert.equal(codec.SCHEMA_HASH, minimal.codec.SCHEMA_HASH);
  const records = Array.from({ length: 16 }, (_, i) => ({
    ...failedResource,
    id: BigInt(i + 1),
    source: "é".repeat(2049),
    error: "é".repeat(1024),
  }));
  assert.deepEqual(codec.decodeResponse(resourceEvent(records), 7n).body, {
    kind: "resources",
    resources: records,
  });
  for (const records of [
    [],
    Array(129).fill(failedResource),
    [{ ...failedResource, id: 0n }],
    [{ ...failedResource, source: "é".repeat(32769) }],
    [{ ...failedResource, error: "é".repeat(1025) }],
    [{ ...failedResource, kind: 0 }],
  ])
    assert.throws(() => codec.decodeResponse(resourceEvent(records), 7n));
  const bytes = resourceEvent([failedResource]);
  for (let n = 0; n < bytes.length; n++)
    assert.throws(() => codec.decodeResponse(bytes.slice(0, n), 7n));
  assert.throws(() => codec.decodeResponse(new Uint8Array([...bytes, 0]), 7n));
  assert.throws(() =>
    codec.decodeResponse(
      resourceEvent([failedResource], { requestId: 1n }),
      7n,
    ),
  );
  assert.throws(() => codec.decodeResponse(bytes, 8n));
  assert.deepEqual(
    minimal.codec.decodeResponse(bytes, 7n),
    codec.decodeResponse(bytes, 7n),
  );
  const textures = await generateClient("resource-textures", []);
  const texture = { ...failedResource, kind: 2 };
  assert.deepEqual(
    textures.codec.decodeResponse(resourceEvent([texture]), 7n).body.resources,
    [texture],
  );
  assert.match(textures.source, /IPPT;version=3/);
  assert.match(textures.source, /rgba8-srgb-linear-alpha/);
  assert.match(textures.source, /exact-payload/);
  assert.doesNotMatch(textures.source, /exact-rgba8|max-dimension=1024/);
  assert.equal(textures.codec.MAX_MESSAGE_BYTES, 1_048_576);
});

test("resource events log failures once without polling and isolate optional listeners", async () => {
  const h = await connect();
  const logs = [];
  const originalError = console.error;
  const originalReport = globalThis.reportError;
  console.error = (...args) => logs.push(args.join(" "));
  globalThis.reportError = () => {
    throw new Error("reporter failed");
  };
  const seen = [];
  let removed = 0;
  const remove = h.client.onResourceChange(() => removed++);
  remove();
  h.client.onResourceChange((resource) => {
    resource.source = "mutated";
    throw new Error("observer failed");
  });
  h.client.onResourceChange((resource) => seen.push(resource));
  try {
    h.emit(resourceEvent([failedResource]));
    assert.deepEqual(seen, [failedResource]);
    assert.equal(removed, 0);
    assert.equal(logs.length, 1);
    for (const text of [
      failedResource.source,
      failedResource.kind,
      failedResource.error,
    ])
      assert.ok(logs[0].includes(text));
    const ready = { ...failedResource, id: 2n, status: "loaded" };
    delete ready.error;
    h.emit(resourceEvent([ready], { tick: 2n }));
    for (let tick = 2n; tick <= 5n; tick++) {
      const frame = new Uint8Array(33);
      const view = new DataView(frame.buffer);
      view.setBigUint64(0, 7n, true);
      view.setBigUint64(16, tick, true);
      view.setUint8(24, 4);
      view.setFloat64(25, Number(tick), true);
      const waiting = h.client.waitForFrame(tick - 1n);
      h.emit(frame);
      assert.equal((await waiting).tick, tick);
    }
    assert.equal(logs.length, 1);
    assert.equal(h.sent.length, 1); // Only bootstrap; no application polling.
    assert.deepEqual(seen, [failedResource, ready]);
    await h.client.close();
    h.emit(resourceEvent([{ ...failedResource, id: 3n }], { tick: 6n }));
    assert.equal(logs.length, 1);
    assert.equal(seen.length, 2);
    assert.throws(() => h.client.onResourceChange(() => {}), /closed/);
  } finally {
    console.error = originalError;
    globalThis.reportError = originalReport;
    await h.client.close();
  }
});

function resourceEvent(
  records,
  { session = 7n, requestId = 0n, tick = 1n } = {},
) {
  const bytes = new Uint8Array(150_000);
  const view = new DataView(bytes.buffer);
  let at = 0;
  const u8 = (value) => {
    view.setUint8(at, value);
    at++;
  };
  const u32 = (value) => {
    view.setUint32(at, value, true);
    at += 4;
  };
  const u64 = (value) => {
    view.setBigUint64(at, value, true);
    at += 8;
  };
  const string = (value) => {
    const encoded = new TextEncoder().encode(value);
    u32(encoded.length);
    bytes.set(encoded, at);
    at += encoded.length;
  };
  u64(session);
  u64(requestId);
  u64(tick);
  u8(9);
  u32(records.length);
  for (const resource of records) {
    u64(resource.id);
    view.setUint16(at, resource.kind, true);
    at += 2;
    string(resource.source);
    u32(resource.variant);
    u8(
      ["unloaded", "start", "progress", "loaded", "failed"].indexOf(
        resource.status,
      ),
    );
    if (resource.status === "failed") string(resource.error);
    u8(0);
    u8(0);
    u64(0n);
    u64(0n);
    u8(0);
  }
  return bytes.slice(0, at);
}

const failedResource = {
  id: 1n,
  kind: 1,
  source: "unknown://broken",

  variant: 0xffffffff,
  status: "failed",
  representation: {
    decoded: false,
    graphicsReady: null,
    sourceBytes: 0n,
    residentBytes: 0n,
    graphicsBytes: null,
  },
  error: "Unsupported provider",
};
