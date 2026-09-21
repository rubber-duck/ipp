// Focused codec boundaries; real hosts run the shared render-state scenario.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  replyToHostCreate,
  encodeManifestLayout,
  manifestVariant,
} from "./generated-client.mjs";

const minimal = await generateClient("render-state-minimal");
const { codec, source } = await generateClient("render-state", []);
const layout = (name, values) => encodeManifestLayout(codec, name, values);
const tag = (name) => manifestVariant(codec, name);
const command = (changes) => ({ type: "RenderStateUpdateCommand", changes });
const request = (changes) =>
  codec.encodeRequest({
    session: 7n,
    requestId: 0n,
    body: { kind: "command", command: command(changes) },
  });
const patch = (changes) =>
  layout("render-state-patch", {
    mask:
      (Object.hasOwn(changes, "showAllDebugGeometries") ? 1 : 0) |
      (Object.hasOwn(changes, "debugGeometryColor") ? 2 : 0) |
      (Object.hasOwn(changes, "ambientLight") ? 4 : 0),
    ambientLight: changes.ambientLight
      ? layout("linear-rgb", {
          r: changes.ambientLight[0],
          g: changes.ambientLight[1],
          b: changes.ambientLight[2],
        })
      : null,
    showAllDebugGeometries: changes.showAllDebugGeometries ?? null,
    debugGeometryColor: changes.debugGeometryColor
      ? layout("linear-rgb", {
          r: changes.debugGeometryColor[0],
          g: changes.debugGeometryColor[1],
          b: changes.debugGeometryColor[2],
        })
      : null,
  });
function response(changes, tick = 1n, requestId = 0n) {
  return layout("response-render-state-updated", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_RENDER_STATE_UPDATED"),
    changes: patch(changes),
  }).bytes;
}
async function connect() {
  let handler;
  const sent = [];
  const client = await codec.IppClient.connectTransport(
    {
      start(events) {
        handler = events;
        events.ready();
      },
      send(bytes) {
        if (replyToHostCreate(bytes, handler)) return;
        sent.push(bytes.slice());
        if (sent.length === 1) {
          const reply = new Uint8Array(24);
          reply.set(bytes);
          new DataView(reply.buffer).setBigUint64(16, 7n, true);
          handler.message(reply);
        }
      },
      async close() {},
    },
    { logLevel: "off" },
  );
  return { client, sent, emit: (bytes) => handler.message(bytes) };
}

test("scene schemas retain render-state commands independently of debug rendering", () => {
  assert.equal(codec.CAPABILITIES.debugGeometry, true);
  assert.equal(codec.CAPABILITIES.spatial, true);
  assert.equal(codec.BoundingGeometry.fields.is_rendered.kind, 7);
  assert.equal(codec.BoundingGeometry.fields.is_rendered.default, false);
  assert.ok(source.includes("is_rendered?: boolean"));
  for (const name of [
    "REQUEST_RENDER_STATE_UPDATE",
    "RESPONSE_RENDER_STATE_UPDATED",
  ])
    assert.equal(name in minimal.codec.WIRE, true);
  assert.equal("BoundingGeometry" in minimal.codec.components, true);
  assert.equal(
    "onRenderStateUpdated" in minimal.codec.IppClient.prototype,
    true,
  );
  assert.equal(minimal.source.includes("function readRenderStatePatch"), true);
  assert.equal(codec.SCHEMA_HASH, minimal.codec.SCHEMA_HASH);
});

test("sparse commands conform to the manifest and notifications carry changed fields only", () => {
  for (const changes of [
    {},
    { showAllDebugGeometries: false },
    { showAllDebugGeometries: true },
    { debugGeometryColor: [0, 0.5, 1] },
    { ambientLight: [2, 0.5, 0] },
    {
      showAllDebugGeometries: false,
      debugGeometryColor: [1, 0, 0],
      ambientLight: [0, 0, 0],
    },
    { showAllDebugGeometries: true, debugGeometryColor: [1, 0.25, 0] },
  ]) {
    assert.deepEqual(
      request(changes),
      layout("request-render-state-update", {
        session: 7n,
        request_id: 0n,
        tag: tag("REQUEST_RENDER_STATE_UPDATE"),
        changes: patch(changes),
      }).bytes,
    );
    if (Object.keys(changes).length)
      assert.deepEqual(codec.decodeResponse(response(changes), 7n).body.event, {
        type: "RenderStateUpdatedEvent",
        changes,
      });
    else
      assert.throws(
        () => codec.decodeResponse(response(changes), 7n),
        /empty render state change/,
      );
  }
});

