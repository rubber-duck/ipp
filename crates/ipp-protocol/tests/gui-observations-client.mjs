// GUI observation wire boundaries through a real generated client: committed
// effects with conflicts/cancellations broadcast, unhandled supplier-private.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  replyToHostCreate,
  encodeManifestLayout,
  manifestVariant,
} from "./generated-client.mjs";

const { codec } = await generateClient("gui-observations", ["surfaces", "gui"]);
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

function path(ids) {
  return concatenate([u32(ids.length), ...ids.map(u32)]);
}

function target(entity, rootIncarnation, node, lifetime) {
  if (entity === undefined) return u8(0);
  return concatenate([
    u8(1),
    u64(entity),
    u64(rootIncarnation),
    u32(node),
    u32(lifetime),
  ]);
}

const buttonEffect = () =>
  concatenate([
    u8(0),
    u64(7n),
    u64(11n),
    u64(12n),
    u64(100n),
    u64(3n),
    u32(20),
    u32(1),
    path([10, 20]),
  ]);

const controlEffect = () =>
  concatenate([
    u8(1),
    u64(7n),
    u64(11n),
    u64(12n),
    u64(100n),
    u64(3n),
    u32(30),
    u32(1),
    path([10, 30]),
    u32(2),
    u8(1),
    u8(1),
  ]);

const conflictRecord = () =>
  concatenate([
    u64(7n),
    u64(11n),
    u64(12n),
    target(100n, 3n, 30, 1),
    u8(0),
    u32(1),
    u32(2),
  ]);

const cancelRecord = () =>
  concatenate([u64(7n), u64(11n), u64(12n), target(), u8(3)]);

const observationsPayload = () =>
  concatenate([
    u8(3),
    u32(2),
    buttonEffect(),
    controlEffect(),
    u32(1),
    conflictRecord(),
    u32(1),
    cancelRecord(),
    u32(0),
  ]);

const textFocusPayload = () =>
  concatenate([
    u8(3),
    u32(0),
    u32(0),
    u32(0),
    u32(1),
    u8(1),
    u64(7n),
    u64(3n),
    u64(4n),
    u64(100n),
    u64(9n),
    u32(5),
    u32(1),
    u32(2),
    text("hé"),
    u32(0),
    u32(3),
    u8(1),
    text("x"),
    u32(0),
    u32(1),
  ]);

const pointerDownInput = () =>
  concatenate([
    u8(1),
    u8(1),
    u32(5),
    u8(1),
    u64(42n),
    f32(1.5),
    f32(2.5),
    u8(1),
    u32(1),
    u64(43n),
    f32(0.5),
    u8(1),
    f32(3.25),
  ]);

const unhandledPayload = (session = 7n) =>
  concatenate([
    u8(1),
    u32(1),
    u64(session),
    u64(11n),
    pointerDownInput(),
    u8(1),
    u64(44n),
  ]);

function observationsResponse(inner, requestId = 0n, tick = 12n) {
  return layout("response-gui-observations", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_GUI_OBSERVATIONS"),
    observations: inner,
  }).bytes;
}

function unhandledResponse(inner, requestId = 0n, tick = 11n) {
  return layout("response-gui-unhandled", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_GUI_UNHANDLED"),
    unhandled: inner,
  }).bytes;
}

test("gui observation tags, layouts and capability selection are generated", () => {
  assert.equal(codec.CAPABILITIES.gui, true);
  assert.equal(codec.WIRE.RESPONSE_GUI_OBSERVATIONS, 31);
  assert.equal(codec.WIRE.RESPONSE_GUI_UNHANDLED, 32);
  assert.ok("response-gui-observations" in codec.WIRE_LAYOUTS);
  assert.ok("response-gui-unhandled" in codec.WIRE_LAYOUTS);
  assert.equal("submitGuiInput" in codec.IppClient.prototype, true);
  assert.equal("subscribeGuiObservations" in codec.IppClient.prototype, true);
});

