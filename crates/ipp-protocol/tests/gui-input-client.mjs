// GUI input and edit codec boundaries through a real generated client; real hosts run the shared GUI scenarios.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  replyToHostCreate,
  encodeManifestLayout,
  manifestVariant,
} from "./generated-client.mjs";

const client = await generateClient("gui-input", ["surfaces", "gui"]);
const { codec, manifest } = client;
const layout = (name, values) => encodeManifestLayout(client, name, values);
const tag = (name) => manifestVariant(client, name);

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
  commitComposition: () => concatenate([u8(1), u8(12), u8(0)]),
};

const fence = {
  contextGeneration: 4n,
  focusGeneration: 5n,
  entity: 42n,
  rootIncarnation: 3n,
  node: 9,
  revision: 6,
};

function fenceBytes() {
  return concatenate([
    u8(1),
    u64(4n),
    u64(5n),
    u64(42n),
    u64(3n),
    u32(9),
    u32(6),
  ]);
}

test("gui input tags, layouts and capability selection are generated", () => {
  assert.equal(codec.CAPABILITIES.gui, true);
  assert.equal(codec.WIRE.REQUEST_GUI_INPUT, 30);
  assert.equal(codec.WIRE.RESPONSE_GUI_INPUT, 30);
  assert.ok("request-gui-input" in manifest.WIRE_LAYOUTS);
  assert.ok("response-gui-input" in manifest.WIRE_LAYOUTS);
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
  // Shift+Tab traversal extends the key enumeration at the end.
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "key", key: "backTab", pressed: true }),
    concatenate([u8(1), u8(6), u8(12), u8(1)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "text", text: "héllo" }),
    concatenate([u8(1), u8(7), text("héllo"), u8(0)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "text", text: "héllo", fence }),
    concatenate([u8(1), u8(7), text("héllo"), fenceBytes()]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "focus",
      handle: {
        session: 7n,
        entity: 42n,
        rootIncarnation: 3n,
        nodeId: 9,
      },
    }),
    concatenate([u8(1), u8(8), u64(7n), u64(42n), u64(3n), u32(9)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "blur" }),
    concatenate([u8(1), u8(9)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "setTextSelection", start: 2, end: 7 }),
    concatenate([u8(1), u8(10), u32(2), u32(7), u8(0)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "setTextSelection",
      start: 2,
      end: 7,
      fence,
    }),
    concatenate([u8(1), u8(10), u32(2), u32(7), fenceBytes()]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({
      kind: "composition",
      text: "世界",
      caretStart: 6,
      caretEnd: 6,
    }),
    concatenate([u8(1), u8(11), text("世界"), u32(6), u32(6), u8(0)]),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "commitComposition" }),
    payloads.commitComposition(),
  );
  assert.deepEqual(
    codec.encodeGuiInput({ kind: "cancelComposition", fence }),
    concatenate([u8(1), u8(13), fenceBytes()]),
  );
  // A fence names a live target.
  assert.throws(
    () =>
      codec.encodeGuiInput({
        kind: "text",
        text: "x",
        fence: { ...fence, node: 0 },
      }),
    /GUI text fence node/,
  );
});

test("gui input encoding rejects out-of-domain values before sending", () => {
  const legalText = "a".repeat(65_536);
  const encoded = codec.encodeGuiInput({ kind: "text", text: legalText });
  assert.equal(encoded.byteLength, 65_543);
  assert.equal(
    new TextDecoder().decode(encoded.subarray(6, 65_542)),
    legalText,
  );
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
    u8(2),
    u64(42n),
    u64(3n),
    u32(1),
    // Node: id, no parent, revision, no children, text data.
    u32(1),
    u32(0),
    u32(0),
    u32(0),
    u8(2),
    text(legalText),
    // Empty node_data row (two mask bytes), no control value.
    u8(0),
    u8(0),
    u8(0),
    // node_style row: enabled, color, opacity, font_size, position, scale,
    // focus_scope.
    u8(0b0000_0001),
    u8(0b1011_0100),
    u8(0b0001_0001),
    u32(1),
    f32(1),
    f32(1),
    f32(1),
    f32(1),
    f32(1),
    f32(0.1),
    f32(0),
    f32(0),
    f32(1),
    f32(1),
    u32(0),
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
  const [node] = decoded.body.response.nodes;
  assert.equal(node.data.text, legalText);
  assert.deepEqual(node.values, {});
  assert.deepEqual(node.style, {
    enabled: true,
    color: [1, 1, 1, 1],
    opacity: 1,
    fontSize: Math.fround(0.1),
    position: [0, 0],
    scale: [1, 1],
    focusScope: false,
  });
});

