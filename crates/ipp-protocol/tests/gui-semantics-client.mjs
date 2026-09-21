// GUI semantic snapshot/action codec boundaries through a real generated client.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  replyToHostCreate,
  encodeManifestLayout,
  manifestVariant,
} from "./generated-client.mjs";

const { codec } = await generateClient("gui-semantics", ["surfaces", "gui"]);
const layout = (name, values) => encodeManifestLayout(codec, name, values);
const tag = (name) => manifestVariant(codec, name);

function concatenate(chunks) {
  const total = chunks.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of chunks) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

function u8(value) {
  return Uint8Array.of(value);
}

function u16(value) {
  const bytes = new Uint8Array(2);
  new DataView(bytes.buffer).setUint16(0, value, true);
  return bytes;
}

function u32(value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setUint32(0, value, true);
  return bytes;
}

function u64(value) {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  return bytes;
}

function f32(value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setFloat32(0, value, true);
  return bytes;
}

function text(value) {
  const bytes = new TextEncoder().encode(value);
  return concatenate([u32(bytes.length), bytes]);
}

const queryBytes = (entity = 42n, maxDepth = 32, limit = 256) =>
  concatenate([u8(1), u64(entity), u32(maxDepth), u32(limit)]);

const actionBytes = (kind, payload = new Uint8Array()) =>
  concatenate([
    u8(2),
    u64(42n),
    u64(3n),
    u32(9),
    u32(1),
    u32(2),
    u8(kind),
    payload,
  ]);

const snapshotBytes = () =>
  concatenate([
    u8(1),
    u64(42n),
    u64(3n),
    u64(12n),
    u32(2),
    // Container root: id, no parent, lifetime, role, no name, no value,
    // revision, bounds, enabled, visible, available, no actions.
    u32(1),
    u32(0),
    u32(1),
    u8(0),
    u8(0),
    u8(0),
    u32(0),
    f32(0),
    f32(0),
    f32(10),
    f32(10),
    u8(1),
    u8(1),
    u8(1),
    u8(0),
    // Button: parent, role, name, revision, bounds, flags, one action.
    u32(2),
    u32(1),
    u32(1),
    u8(4),
    u8(1),
    text("Go"),
    u8(0),
    u32(0),
    f32(1),
    f32(1),
    f32(2),
    f32(1),
    u8(1),
    u8(1),
    u8(1),
    u8(1),
    u8(0),
    // Observed focus.
    u8(1),
    u32(2),
    u32(1),
  ]);

const snapshotTree = {
  entity: 42n,
  rootIncarnation: 3n,
  evaluationTick: 12n,
  nodes: [
    {
      id: 1,
      lifetime: 1,
      role: "container",
      value: { kind: "none" },
      revision: 0,
      bounds: [0, 0, 10, 10],
      enabled: true,
      visible: true,
      available: true,
      actions: [],
    },
    {
      id: 2,
      lifetime: 1,
      parent: 1,
      role: "button",
      name: "Go",
      value: { kind: "none" },
      revision: 0,
      bounds: [1, 1, 2, 1],
      enabled: true,
      visible: true,
      available: true,
      actions: ["press"],
    },
  ],
  focused: { id: 2, lifetime: 1 },
};

test("semantic wire branches use their manifest tags and layouts", () => {
  assert.deepEqual(tag("REQUEST_GUI_SEMANTIC_SNAPSHOT"), {
    space: "request",
    value: 31,
  });
  assert.deepEqual(tag("REQUEST_GUI_SEMANTIC_ACTION"), {
    space: "request",
    value: 32,
  });
  assert.deepEqual(tag("RESPONSE_GUI_SEMANTIC_SNAPSHOT"), {
    space: "response",
    value: 33,
  });
  for (const name of [
    "request-gui-semantic-snapshot",
    "request-gui-semantic-action",
    "response-gui-semantic-snapshot",
  ]) {
    assert.ok(codec.WIRE_LAYOUTS[name], `missing layout ${name}`);
  }
});

test("semantic queries encode bounded panels and fence zero entities", () => {
  assert.deepEqual(
    codec.encodeGuiSemanticSnapshotQuery({ entity: 42n }),
    queryBytes(),
  );
  assert.deepEqual(
    codec.encodeGuiSemanticSnapshotQuery({
      entity: 42n,
      maxDepth: 10,
      limit: 50,
    }),
    queryBytes(42n, 10, 50),
  );
  assert.throws(
    () => codec.encodeGuiSemanticSnapshotQuery({ entity: 0n }),
    /zero GUI semantic entity/,
  );
  assert.throws(
    () => codec.encodeGuiSemanticSnapshotQuery({ entity: 42n, maxDepth: 33 }),
    /GUI semantic snapshot bounds/,
  );
});

