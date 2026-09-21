// Focused codec/admission evidence supplements real native/browser subscription scenarios.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  encodeManifestLayout,
  manifestVariant,
  replyToHostCreate,
} from "./generated-client.mjs";

const { codec } = await generateClient("lifecycle-minimal");
const spatial = await generateClient("lifecycle-assets", []);
const layout = (name, values) => encodeManifestLayout(codec, name, values);
const tag = (name) => manifestVariant(codec, name);

function response(name, requestId, fields = {}) {
  return layout(`response-lifecycle-${name}`, {
    session: 7n,
    request_id: requestId,
    tick: 3n,
    tag: tag(`RESPONSE_LIFECYCLE_${name.toUpperCase()}`),
    ...fields,
  }).bytes;
}

function publication(subscription = 1n) {
  return layout("lifecycle-publication", {
    subscription,
    sequence: 2n,
    tick: 2n,
    observation: layout("lifecycle-component", {
      tag: tag("LIFECYCLE_COMPONENT_REPLACED"),
      entity: 9n,
      component: codec.Scalar.id,
      previous_incarnation: 4n,
      incarnation: 5n,
    }),
  });
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

function lastRequestId(sent) {
  const bytes = sent.at(-1);
  return new DataView(
    bytes.buffer,
    bytes.byteOffset,
    bytes.byteLength,
  ).getBigUint64(8, true);
}

test("baseline contracts expose entity, component and asset subscription domains", () => {
  const body = { kind: "subscribeLifecycle", subscription: 1n, filter: {} };
  const bytes = codec.encodeRequest({ session: 7n, requestId: 10n, body });
  assert.deepEqual(
    bytes,
    layout("request-lifecycle-subscribe", {
      session: 7n,
      request_id: 10n,
      tag: tag("REQUEST_LIFECYCLE_SUBSCRIBE"),
      subscription: 1n,
      domains: 7,
      asset: 0n,
      entity: 0n,
      component: 0,
    }).bytes,
  );
  assert.equal("LIFECYCLE_ASSET_STATUS_CHANGED" in codec.WIRE, true);
  assert.equal("LIFECYCLE_ASSET_REMOVED" in spatial.codec.WIRE, true);
  assert.doesNotThrow(() =>
    codec.encodeRequest({
      session: 7n,
      requestId: 10n,
      body: { ...body, filter: { assets: true } },
    }),
  );
  assert.throws(() =>
    codec.encodeRequest({
      session: 7n,
      requestId: 10n,
      body: {
        ...body,
        filter: { entities: false, components: false, assets: false },
      },
    }),
  );
  assert.throws(() =>
    codec.encodeRequest({ session: 7n, requestId: 0n, body }),
  );
  assert.throws(() =>
    codec.encodeRequest({
      session: 7n,
      requestId: 10n,
      body: { ...body, subscription: 0n },
    }),
  );
});

test("lifecycle decoder preserves effect tick and incarnations and rejects invalid envelopes", () => {
  const bytes = response("events", 0n, { events: [publication()] });
  const decoded = codec.decodeResponse(bytes, 7n);
  assert.equal(decoded.body.kind, "lifecycleEvents");
  assert.deepEqual(decoded.body.events[0], {
    subscription: 1n,
    sequence: 2n,
    tick: 2n,
    observation: {
      kind: "component",
      entity: 9n,
      component: codec.Scalar.id,
      change: "replaced",
      previousIncarnation: 4n,
      incarnation: 5n,
    },
  });
  assert.throws(() => codec.decodeResponse(bytes, 8n));
  assert.throws(() =>
    codec.decodeResponse(
      response("events", 1n, { events: [publication()] }),
      7n,
    ),
  );
  assert.throws(() =>
    codec.decodeResponse(response("events", 0n, { events: [] }), 7n),
  );
  assert.throws(() =>
    codec.decodeResponse(
      response("events", 0n, { events: [publication(0n)] }),
      7n,
    ),
  );
  assert.throws(() =>
    codec.decodeResponse(response("overflow", 0n, { dropped: 0n }), 7n),
  );
});

test("subscription acknowledgements, release and overflow isolate local observers", async () => {
  const { client, sent, emit } = await connect();
  const events = [];
  try {
    const pending = client.subscribeLifecycle({}, (event) =>
      events.push(event),
    );
    emit(response("subscription", lastRequestId(sent)));
    const subscription = await pending;
    emit(response("events", 0n, { events: [publication(subscription.id)] }));
    assert.equal(events.length, 1);
    assert.equal(events[0].tick, 2n);
    assert.equal(events[0].kind, "change");
    emit(response("overflow", 0n, { dropped: 129n }));
    assert.equal(events[1].kind, "overflow");
    assert.equal(events[1].dropped, 129n);
    emit(response("events", 0n, { events: [publication(subscription.id)] }));
    assert.equal(events.length, 2);
    await subscription.unsubscribe();
    const again = client.subscribeLifecycle({}, (event) => events.push(event));
    emit(response("subscription", lastRequestId(sent)));
    const next = await again;
    assert.notEqual(next.id, subscription.id);
    const release = next.unsubscribe();
    emit(response("subscription", lastRequestId(sent)));
    await release;
    emit(response("events", 0n, { events: [publication(next.id)] }));
    assert.equal(events.length, 2);
  } finally {
    await client.close();
  }
});