test("GUI edits encode data, values and style in the contract row layouts", () => {
  const handle = { session: 7n, entity: 42n, rootIncarnation: 3n, nodeId: 1 };
  assert.deepEqual(
    codec.encodeGuiEdits([
      {
        action: "insert",
        entity: 42n,
        rootIncarnation: 3n,
        id: 1,
        index: 0,
        data: { kind: "slider" },
        values: { value: 0.5, min: 0, max: 1, step: 0.25 },
        style: { width: 2, position: [0.5, -0.25] },
      },
      {
        action: "update",
        handle,
        patch: { style: { enabled: false, width: null, position: [1, 2] } },
      },
    ]),
    concatenate([
      u8(5),
      u32(2),
      // Insert: identity, slider data, then the node_data row (a two-byte
      // presence mask, value, min, max, step) and the node_style row with
      // its required defaults.
      u8(1),
      u64(42n),
      u64(3n),
      u32(1),
      u8(0),
      u32(0),
      u8(7),
      u8(0b0011_1100),
      u8(0),
      f32(0.5),
      f32(0),
      f32(1),
      f32(0.25),
      u8(0b0000_0011),
      u8(0b1011_0100),
      u8(0b0001_0001),
      u32(1),
      f32(2),
      f32(1),
      f32(1),
      f32(1),
      f32(1),
      f32(1),
      f32(0.1),
      f32(0.5),
      f32(-0.25),
      f32(1),
      f32(1),
      u32(0),
      // Update: handle, no data or values, changed and set masks over the
      // style layout (enabled, width cleared, position), set values.
      u8(2),
      u64(7n),
      u64(42n),
      u64(3n),
      u32(1),
      u8(0),
      u8(0),
      u8(0b0000_0011),
      u8(0b1000_0000),
      u8(0),
      u8(0b0000_0001),
      u8(0b1000_0000),
      u8(0),
      u32(0),
      f32(1),
      f32(2),
    ]),
  );
  assert.throws(
    () =>
      codec.encodeGuiEdits([
        { action: "update", handle, patch: { style: { opacity: null } } },
      ]),
    /cannot be cleared/,
  );
  assert.throws(
    () =>
      codec.encodeGuiEdits([
        { action: "update", handle, patch: { style: { lanes: 1 } } },
      ]),
    /unknown GUI lanes/,
  );
});

