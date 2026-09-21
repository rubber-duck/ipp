// Focused codec/client tests with controlled delivery, not host integration coverage.
// Run: node --test crates/ipp-protocol/tests/frame-client.mjs
// Uses an executed native contract, the production generator and client, and pinned tsc.
import assert from "node:assert/strict";
import test from "node:test";
import { generateClient, replyToHostCreate } from "./generated-client.mjs";

const { codec, logging } = await generateClient("frames");
const { IppClient } = codec;

test("diagnostic filtering is lazy and console failures cannot poison a session", async () => {
  let detailsEvaluated = false;
  const disabled = new logging.DiagnosticLogger("test", "off");
  disabled.log("error", "disabled", () => {
    detailsEvaluated = true;
    return { unexpected: true };
  });
  assert.equal(detailsEvaluated, false);

  const originalInfo = console.info;
  console.info = () => {
    throw new Error("intentional console sink failure");
  };
  try {
    const { client } = await connect({ logLevel: "info" });
    assert.equal(client.session, 7n);
    await client.close();
  } finally {
    console.info = originalInfo;
  }
});

function packet({
  session = 7n,
  requestId = 0n,
  tick = 1n,
  time = 0.25,
  tag = codec.WIRE.RESPONSE_FRAME,
} = {}) {
  const bytes = new Uint8Array(tag === codec.WIRE.RESPONSE_INSPECT ? 37 : 33);
  const view = new DataView(bytes.buffer);
  view.setBigUint64(0, session, true);
  view.setBigUint64(8, requestId, true);
  view.setBigUint64(16, tick, true);
  view.setUint8(24, tag);
  view.setFloat64(25, time, true);
  return bytes;
}

async function connect(options = {}) {
  let events;
  const sent = [];
  let closes = 0;
  const client = await IppClient.connectTransport(
    {
      start(value) {
        events = value;
        events.ready();
      },
      send(bytes) {
        if (replyToHostCreate(bytes, events)) return;
        sent.push(bytes.slice());
        if (sent.length === 1) {
          const reply = new Uint8Array(24);
          reply.set(bytes);
          new DataView(reply.buffer).setBigUint64(16, 7n, true);
          events.message(reply);
        }
      },
      async close() {
        closes++;
      },
    },
    options,
  );
  assert.ok(client instanceof IppClient);
  return {
    client,
    sent,
    emit: (bytes) => events.message(bytes),
    get closes() {
      return closes;
    },
  };
}

function inspection(requestId, tick) {
  const bytes = new Uint8Array(57);
  bytes.set(packet({ requestId, tick, tag: codec.WIRE.RESPONSE_INSPECT }));
  return bytes;
}

function batchResponse(requestId, tick, batchId) {
  const bytes = new Uint8Array(50);
  bytes.set(
    packet({ requestId, tick, tag: codec.WIRE.RESPONSE_BATCH }).slice(0, 25),
  );
  const view = new DataView(bytes.buffer);
  view.setBigUint64(25, batchId, true);
  view.setBigUint64(33, tick, true);
  view.setUint8(41, codec.WIRE.OUTCOME_SUCCESS);
  return bytes;
}

test("generated codec reserves events and rejects retired tags, invalid times, and malformed frames", () => {
  assert.equal(codec.PROTOCOL_VERSION, 2);
  assert.equal(typeof IppClient.connectWebSocket, "function");
  assert.equal("connect" in IppClient, false);
  assert.equal(codec.WIRE.RESPONSE_FRAME, 4);
  assert.equal(codec.WIRE.VALUE_BYTES, 6);
  assert.deepEqual(codec.WIRE_TAG_LAYOUTS.RESPONSE_FRAME, {
    space: 5,
    capability: "base",
    layout: "response-frame",
  });
  assert.equal("REQUEST_STEP" in codec.WIRE, false);
  assert.equal("RESPONSE_STEP" in codec.WIRE, false);
  assert.match(
    codec.ENCODING,
    /nonzero-rpc-and-query;zero-command-and-unsolicited-event;commands-no-reply/,
  );
  assert.deepEqual(codec.decodeResponse(packet(), 7n).body, {
    kind: "frame",
    time: 0.25,
  });
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 0n,
        body: { kind: "inspect" },
      }),
    /reserved request identity/,
  );
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 1n,
        body: { kind: "step", dt: 1 },
      }),
    /unsupported request/,
  );
  for (const bytes of [
    packet({ requestId: 1n }),
    inspection(0n, 1n),
    packet({ tag: 2, requestId: 1n }),
    packet({ session: 8n }),
    packet({ session: 0n }),
    new Uint8Array([...packet(), 0]),
  ])
    assert.throws(() => codec.decodeResponse(bytes, 7n));
  for (let n = 0; n < 33; n++)
    assert.throws(() => codec.decodeResponse(packet().slice(0, n), 7n));
  for (const time of [NaN, Infinity, -Infinity, -1])
    for (const [tag, requestId] of [
      [codec.WIRE.RESPONSE_FRAME, 0n],
      [codec.WIRE.RESPONSE_INSPECT, 1n],
    ])
      assert.throws(() =>
        codec.decodeResponse(packet({ tag, requestId, time }), 7n),
      );
});

