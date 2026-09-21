// Focused ownership, codec and correlation tests; no runtime/render integration claim.
import assert from "node:assert/strict";
import test from "node:test";
import { generateClient, replyToHostCreate } from "./generated-client.mjs";

const { codec, source } = await generateClient("texture", ["builtin-assets"]);
const minimal = await generateClient("texture-minimal");
const key = { kind: 2, asset: 0x7fffffffffffffffn, variant: 3 };
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

test("baseline texture contract retains component ID6 while built-in providers remain optional", () => {
  assert.deepEqual(codec.CAPABILITIES, {
    snapshot: true,
    animation: true,
    assets: true,
    stateOverlays: true,
    spatial: true,
    textures: true,
    builtinAssets: true,
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
  assert.equal(codec.components.UnlitTexture.id, 6);
  assert.deepEqual(
    codec.UnlitTexture.insert(codec.Entity.alias(1), {
      source: "ipp://mesh/cube?width=1&height=1&length=1",

      variant: key.variant,
    }).fields,
    [
      {
        offset: codec.components.UnlitTexture.fields.source.offset,
        value: {
          kind: "string",
          value: "ipp://mesh/cube?width=1&height=1&length=1",
        },
      },
      {
        offset: codec.components.UnlitTexture.fields.variant.offset,
        value: { kind: "u32", value: key.variant },
      },
    ],
  );
  for (const text of ["UnlitTexture", "ASSET_TEXTURE"])
    assert.equal(minimal.source.includes(text), true, text);
  assert.equal(minimal.codec.CAPABILITIES.textures, true);
  assert.equal(minimal.codec.CAPABILITIES.builtinAssets, false);
  assert.notEqual(codec.SCHEMA_HASH, minimal.codec.SCHEMA_HASH);
  assert.match(source, /IPPT;version=3/);
  assert.match(source, /rgba8-srgb-linear-alpha/);
  assert.match(source, /exact-payload/);
  assert.doesNotMatch(source, /exact-rgba8|max-dimension=1024/);
  assert.doesNotMatch(
    source,
    /REQUEST_LOAD_BUILTIN_TEXTURE|REQUEST_LOAD_BUILTIN_MESH/,
  );
});

test("texture resource events expose terminal state and log each failure exactly once", async () => {
  const failed = {
    id: 11n,
    kind: 2,
    source: "https://assets.example/legacy-v1.texture",

    variant: 3,
    status: "failed",
    representation: {
      decoded: false,
      graphicsReady: null,
      sourceBytes: 0n,
      residentBytes: 0n,
      graphicsBytes: null,
    },
    error: "InvalidAsset",
  };
  const ready = {
    id: 12n,
    kind: 2,
    source: "https://assets.example/checker.texture",

    variant: 0,
    status: "loaded",
    representation: {
      decoded: false,
      graphicsReady: null,
      sourceBytes: 0n,
      residentBytes: 0n,
      graphicsBytes: null,
    },
  };
  const packet = resourceEvent([failed, ready]);
  assert.equal(codec.WIRE.RESPONSE_RESOURCES, 9);
  assert.deepEqual(codec.decodeResponse(packet, 7n).body, {
    kind: "resources",
    resources: [failed, ready],
  });
  for (const symbol of ["RESPONSE_RESOURCES", "readResources"])
    assert.equal(minimal.source.includes(symbol), true, symbol);

  const fixture = await connect();
  const seen = [];
  const logs = [];
  const remove = fixture.client.onResourceChange((resource) =>
    seen.push(resource),
  );
  const originalError = console.error;
  console.error = (...args) => logs.push(args.join(" "));
  try {
    fixture.emit(packet);
    assert.deepEqual(seen, [failed, ready]);
    assert.deepEqual(logs, [
      `IPP 2 resource failed: ${failed.source}: ${failed.error}`,
    ]);
    remove();
    fixture.emit(resourceEvent([{ ...ready, id: 13n }], { tick: 2n }));
    assert.deepEqual(seen, [failed, ready]);
    assert.equal(logs.length, 1);
  } finally {
    console.error = originalError;
    await fixture.client.close();
  }
});

test("builtin request codecs and imperative consumer methods are absent", async () => {
  for (const selected of [codec, minimal.codec]) {
    const fixture = await connect({ selected });
    try {
      for (const name of [
        "uploadMesh",
        "uploadTexture",
        "loadBuiltinMesh",
        "loadBuiltinTexture",
      ])
        assert.equal(name in fixture.client, false);
      for (const kind of ["loadBuiltinMesh", "loadBuiltinTexture"])
        assert.throws(
          () =>
            selected.encodeRequest({
              session: 7n,
              requestId: 1n,
              body: {
                kind,
                key,
                uri: "ipp://mesh/cube?width=1&height=1&length=1",
              },
            }),
          /unsupported request/,
        );
    } finally {
      await fixture.client.close();
    }
  }
});

function resourceEvent(
  records,
  { session = 7n, requestId = 0n, tick = 1n } = {},
) {
  const bytes = new Uint8Array(32_768);
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