test("VirtualList edits encode the list kind, its items and scroll-to-index", () => {
  const handle = { session: 7n, entity: 42n, rootIncarnation: 3n, nodeId: 2 };
  assert.deepEqual(
    codec.encodeGuiEdits([
      {
        action: "insert",
        entity: 42n,
        rootIncarnation: 3n,
        id: 2,
        index: 0,
        data: { kind: "container", containerKind: "virtualList" },
        values: {
          itemCount: 100000,
          itemExtent: 1.5,
          overscan: 3,
          axis: 1,
          anchorIndex: 0,
          anchorOffset: 0,
        },
        style: {},
      },
      { action: "scrollToIndex", handle, index: 5000, offset: 0.25 },
    ]),
    concatenate([
      u8(5),
      u32(2),
      // Insert: container kind 7, then the node_data row with the six
      // VirtualList properties (bits 6..11) and the default style row.
      u8(1),
      u64(42n),
      u64(3n),
      u32(2),
      u8(0),
      u32(0),
      u8(1),
      u8(7),
      u8(0b1100_0000),
      u8(0b0000_1111),
      u32(100000),
      f32(1.5),
      u32(3),
      u32(1),
      u32(0),
      f32(0),
      u8(0b0000_0001),
      u8(0b1011_0100),
      u8(0b0001_0001),
      u32(1),
      f32(1),
      f32(1),
      f32(1),
      f32(1),
      f32(1),
      f32(0.1),
      f32(0),
      f32(0),
      f32(1),
      f32(1),
      u32(0),
      // Scroll-to-index: handle, item index, offset.
      u8(9),
      u64(7n),
      u64(42n),
      u64(3n),
      u32(2),
      u32(5000),
      f32(0.25),
    ]),
  );
  for (const edit of [
    { action: "scrollToIndex", handle, index: -1, offset: 0 },
    { action: "scrollToIndex", handle, index: 1, offset: -0.5 },
    { action: "scrollToIndex", handle, index: 1, offset: Number.NaN },
  ])
    assert.throws(() => codec.encodeGuiEdits([edit]), Error);
});