test("malformed local settings throw synchronously without sending or poisoning the session", async () => {
  const { client, sent } = await connect();
  try {
    for (const changes of [
      null,
      [],
      { unknown: true },
      { showAllDebugGeometries: undefined },
      { showAllDebugGeometries: 1 },
      { debugGeometryColor: [1, 0] },
      { debugGeometryColor: [NaN, 0, 0] },
      { debugGeometryColor: [2, 0, 0] },
      { debugGeometryColor: [-1, 0, 0] },
      { ambientLight: [-1, 0, 0] },
      { ambientLight: [NaN, 0, 0] },
      { ambientLight: [Infinity, 0, 0] },
      { ambientLight: [0, 0] },
    ])
      assert.throws(() => client.sendCommand(command(changes)), {
        code: "IPP_REQUEST_NOT_SENT",
      });
    assert.throws(() => client.sendCommand({ ...command({}), unknown: true }), {
      code: "IPP_REQUEST_NOT_SENT",
    });
    assert.equal(sent.length, 1);
    assert.equal(
      client.sendCommand(command({ showAllDebugGeometries: true })),
      undefined,
    );
    assert.equal(sent.length, 2);
  } finally {
    await client.close();
  }
});

test("render observers receive each sparse notification once with immutable sibling observations", async () => {
  const { client, sent, emit } = await connect();
  const seen = [];
  try {
    client.onRenderStateUpdated((event) => {
      event.changes.debugGeometryColor?.fill(1);
      event.changes.ambientLight?.fill(9);
      event.changes.showAllDebugGeometries = false;
      throw new Error("observer failed");
    });
    const remove = client.onRenderStateUpdated((event) => seen.push(event));
    const changes = {
      showAllDebugGeometries: true,
      debugGeometryColor: [0.25, 0.5, 0],
      ambientLight: [2, 0.5, 0],
    };
    client.sendCommand(command(changes));
    assert.equal(new DataView(sent.at(-1).buffer).getBigUint64(8, true), 0n);
    assert.equal(seen.length, 0);
    emit(response(changes));
    emit(response({ showAllDebugGeometries: false }));
    assert.deepEqual(seen, [
      {
        session: 7n,
        requestId: 0n,
        tick: 1n,
        type: "RenderStateUpdatedEvent",
        changes,
      },
      {
        session: 7n,
        requestId: 0n,
        tick: 1n,
        type: "RenderStateUpdatedEvent",
        changes: { showAllDebugGeometries: false },
      },
    ]);
    remove();
    emit(response(changes));
    assert.equal(seen.length, 2);
  } finally {
    await client.close();
  }
});

test("nonzero notification IDs, malformed masks, booleans, colors and stale sessions reject", () => {
  assert.throws(
    () =>
      codec.decodeResponse(
        response({ showAllDebugGeometries: true }, 1n, 9n),
        7n,
      ),
    /reserved response identity/,
  );
  assert.throws(
    () => codec.decodeResponse(response({ showAllDebugGeometries: true }), 8n),
    /session mismatch/,
  );
  for (const [offset, value] of [
    [25, 8],
    [27, 2],
  ]) {
    const bytes = response({ showAllDebugGeometries: true });
    bytes[offset] = value;
    assert.throws(() => codec.decodeResponse(bytes, 7n));
  }
  const color = response({ debugGeometryColor: [0, 0, 0] });
  new DataView(color.buffer).setFloat32(27, 2, true);
  assert.throws(
    () => codec.decodeResponse(color, 7n),
    /render state color range/,
  );
  const valid = response({ showAllDebugGeometries: true });
  assert.throws(() => codec.decodeResponse(valid.slice(0, -1), 7n));
  assert.throws(() => codec.decodeResponse(new Uint8Array([...valid, 0]), 7n));
});

test("a nonzero render notification cannot resolve a pending batch", async () => {
  const { client, emit } = await connect();
  const pending = client.batch([]);
  emit(response({ showAllDebugGeometries: true }, 1n, 1n));
  await assert.rejects(pending, /reserved response identity/);
  await client.close();
});
