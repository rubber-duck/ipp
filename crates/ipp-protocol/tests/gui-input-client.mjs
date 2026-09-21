// GUI input codec boundaries through a real generated client; real hosts run the shared GUI scenarios.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  replyToHostCreate,
  encodeManifestLayout,
  manifestVariant,
} from "./generated-client.mjs";

const { codec } = await generateClient("gui-input", ["surfaces", "gui"]);
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

function u16(value) {
  const bytes = new Uint8Array(2);
  new DataView(bytes.buffer).setUint16(0, value, true);
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

const payloads = {
  pointerDown: () =>
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
    ]),
  pointerMove: () =>
    concatenate([
      u8(1),
      u8(3),
      u32(9),
      u8(0),
      f32(0.5),
      f32(0.75),
      u32(0),
      u8(0),
    ]),
  scroll: () =>
    concatenate([
      u8(1),
      u8(5),
      u8(0),
      f32(1.5),
      f32(2.5),
      f32(0),
      f32(-4),
      u32(0),
      u8(0),
    ]),
  key: () => concatenate([u8(1), u8(6), u8(10), u8(0)]),
  commitComposition: () => concatenate([u8(1), u8(12)]),
};

test("gui input tags, layouts and capability selection are generated", () => {
  assert.equal(codec.CAPABILITIES.gui, true);
  assert.equal(codec.WIRE.REQUEST_GUI_INPUT, 30);
  assert.equal(codec.WIRE.RESPONSE_GUI_INPUT, 30);
  assert.ok("request-gui-input" in codec.WIRE_LAYOUTS);
  assert.ok("response-gui-input" in codec.WIRE_LAYOUTS);
  assert.equal("submitGuiInput" in codec.IppClient.prototype, true);
  assert.equal(typeof codec.encodeGuiInput, "function");
});

test("every gui input action encodes its exact wire payload", () => {
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "pointerDown",
      pointer: 5,
      panel: 42n,
      position: [1.5, 2.5],
      button: "secondary",
      blockers: [{ entity: 43n, distance: 0.5 }],
      panelDistance: 3.25,
    }),
    payloads.pointerDown(),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "pointerCancel", pointer: 9 }),
    concatenate([u8(1), u8(4), u32(9)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "pointerMove",
      pointer: 9,
      position: [0.5, 0.75],
    }),
    payloads.pointerMove(),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "scroll",
      position: [1.5, 2.5],
      delta: [0, -4],
    }),
    payloads.scroll(),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "key", key: "home", pressed: false }),
    payloads.key(),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "text", text: "héllo" }),
    concatenate([u8(1), u8(7), text("héllo")]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "focus",
      handle: {
        session: 7n,
        entity: 42n,
        rootIncarnation: 3n,
        nodeId: 9,
        nodeLifetime: 1,
      },
    }),
    concatenate([u8(1), u8(8), u64(7n), u64(42n), u64(3n), u32(9), u32(1)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "blur" }),
    concatenate([u8(1), u8(9)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "setTextSelection", start: 2, end: 7 }),
    concatenate([u8(1), u8(10), u32(2), u32(7)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "composition",
      text: "世界",
      caretStart: 6,
      caretEnd: 6,
    }),
    concatenate([u8(1), u8(11), text("世界"), u32(6), u32(6)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "commitComposition" }),
    payloads.commitComposition(),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "cancelComposition" }),
    concatenate([u8(1), u8(13)]),
  );
});

test("gui input encoding rejects out-of-domain values before sending", () => {
  const legalText = "a".repeat(65_536);
  const encoded = codec.encodeGuiInput({ kind: "text", text: legalText });
  assert.equal(encoded.byteLength, 65_542);
  assert.equal(new TextDecoder().decode(encoded.subarray(6)), legalText);
  assert.throws(
    () => codec.encodeGuiInput({ kind: "text", text: `${legalText}a` }),
    /string limit|integer out of range/,
  );
  assert.throws(
    () =>
      codec.encodeGuiInput({
        kind: "pointerDown",
        pointer: 5,
        position: [1.5, NaN],
        button: "primary",
      }),
    /nonfinite f32/,
  );
  assert.throws(
    () => codec.encodeGuiInput({ kind: "key", key: "f13", pressed: true }),
    /GUI input key/,
  );
  assert.throws(
    () =>
      codec.encodeGuiInput({
        kind: "pointerDown",
        pointer: 5,
        position: [0, 0],
        button: "primary",
        blockers: [{ entity: 0n, distance: 1 }],
      }),
    /GUI input blocker entity/,
  );
  assert.throws(
    () => codec.encodeGuiInput({ kind: "blur", extra: true }),
    /unknown system input field/,
  );
});