test("GUI theme and part edits encode part identities and part patches", () => {
  const handle = { session: 7n, entity: 42n, rootIncarnation: 3n, nodeId: 1 };
  assert.deepEqual(
    codec.encodeGuiEdits([
      {
        action: "updateTheme",
        entity: 42n,
        rootIncarnation: 3n,
        theme: 11,
        part: { part: "background", state: "hovered" },
        patch: { color: [0.25, 0.5, 0.75, 1], opacity: null },
      },
      { action: "removeTheme", entity: 42n, rootIncarnation: 3n, theme: 11 },
      {
        action: "updatePart",
        handle,
        part: "icon",
        patch: { borderWidth: 0.5 },
      },
    ]),
    concatenate([
      u8(5),
      u32(3),
      // Theme: identity, background (0) hovered (1 + 1 * 3), then changed
      // and set masks over the 23 part properties and the set values.
      u8(6),
      u64(42n),
      u64(3n),
      u32(11),
      u8(4),
      u8(0b0000_0011),
      u8(0),
      u8(0),
      u8(0b0000_0001),
      u8(0),
      u8(0),
      f32(0.25),
      f32(0.5),
      f32(0.75),
      f32(1),
      u8(7),
      u64(42n),
      u64(3n),
      u32(11),
      // Part: handle, icon (3), border width (bit 6).
      u8(8),
      u64(7n),
      u64(42n),
      u64(3n),
      u32(1),
      u8(3),
      u8(0b0100_0000),
      u8(0),
      u8(0),
      u8(0b0100_0000),
      u8(0),
      u8(0),
      f32(0.5),
    ]),
  );
  assert.throws(
    () =>
      codec.encodeGuiEdits([
        {
          action: "updateTheme",
          entity: 42n,
          rootIncarnation: 3n,
          theme: 1,
          part: { part: "icon", variant: "checked" },
          patch: {},
        },
      ]),
    /requires a state/,
  );
  assert.throws(
    () =>
      codec.encodeGuiEdits([
        { action: "updatePart", handle, part: "icon", patch: { theme: 1 } },
      ]),
    /unknown GUI theme/,
  );
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

/** Connect a client whose transport answers GUI edit paging like a Host:
 * `respond(kind, edits)` returns the applied prefix and optional error of
 * each GUI request; batch control requests always succeed. */
async function connectPagingHost(respond) {
  let handler;
  let attached = false;
  const requests = [];
  const client = await codec.IppClient.connectTransport(
    {
      start(events) {
        handler = events;
        events.ready();
      },
      send(bytes) {
        if (replyToHostCreate(bytes, handler)) return;
        if (!attached) {
          attached = true;
          const reply = new Uint8Array(24);
          reply.set(bytes);
          new DataView(reply.buffer).setBigUint64(16, 7n, true);
          handler.message(reply);
          return;
        }
        const request_id = requestIdOf(bytes);
        const header = { session: 7n, request_id, tick: 1n };
        let reply;
        if (bytes[16] === codec.WIRE.REQUEST_BEGIN_BATCH) {
          requests.push({ kind: "begin" });
          reply = layout("response-batch-identity", {
            ...header,
            tag: tag("RESPONSE_BATCH_STARTED"),
            batch_id: 27n,
          });
        } else if (bytes[16] === codec.WIRE.REQUEST_END_BATCH) {
          requests.push({ kind: "end" });
          reply = layout("response-batch-identity", {
            ...header,
            tag: tag("RESPONSE_BATCH_FINISHED"),
            batch_id: 27n,
          });
        } else if (bytes[16] === codec.WIRE.REQUEST_GUI) {
          const view = new DataView(bytes.buffer, bytes.byteOffset);
          const streamed = bytes[17] === codec.WIRE.OPTION_SOME;
          // Edits payload: u32 length, then version and edit count.
          const edits = view.getUint32(18 + (streamed ? 8 : 0) + 4 + 1, true);
          const kind = streamed ? "page" : "direct";
          requests.push({ kind, edits });
          const { applied, error } = respond(kind, edits);
          reply = layout("response-gui", {
            ...header,
            tag: tag("RESPONSE_GUI"),
            applied,
            error: error ?? null,
          });
        } else throw new Error(`unexpected request tag ${bytes[16]}`);
        queueMicrotask(() => handler.message(reply.bytes));
      },
      async close() {},
    },
    { logLevel: "off" },
  );
  return { client, requests };
}

test("GUI edit batches page at the exact request-byte budget", async () => {
  const handle = (nodeId) => ({
    session: 7n,
    entity: 42n,
    rootIncarnation: 3n,
    nodeId,
  });
  const text = (nodeId, value) => ({
    action: "update",
    handle: handle(nodeId),
    patch: { data: { kind: "text", text: value } },
  });
  // Eighteen 60 kB patches exceed one message; seventeen fit in a page.
  const large = (character) =>
    Array.from({ length: 18 }, (_, index) =>
      text(2, `${character.repeat(60_000)}${index}`),
    );
  // The Host rejects the first of the last page's two trailing edits.
  let failLastPage = false;
  const { client, requests } = await connectPagingHost((kind, edits) =>
    failLastPage && kind === "page" && edits === 3
      ? { applied: 1, error: "duplicate GUI node" }
      : { applied: edits },
  );
  try {
    const small = Array.from({ length: 50 }, (_, index) =>
      text(index + 1, `node ${index}`),
    );
    assert.deepEqual(await client.editGuiBatch(small), {
      ok: true,
      applied: 50,
      requests: 1,
    });
    assert.deepEqual(requests, [{ kind: "direct", edits: 50 }]);

    requests.length = 0;
    assert.deepEqual(await client.editGuiBatch(large("x")), {
      ok: true,
      applied: 18,
      requests: 4,
    });
    assert.deepEqual(requests, [
      { kind: "begin" },
      { kind: "page", edits: 17 },
      { kind: "page", edits: 1 },
      { kind: "end" },
    ]);

    // A failure on the second page reports the global applied prefix and
    // stops before finishing the logical batch.
    requests.length = 0;
    failLastPage = true;
    const failed = await client.editGuiBatch([
      ...large("y"),
      text(3, "must fail"),
      text(4, "suffix"),
    ]);
    assert.deepEqual(
      { ...failed, error: undefined },
      { ok: false, applied: 18, requests: 3, error: undefined },
    );
    assert.deepEqual(requests, [
      { kind: "begin" },
      { kind: "page", edits: 17 },
      { kind: "page", edits: 3 },
    ]);
  } finally {
    await client.close();
  }
});