test("minimal generated codec retains every generic field value encoding", () => {
  const payload = new Uint8Array([7, 8, 9]);
  const bytes = codec.encodeRequest({
    session: 7n,
    requestId: 1n,
    body: {
      kind: "batch",
      batch: {
        id: 2n,
        operations: [
          {
            kind: "setField",
            entity: codec.Entity.alias(4),
            component: 99,
            field: { offset: 12, value: { kind: "bytes", value: payload } },
          },
        ],
      },
    },
  });
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  assert.equal(bytes[41], codec.WIRE.VALUE_BYTES);
  assert.equal(view.getUint32(42, true), payload.length);
  assert.deepEqual([...bytes.slice(46)], [...payload]);
});

test("generated client rejects a bootstrap mismatch and closes its transport", async () => {
  let events;
  let closes = 0;
  await assert.rejects(
    IppClient.connectTransport(
      {
        start(value) {
          events = value;
          events.ready();
        },
        send(bytes) {
          const reply = new Uint8Array(24);
          reply.set(bytes);
          reply[8] ^= 1;
          new DataView(reply.buffer).setBigUint64(16, 7n, true);
          events.message(reply);
        },
        async close() {
          closes++;
        },
      },
      { timeoutMs: 1_000 },
    ),
    /bootstrap compatibility mismatch/,
  );
  assert.ok(closes > 0);
});

test("frame waits send no data, explicit thresholds reuse latest, default waits for new progress", async () => {
  const { client, sent, emit } = await connect();
  try {
    assert.equal("step" in client, false);
    const first = client.waitForFrame();
    const future = client.waitForFrame(2n);
    assert.equal(sent.length, 1);
    emit(packet());
    assert.deepEqual(await first, { tick: 1n, time: 0.25 });
    const cached = await client.waitForFrame(0n);
    cached.tick = 100n;
    assert.equal((await client.waitForFrame(0n)).tick, 1n);
    let settled = false;
    const next = client.waitForFrame().then((frame) => {
      settled = true;
      return frame;
    });
    await Promise.resolve();
    assert.equal(settled, false);
    emit(packet({ tick: 2n }));
    assert.equal((await next).tick, 2n);
    emit(packet({ tick: 3n }));
    assert.equal((await future).tick, 3n);
    assert.equal(sent.length, 1);
  } finally {
    await client.close();
  }
});

test("unsolicited frames interleave correlated async batch and inspect responses", async () => {
  const { client, sent, emit } = await connect();
  try {
    const batch = client.batch([], 12n);
    const inspect = client.inspectPage();
    const frame = client.waitForFrame();
    emit(packet());
    emit(batchResponse(1n, 2n, 12n));
    emit(inspection(2n, 2n));
    assert.equal((await batch).tick, 2n);
    assert.equal((await inspect).tick, 2n);
    assert.equal((await frame).tick, 1n);
    let settled = false;
    const next = client.waitForFrame().then((value) => {
      settled = true;
      return value;
    });
    emit(packet({ tick: 2n }));
    await Promise.resolve();
    assert.equal(
      settled,
      false,
      "default threshold includes the latest observed RPC tick",
    );
    emit(packet({ tick: 3n }));
    assert.equal((await next).tick, 3n);
    assert.equal(sent.length, 3);
  } finally {
    await client.close();
  }
});

test("frame waiter bounds, timeout diagnostics and recovery", async (t) => {
  const { client, sent, emit } = await connect({ timeoutMs: 1000 });
  t.mock.timers.enable({ apis: ["setTimeout"] });
  try {
    for (const invalid of [-1n, 0x1_0000000000000000n, 1])
      await assert.rejects(client.waitForFrame(invalid), /afterTick/);
    const waiters = Array.from({ length: 64 }, () =>
      assert.rejects(client.waitForFrame(), /Waiting for a frame timed out/),
    );
    await assert.rejects(client.waitForFrame(), /Frame waiter limit/);
    t.mock.timers.tick(1000);
    await Promise.all(waiters);
    const recovered = client.waitForFrame();
    emit(packet());
    assert.equal((await recovered).tick, 1n);
    assert.equal(sent.length, 1);
  } finally {
    await client.close();
  }
});

test("close and RPC timeout reject both request and frame waiters", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const timeout of [false, true]) {
    const { client } = await connect({ timeoutMs: 1000 });
    const message = timeout ? /Request timed out/ : /Client closed/;
    const rpc = assert.rejects(client.inspect(), message);
    const frame = assert.rejects(client.waitForFrame(), message);
    if (timeout) t.mock.timers.tick(1000);
    else await client.close();
    await Promise.all([rpc, frame]);
    await assert.rejects(client.waitForFrame(0n), /closed/);
    await client.close();
  }
});

test("stale sessions, backward ticks/time and malformed event IDs fail closed", async () => {
  for (const invalid of [
    packet({ session: 8n, tick: 3n }),
    packet({ tick: 1n }),
    packet({ tick: 2n }),
    packet({ tick: 3n, time: 0 }),
    packet({ requestId: 1n, tick: 3n }),
    inspection(0n, 3n),
  ]) {
    const fixture = await connect();
    fixture.emit(packet({ tick: 2n }));
    const rpc = assert.rejects(fixture.client.inspect());
    const frame = assert.rejects(fixture.client.waitForFrame());
    fixture.emit(invalid);
    await Promise.all([rpc, frame]);
    assert.ok(fixture.closes > 0);
    await fixture.client.close();
  }
});
