// Target-generated production codec coverage for exact view fences and ordered navigation.
import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  encodeManifestLayout,
  manifestVariant,
} from "./generated-client.mjs";

const client = await generateClient("composed-query");
const { codec } = client;
const output = {
  world: { id: 3n, incarnation: 9n },
  entity: 4n,
  kind: "camera",
  incarnation: 8n,
};
const binding = {
  output,
  viewport: { width: 640, height: 480, devicePixelRatio: 1.25 },
  generation: { host: 7n, serial: 2n },
};
const publication = { host: 7n, revision: 11n };
const tag = (name) => manifestVariant(client, name);

test("bound source and navigation encode complete generation, viewport and source fences", () => {
  const bytes = codec.encodeRequest({
    session: 2n,
    requestId: 3n,
    body: {
      kind: "cameraNavigate",
      request: {
        binding,
        publication,
        motion: { kind: "pan", x: 0.25, y: -0.5 },
      },
    },
  });
  assert.equal(bytes.length, 111);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  assert.equal(view.getBigUint64(66, true), 7n);
  assert.equal(view.getBigUint64(74, true), 2n);
  assert.equal(view.getUint8(82), tag("OPTION_SOME").value);
  assert.equal(view.getBigUint64(83, true), 7n);
  assert.equal(view.getBigUint64(91, true), 11n);
  assert.equal(view.getUint32(99, true), 1);
  const query = codec.encodeRequest({
    session: 2n,
    requestId: 4n,
    body: {
      kind: "query",
      query: {
        type: "CameraProjectQuery",
        view: { kind: "bound", binding, publication },
        x: 0.5,
        y: 0.5,
        plane: { point: [0, 0, 0], normal: [0, 0, 1] },
      },
    },
  });
  assert.ok(query.length > bytes.length);
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 2n,
        requestId: 3n,
        body: {
          kind: "cameraNavigate",
          request: {
            binding,
            publication,
            motion: { kind: "pan", x: 1, y: 0, width: 10, height: 10 },
          },
        },
      }),
    /unknown/,
  );
});

test("ordinary gestures and queries request current completed state without a source identity", () => {
  const bytes = codec.encodeRequest({
    session: 2n,
    requestId: 5n,
    body: {
      kind: "cameraNavigate",
      request: { binding, motion: { kind: "pan", x: 0.1, y: 0 } },
    },
  });
  assert.equal(bytes.length, 95);
  assert.equal(bytes[82], tag("OPTION_NONE").value);
  const query = codec.encodeRequest({
    session: 2n,
    requestId: 6n,
    body: {
      kind: "query",
      query: {
        type: "CameraProjectQuery",
        view: { kind: "bound", binding },
        x: 0.5,
        y: 0.5,
        plane: { point: [0, 0, 0], normal: [0, 0, 1] },
      },
    },
  });
  assert.equal(query[83], tag("OPTION_NONE").value);
});

test("navigation terminal remains a correlated response, not an active camera event", () => {
  const bytes = encodeManifestLayout(client, "response-camera-navigated", {
    session: 2n,
    request_id: 3n,
    tick: 4n,
    tag: tag("RESPONSE_CAMERA_NAVIGATED"),
  }).bytes;
  const decoded = codec.decodeResponse(bytes, 2n);
  assert.equal(decoded.session, 2n);
  assert.equal(decoded.requestId, 3n);
  assert.equal(decoded.tick, 4n);
  assert.deepEqual(decoded.body, { kind: "cameraNavigated" });
});