test("semantic actions encode every kind and reject out-of-domain values", () => {
  const head = {
    entity: 42n,
    rootIncarnation: 3n,
    node: 9,
    lifetime: 1,
    expectedRevision: 2,
  };
  assert.deepEqual(
    codec.encodeGuiSemanticAction({ ...head, action: { kind: "press" } }),
    actionBytes(0),
  );
  assert.deepEqual(
    codec.encodeGuiSemanticAction({ ...head, action: { kind: "toggle" } }),
    actionBytes(1),
  );
  assert.deepEqual(
    codec.encodeGuiSemanticAction({
      ...head,
      action: { kind: "setScalar", value: 1.5 },
    }),
    actionBytes(2, f32(1.5)),
  );
  assert.deepEqual(
    codec.encodeGuiSemanticAction({
      ...head,
      action: { kind: "setText", value: "héllo" },
    }),
    actionBytes(3, text("héllo")),
  );
  assert.deepEqual(
    codec.encodeGuiSemanticAction({ ...head, action: { kind: "focus" } }),
    actionBytes(4),
  );
  const legalText = "é".repeat(32_768);
  assert.deepEqual(
    codec.encodeGuiSemanticAction({
      ...head,
      action: { kind: "setText", value: legalText },
    }),
    actionBytes(3, text(legalText)),
  );
  assert.throws(
    () =>
      codec.encodeGuiSemanticAction({
        ...head,
        action: { kind: "setText", value: `${legalText}a` },
      }),
    /integer out of range/,
  );
  assert.throws(
    () =>
      codec.encodeGuiSemanticAction({
        ...head,
        node: 0,
        action: { kind: "press" },
      }),
    /zero GUI semantic node/,
  );
  assert.throws(
    () =>
      codec.encodeGuiSemanticAction({
        ...head,
        action: { kind: "hold" },
      }),
    /GUI semantic action kind/,
  );
  assert.throws(
    () =>
      codec.encodeGuiSemanticAction({
        ...head,
        action: { kind: "press", extra: true },
      }),
    /unknown system input field/,
  );
});

test("semantic requests conform to the manifest and require correlation", () => {
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 3n,
      body: { kind: "guiSemanticSnapshot", query: { entity: 42n } },
    }),
    layout("request-gui-semantic-snapshot", {
      session: 7n,
      request_id: 3n,
      tag: tag("REQUEST_GUI_SEMANTIC_SNAPSHOT"),
      query: queryBytes(),
    }).bytes,
  );
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 3n,
      body: {
        kind: "guiSemanticAction",
        action: {
          entity: 42n,
          rootIncarnation: 3n,
          node: 9,
          lifetime: 1,
          expectedRevision: 2,
          action: { kind: "toggle" },
        },
      },
    }),
    layout("request-gui-semantic-action", {
      session: 7n,
      request_id: 3n,
      tag: tag("REQUEST_GUI_SEMANTIC_ACTION"),
      action: actionBytes(1),
    }).bytes,
  );
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 0n,
        body: { kind: "guiSemanticSnapshot", query: { entity: 42n } },
      }),
    /reserved request identity/,
  );
});

function snapshotResponse(requestId = 3n, tick = 11n) {
  return layout("response-gui-semantic-snapshot", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_GUI_SEMANTIC_SNAPSHOT"),
    snapshot: snapshotBytes(),
  }).bytes;
}

test("semantic snapshots decode to bounded trees with focus", () => {
  assert.deepEqual(codec.decodeResponse(snapshotResponse(), 7n), {
    session: 7n,
    requestId: 3n,
    tick: 11n,
    body: { kind: "guiSemanticSnapshot", snapshot: snapshotTree },
  });
  assert.throws(
    () => codec.decodeResponse(snapshotResponse(), 8n),
    /session mismatch/,
  );
  const trailing = concatenate([snapshotResponse(), u8(0)]);
  assert.throws(() => codec.decodeResponse(trailing, 7n), /trailing bytes/);
  const badRole = snapshotBytes().slice();
  badRole[1 + 8 + 8 + 8 + 4 + 4 + 4 + 4] = 9;
  const bad = layout("response-gui-semantic-snapshot", {
    session: 7n,
    request_id: 3n,
    tick: 11n,
    tag: tag("RESPONSE_GUI_SEMANTIC_SNAPSHOT"),
    snapshot: badRole,
  }).bytes;
  assert.throws(() => codec.decodeResponse(bad, 7n), /GUI semantic role/);
});

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

function requestIdOf(sent) {
  return new DataView(sent.buffer, sent.byteOffset + 8, 8).getBigUint64(
    0,
    true,
  );
}

function snapshotReply(sent, tick = 11n) {
  return layout("response-gui-semantic-snapshot", {
    session: 7n,
    request_id: requestIdOf(sent),
    tick,
    tag: tag("RESPONSE_GUI_SEMANTIC_SNAPSHOT"),
    snapshot: snapshotBytes(),
  }).bytes;
}

function admissionReply(sent, tick = 11n) {
  return layout("response-gui-input", {
    session: 7n,
    request_id: requestIdOf(sent),
    tick,
    tag: tag("RESPONSE_GUI_INPUT"),
    routing: layout("gui-input-routing", {
      tick,
      reason: 0,
      blocker: null,
    }),
  }).bytes;
}

function errorReply(sent, message = "semantic action unknown node 9") {
  const encoded = new TextEncoder().encode(message);
  return concatenate([
    u64(7n),
    u64(requestIdOf(sent)),
    u64(11n),
    u8(255),
    u16(1),
    u32(encoded.length),
    encoded,
  ]);
}

test("semantic clients observe snapshots and dispatch actions end to end", async () => {
  const { client, sent, emit } = await connect();
  try {
    const observed = client.semanticSnapshot({ entity: 42n });
    emit(snapshotReply(sent.at(-1)));
    assert.deepEqual(await observed, snapshotTree);

    const pressed = client.semanticAction({
      entity: 42n,
      rootIncarnation: 3n,
      node: 2,
      lifetime: 1,
      expectedRevision: 0,
      action: { kind: "press" },
    });
    emit(admissionReply(sent.at(-1)));
    await pressed;

    const refused = client.semanticAction({
      entity: 42n,
      rootIncarnation: 3n,
      node: 9,
      lifetime: 1,
      expectedRevision: 0,
      action: { kind: "toggle" },
    });
    emit(errorReply(sent.at(-1)));
    await assert.rejects(refused, /Host 1: semantic action unknown node 9/);
  } finally {
    await client.close();
  }
});
