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
  assert.equal(bytes.length, 115);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  assert.equal(view.getBigUint64(66, true), 7n);
  assert.equal(view.getBigUint64(74, true), 2n);
  assert.equal(view.getUint8(82), tag("OPTION_SOME").value);
  assert.equal(view.getBigUint64(83, true), 7n);
  assert.equal(view.getBigUint64(91, true), 11n);
  assert.equal(view.getUint32(99, true), 0);
  assert.equal(view.getUint32(103, true), 1);
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
  assert.equal(bytes.length, 99);
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

test("composed camera requests encode exact producer references and bound path depth", () => {
  const path = [
    { parent: output.world, anchor: 21n, incarnation: 22n, revision: 23n },
  ];
  const root = { kind: "bound", binding };
  const query = {
    type: "GeometryPickQuery",
    view: { kind: "camera", root, path },
    x: 0.5,
    y: 0.5,
    includeViewPlane: true,
  };
  const layout = (name, fields) => encodeManifestLayout(client, name, fields);
  const encodedBinding = layout("root-binding", {
    output: layout("output-reference", {
      world: layout("world-reference", output.world),
      target: layout("output-target-camera", {
        tag: tag("OUTPUT_TARGET_CAMERA"),
        entity: output.entity,
        incarnation: output.incarnation,
      }),
    }),
    width: 640,
    height: 480,
    device_pixel_ratio: 1.25,
    generation: layout("presentation-identity", binding.generation),
  });
  const expected = layout("request-geometry-pick", {
    session: 2n,
    request_id: 7n,
    tag: tag("REQUEST_GEOMETRY_PICK"),
    view: layout("view-camera", {
      tag: tag("VIEW_CAMERA"),
      root: layout("view-bound", {
        tag: tag("VIEW_BOUND"),
        binding: encodedBinding,
        publication: null,
      }),
      path: path.map((reference) =>
        layout("camera-attachment-reference", {
          ...reference,
          parent: layout("world-reference", reference.parent),
        }),
      ),
    }),
    x: 0.5,
    y: 0.5,
    include_view_plane: true,
  }).bytes;
  assert.deepEqual(
    codec.encodeRequest({
      session: 2n,
      requestId: 7n,
      body: { kind: "query", query },
    }),
    expected,
  );
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 2n,
        requestId: 7n,
        body: {
          kind: "query",
          query: { ...query, view: { ...query.view, root: query.view } },
        },
      }),
    /nonrecursive/,
  );
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 2n,
        requestId: 7n,
        body: {
          kind: "cameraNavigate",
          request: {
            binding,
            path: Array(65).fill(path[0]),
            motion: { kind: "zoom", amount: 1 },
          },
        },
      }),
    /depth/,
  );
});