test("broadcast observations decode to effects, conflicts and cancellations", () => {
  assert.deepEqual(
    codec.decodeResponse(observationsResponse(observationsPayload()), 7n),
    {
      session: 7n,
      requestId: 0n,
      tick: 12n,
      body: {
        kind: "guiObservations",
        observations: {
          effects: [
            {
              kind: "buttonPressed",
              entity: 100n,
              rootIncarnation: 3n,
              node: 20,
              lifetime: 1,
              path: [10, 20],
              sourceTick: 11n,
              effectTick: 12n,
            },
            {
              kind: "controlCommitted",
              entity: 100n,
              rootIncarnation: 3n,
              node: 30,
              lifetime: 1,
              value: { kind: "bool", value: true },
              revision: 2,
              path: [10, 30],
              sourceTick: 11n,
              effectTick: 12n,
            },
          ],
          conflicts: [
            {
              session: 7n,
              sourceTick: 11n,
              effectTick: 12n,
              target: {
                entity: 100n,
                rootIncarnation: 3n,
                node: 30,
                lifetime: 1,
              },
              reason: { kind: "revisionMismatch", expected: 1, found: 2 },
            },
          ],
          cancellations: [
            {
              session: 7n,
              sourceTick: 11n,
              effectTick: 12n,
              reason: "gestureCancelled",
            },
          ],
        },
      },
    },
  );
  assert.throws(
    () =>
      codec.decodeResponse(observationsResponse(observationsPayload(), 1n), 7n),
    /reserved response identity/,
  );
  assert.throws(
    () => codec.decodeResponse(observationsResponse(observationsPayload()), 8n),
    /session mismatch/,
  );
});

test("unhandled inputs decode with their verbatim input echo", () => {
  assert.deepEqual(
    codec.decodeResponse(unhandledResponse(unhandledPayload()), 7n),
    {
      session: 7n,
      requestId: 0n,
      tick: 11n,
      body: {
        kind: "guiUnhandledInputs",
        inputs: [
          {
            session: 7n,
            tick: 11n,
            input: {
              kind: "pointerDown",
              pointer: 5,
              panel: 42n,
              position: [1.5, 2.5],
              button: "secondary",
              blockers: [{ entity: 43n, distance: 0.5 }],
              panelDistance: 3.25,
            },
            reason: { kind: "blocked", entity: 44n },
          },
        ],
      },
    },
  );
});

test("observation payloads reject empty, oversized and mistagged content", () => {
  for (const inner of [
    concatenate([u8(1), u32(0), u32(0), u32(0)]),
    concatenate([u8(3), u32(0), u32(0), u32(0), u32(0)]),
    concatenate([u8(3), u32(129), u32(0), u32(0), u32(0)]),
    concatenate([u8(3), u32(1), buttonEffect(), u32(0), u32(0), u32(0), u8(0)]),
  ]) {
    assert.throws(
      () => codec.decodeResponse(observationsResponse(inner), 7n),
      Error,
    );
  }
  assert.throws(
    () =>
      codec.decodeResponse(unhandledResponse(concatenate([u8(1), u32(0)])), 7n),
    Error,
  );
  const trailing = concatenate([
    observationsResponse(observationsPayload()),
    u8(0),
  ]);
  assert.throws(() => codec.decodeResponse(trailing, 7n), /trailing bytes/);
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

test("subscribed observations deliver batches over the live client path", async () => {
  const { client, emit } = await connect();
  try {
    const seen = [];
    const stop = client.subscribeGuiObservations(
      (batch) => void seen.push(batch),
    );
    emit(observationsResponse(observationsPayload()));
    assert.equal(seen.length, 1);
    assert.equal(seen[0].effects.length, 2);
    assert.equal(seen[0].conflicts.length, 1);
    assert.equal(seen[0].cancellations.length, 1);
    assert.deepEqual(seen[0].unhandled, []);
    // Same report tick as the observations above: the client connection
    // requires monotonic ticks.
    emit(unhandledResponse(unhandledPayload(), 0n, 12n));
    assert.equal(seen.length, 2);
    assert.equal(seen[1].unhandled.length, 1);
    assert.deepEqual(seen[1].effects, []);
    // Foreign unhandled inputs never reach this session's subscribers.
    emit(unhandledResponse(unhandledPayload(8n), 0n, 12n));
    assert.equal(seen.length, 2);
    stop();
    emit(observationsResponse(observationsPayload()));
    assert.equal(seen.length, 2);
  } finally {
    await client.close();
  }
});

test("focused text state replays to late subscribers and clears on session close", async () => {
  const { client, emit } = await connect();
  const early = [];
  client.subscribeGuiObservations((batch) => early.push(batch.textFocus));
  emit(observationsResponse(textFocusPayload()));
  assert.equal(early[0].text, "hé");
  assert.deepEqual(early[0].composition, {
    text: "x",
    caretStart: 0,
    caretEnd: 1,
  });

  const late = [];
  client.subscribeGuiObservations((batch) => late.push(batch.textFocus));
  assert.equal(late.length, 1);
  assert.equal(late[0].contextGeneration, 3n);
  await client.close();
  assert.equal(early.at(-1), null);
  assert.equal(late.at(-1), null);
});