test("gui input requests conform to the manifest and require correlation", () => {
  const input = {
    kind: "pointerDown",
    pointer: 5,
    panel: 42n,
    position: [1.5, 2.5],
    button: "secondary",
    blockers: [{ entity: 43n, distance: 0.5 }],
    panelDistance: 3.25,
  };
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 3n,
      body: { kind: "guiInput", input },
    }),
    layout("request-gui-input", {
      session: 7n,
      request_id: 3n,
      tag: tag("REQUEST_GUI_INPUT"),
      input: payloads.pointerDown(),
    }).bytes,
  );
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 0n,
        body: { kind: "guiInput", input },
      }),
    /reserved request identity/,
  );
});

function guiInputResponse(
  requestId = 3n,
  tick = 11n,
  routing = { tick, reason: 0, blocker: null },
) {
  return layout("response-gui-input", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_GUI_INPUT"),
    routing: layout("gui-input-routing", routing),
  }).bytes;
}

test("GUI inspection envelopes decode one legal maximum text value", () => {
  const legalText = "a".repeat(65_536);
  const payload = concatenate([
    u8(1),
    u64(42n),
    u64(3n),
    u32(1),
    u32(1),
    u32(0),
    u32(1),
    u32(0),
    u32(0),
    u8(2),
    text(legalText),
    u8(0),
    u16(1 << 13),
    f32(1),
    f32(1),
    f32(1),
    f32(1),
    f32(1),
    f32(0.1),
    u8(1),
  ]);
  assert.ok(payload.byteLength > 65_536);
  const response = layout("response-gui-inspect", {
    session: 7n,
    request_id: 3n,
    tick: 11n,
    tag: tag("RESPONSE_GUI_INSPECT"),
    payload,
  }).bytes;
  const decoded = codec.decodeResponse(response, 7n);
  assert.equal(decoded.body.kind, "guiInspect");
  assert.equal(decoded.body.response.nodes[0].content.text, legalText);
});

test("gui input responses decode to their correlated acknowledgement", () => {
  assert.deepEqual(codec.decodeResponse(guiInputResponse(), 7n), {
    session: 7n,
    requestId: 3n,
    tick: 11n,
    body: { kind: "guiInput", outcome: { tick: 11n } },
  });
  assert.throws(
    () => codec.decodeResponse(guiInputResponse(), 8n),
    /session mismatch/,
  );
  const trailing = concatenate([guiInputResponse(), u8(0)]);
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

function requestIdOf(sent) {
  return new DataView(sent.buffer, sent.byteOffset + 8, 8).getBigUint64(
    0,
    true,
  );
}

function replyFor(sent, tick = 11n) {
  return guiInputResponse(requestIdOf(sent), tick);
}

function errorReply(sent, message = "rejected") {
  const encoded = new TextEncoder().encode(message);
  return concatenate([
    u64(7n),
    u64(requestIdOf(sent)),
    u64(13n),
    u8(255),
    u16(1),
    u32(encoded.length),
    encoded,
  ]);
}

test("submitGuiInput resolves authoritative routing and rejects host errors", async () => {
  const { client, sent, emit } = await connect();
  try {
    const before = sent.length;
    const first = client.submitGuiInput({ kind: "blur" });
    const second = client.submitGuiInput({
      kind: "pointerDown",
      pointer: 1,
      position: [2, 1.5],
      button: "primary",
    });
    assert.equal(sent.length, before + 2);
    emit(replyFor(sent[before]));
    emit(
      guiInputResponse(requestIdOf(sent[before + 1]), 12n, {
        tick: 12n,
        reason: 1,
        blocker: null,
      }),
    );
    assert.deepEqual(await first, { tick: 11n });
    assert.deepEqual(await second, {
      tick: 12n,
      unhandled: { kind: "noPanelHit" },
    });

    const rejected = client.submitGuiInput({ kind: "blur" });
    emit(errorReply(sent.at(-1)));
    await assert.rejects(rejected, /Host 1: rejected/);
  } finally {
    await client.close();
  }
});
