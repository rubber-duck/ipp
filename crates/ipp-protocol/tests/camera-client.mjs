// Focused target generation and framing; shared scenarios exercise real hosts.
import assert from "node:assert/strict";
import test from "node:test";
import {
  encodeManifestLayout,
  generateClient,
  hostAnnouncement,
  manifestVariant,
  replyToHostCreate,
} from "./generated-client.mjs";

const client = await generateClient("camera-picking");
const { codec, manifest } = client;
const layout = (name, fields) => encodeManifestLayout(client, name, fields);
const tag = (name) => manifestVariant(client, name);
const output = {
  world: { id: 3n, incarnation: 9n },
  entity: 4n,
  kind: "camera",
  incarnation: 8n,
};
const viewport = { width: 640, height: 480, devicePixelRatio: 1.25 };
const publication = { host: 7n, revision: 11n };
const binding = {
  output,
  viewport,
  generation: { host: 7n, serial: 2n },
};
const view = { kind: "root", output, expectedViewport: viewport };
const center = { type: "GeometryPickQuery", view, x: 0.5, y: 0.5 };
const descriptor = { output, publication, viewport };

const outputLayout = (value) =>
  layout("output-reference", {
    world: layout("world-reference", value.world),
    target:
      value.kind === "canvas"
        ? layout("output-target-canvas", { tag: tag("OUTPUT_TARGET_CANVAS") })
        : layout("output-target-camera", {
            tag: tag("OUTPUT_TARGET_CAMERA"),
            entity: value.entity,
            incarnation: value.incarnation,
          }),
  });
const viewportLayout = (value) =>
  layout("view-viewport", {
    width: value.width,
    height: value.height,
    device_pixel_ratio: value.devicePixelRatio,
  });
const publicationLayout = (value) => layout("publication-reference", value);
const viewRootLayout = layout("view-root", {
  tag: tag("VIEW_ROOT"),
  output: outputLayout(output),
  expected_viewport: viewportLayout(viewport),
});
const descriptorLayout = layout("view-descriptor", {
  output: outputLayout(output),
  publication: publicationLayout(publication),
  viewport: viewportLayout(viewport),
});
const bindingLayout = layout("root-binding", {
  output: outputLayout(output),
  width: viewport.width,
  height: viewport.height,
  device_pixel_ratio: viewport.devicePixelRatio,
  generation: layout("presentation-identity", binding.generation),
});

test("view plane requests default to false and require boolean flags", () => {
  for (const includeViewPlane of [undefined, false, true]) {
    const query = {
      ...center,
      ...(includeViewPlane === undefined ? {} : { includeViewPlane }),
    };
    const bytes = codec.encodeRequest({
      session: 7n,
      requestId: 1n,
      body: { kind: "query", query },
    });
    assert.deepEqual(
      bytes,
      layout("request-geometry-pick", {
        session: 7n,
        request_id: 1n,
        tag: tag("REQUEST_GEOMETRY_PICK"),
        view: viewRootLayout,
        x: 0.5,
        y: 0.5,
        include_view_plane: includeViewPlane ?? false,
      }).bytes,
    );
  }
  for (const includeViewPlane of [null, 0, 1, "true", {}, []]) {
    assert.throws(
      () =>
        codec.encodeRequest({
          session: 7n,
          requestId: 1n,
          body: { kind: "query", query: { ...center, includeViewPlane } },
        }),
      /boolean/,
    );
  }
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 1n,
        body: {
          kind: "query",
          query: { ...center, view: { ...view, kind: "latest" } },
        },
      }),
    /view target required/,
  );
});

function hitLayout(part, include, requestId = 1n) {
  return layout("response-geometry-pick", {
    session: 7n,
    request_id: requestId,
    tick: 3n,
    tag: tag("RESPONSE_GEOMETRY_PICK"),
    result: layout("pick-result-hit", {
      tag: tag("PICK_OUTCOME_HIT"),
      view: descriptorLayout,
      world: layout("world-reference", { id: 5n, incarnation: 6n }),
      publication: publicationLayout(publication),
      entity: 42n,
      incarnation: 2n,
      position_x: 1,
      position_y: 2,
      position_z: 3,
      distance: 4,
      part,
      path: [
        layout("view-path-entry", {
          world: layout("world-reference", { id: 5n, incarnation: 6n }),
          anchor: 12n,
        }),
      ],
      view_plane: include
        ? layout("pick-view-plane", {
            point_x: 1,
            point_y: 2,
            point_z: 3,
            normal_x: 0,
            normal_y: 0,
            normal_z: -1,
          })
        : null,
    }),
  }).bytes;
}

test("hit responses carry exact view, identity, path and optional finite planes and reject malformed payloads", () => {
  for (const part of [0, 8]) {
    for (const include of [false, true]) {
      const bytes = hitLayout(part, include);
      const event = codec.decodeResponse(bytes, 7n).body.event;
      assert.deepEqual(event.view, descriptor);
      assert.equal(event.ok, true);
      assert.deepEqual(event.hit, {
        world: { id: 5n, incarnation: 6n },
        publication,
        entity: 42n,
        incarnation: 2n,
        position: [1, 2, 3],
        distance: 4,
        part,
        path: [{ world: { id: 5n, incarnation: 6n }, anchor: 12n }],
        ...(include
          ? { viewPlane: { point: [1, 2, 3], normal: [0, 0, -1] } }
          : {}),
      });
      assert.throws(() => codec.decodeResponse(bytes.slice(0, -1), 7n));
      const trailing = new Uint8Array([...bytes, 0]);
      assert.throws(() => codec.decodeResponse(trailing, 7n));
      const malformed = bytes.slice();
      const optionAt = bytes.length - (include ? 25 : 1);
      malformed[optionAt] = 2;
      assert.throws(
        () => codec.decodeResponse(malformed, 7n),
        /view plane option/,
      );
      if (include) {
        for (let coordinate = 0; coordinate < 6; coordinate++) {
          const invalid = bytes.slice();
          new DataView(invalid.buffer).setFloat32(
            optionAt + 1 + coordinate * 4,
            NaN,
            true,
          );
          assert.throws(() => codec.decodeResponse(invalid, 7n));
        }
      }
    }
  }
});

async function connect(sendFailure) {
  let events;
  const sent = [];
  let closes = 0;
  const client = await codec.IppClient.connectTransport(
    {
      start(value) {
        events = value;
        events.ready();
      },
      send(bytes) {
        if (replyToHostCreate(bytes, events)) return;
        if (sent.length > 0 && sendFailure)
          throw new Error("transport unavailable");
        sent.push(bytes.slice());
        if (sent.length === 1) {
          const reply = hostAnnouncement(codec);
          events.message(reply);
        }
      },
      async close() {
        closes++;
      },
    },
    { selectedSystems: [], logLevel: "off" },
  );
  return {
    client,
    sent,
    emit: (bytes) => events.message(bytes),
    get closes() {
      return closes;
    },
  };
}

function result(requestId, error, tick = 1n) {
  return layout("response-geometry-pick", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_GEOMETRY_PICK"),
    result: error
      ? layout("pick-result-failure", {
          tag: tag("PICK_OUTCOME_FAILURE"),
          reason: error,
        })
      : layout("pick-result-miss", {
          tag: tag("PICK_OUTCOME_MISS"),
          view: descriptorLayout,
        }),
  }).bytes;
}

function navigated(requestId, tick = 1n) {
  return layout("response-camera-navigated", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_CAMERA_NAVIGATED"),
  }).bytes;
}

function renderState(requestId = 0n) {
  return layout("response-render-state-updated", {
    session: 7n,
    request_id: requestId,
    tick: 1n,
    tag: tag("RESPONSE_RENDER_STATE_UPDATED"),
    changes: layout("render-state-patch", {
      mask: 1,
      showAllDebugGeometries: true,
      debugGeometryColor: null,
      ambientLight: null,
    }),
  }).bytes;
}

const requestId = (bytes) =>
  new DataView(bytes.buffer, bytes.byteOffset).getBigUint64(8, true);

test("baseline contracts expose camera navigation, geometry queries and registrations", () => {
  for (const name of ["sendCommand", "query", "navigateCamera"])
    assert.equal(name in codec.IppClient.prototype, true);
  assert.equal("sendEvent" in codec.IppClient.prototype, false);
  assert.equal("onCameraStateChanged" in codec.IppClient.prototype, false);
  assert.equal("Camera" in codec.components, true);
  assert.equal("PickingGeometry" in codec.components, true);
  assert.equal("REQUEST_CAMERA_NAVIGATE" in codec.WIRE, true);
  assert.equal("REQUEST_GEOMETRY_PICK" in codec.WIRE, true);
  assert.equal(codec.Camera.id, codec.components.Camera.id);
  assert.equal(codec.GEOMETRY_TYPE, codec.WIRE.ASSET_GEOMETRY);
  assert.equal(
    manifest.ASSET_FORMATS.ASSET_GEOMETRY.typeId,
    codec.GEOMETRY_TYPE,
  );
  assert.ok(manifest.ASSET_FORMATS.ASSET_GEOMETRY.format.startsWith("IPPG;"));
  assert.equal("encodeBoundingShape" in codec, true);
  assert.equal(codec.Camera.fields.focus_distance.default, 6);
});

test("queries and navigation keep ordered correlation and exact terminal results", async () => {
  const { client, sent, emit } = await connect();
  try {
    const first = client.query(center);
    const navigation = client.navigateCamera({
      binding,
      motion: { kind: "zoom", amount: 0.25 },
    });
    const second = client.query(center);
    assert.deepEqual(sent.slice(-3).map(requestId), [1n, 2n, 3n]);
    emit(result(1n));
    emit(navigated(2n));
    emit(result(3n, "camera binding is stale"));
    assert.deepEqual(await first, {
      session: 7n,
      requestId: 1n,
      tick: 1n,
      type: "GeometryPickResultEvent",
      view: descriptor,
      ok: true,
      hit: null,
    });
    assert.equal(await navigation, undefined);
    assert.equal((await second).error, "camera binding is stale");
  } finally {
    await client.close();
  }
});

test("all motion variants conform to the canonical manifest and malformed fields reject locally", async () => {
  const { client, sent } = await connect();
  const pending = [];
  try {
    for (const [kind, motion, first, second] of [
      [0, { kind: "rotate", yaw: 0.25, pitch: -0.5 }, 0.25, -0.5],
      [1, { kind: "pan", x: 0.25, y: -0.5 }, 0.25, -0.5],
      [2, { kind: "zoom", amount: 0.25 }, 0.25, 0],
    ]) {
      pending.push(
        client.navigateCamera({ binding, publication, motion }).catch(() => {}),
      );
      assert.deepEqual(
        sent.at(-1),
        layout("request-camera-navigate", {
          session: 7n,
          request_id: requestId(sent.at(-1)),
          tag: tag("REQUEST_CAMERA_NAVIGATE"),
          binding: bindingLayout,
          publication: publicationLayout(publication),
          kind,
          first,
          second,
        }).bytes,
      );
    }
    for (const motion of [
      { kind: "rotate", yaw: NaN, pitch: 0 },
      { kind: "rotate", yaw: 0, pitch: 0, extra: 1 },
      { kind: "pan", x: 0, y: 0, width: 10, height: 10 },
      { kind: "zoom", amount: Infinity },
      { kind: "teleport", amount: 1 },
    ])
      await assert.rejects(client.navigateCamera({ binding, motion }), {
        code: "IPP_REQUEST_NOT_SENT",
      });
    await assert.rejects(
      client.navigateCamera({
        binding,
        motion: { kind: "zoom", amount: 1 },
        unknown: true,
      }),
      { code: "IPP_REQUEST_NOT_SENT" },
    );
    await assert.rejects(client.query({ ...center, x: NaN }), {
      code: "IPP_REQUEST_NOT_SENT",
    });
    await assert.rejects(client.query({ ...center, unknown: true }), {
      code: "IPP_REQUEST_NOT_SENT",
    });
  } finally {
    await client.close();
    await Promise.all(pending);
  }
});

test("command and query ID rules and misrouted notifications reject", () => {
  for (const [requestId, body] of [
    [
      1n,
      {
        kind: "command",
        command: {
          type: "RenderStateUpdateCommand",
          changes: { showAllDebugGeometries: true },
        },
      },
    ],
    [0n, { kind: "query", query: center }],
    [
      0n,
      {
        kind: "cameraNavigate",
        request: { binding, motion: { kind: "zoom", amount: 1 } },
      },
    ],
  ])
    assert.throws(
      () => codec.encodeRequest({ session: 7n, requestId, body }),
      /reserved request identity/,
    );
  assert.throws(
    () => codec.decodeResponse(renderState(1n), 7n),
    /reserved response identity/,
  );
  assert.throws(
    () => codec.decodeResponse(result(0n), 7n),
    /reserved response identity/,
  );
  assert.throws(
    () => codec.decodeResponse(navigated(0n), 7n),
    /reserved response identity/,
  );
});

test("wrong query identity, old sessions, and transport failures terminate only their session", async () => {
  for (const mutation of [
    (bytes) => new DataView(bytes.buffer).setBigUint64(8, 99n, true),
    (bytes) => new DataView(bytes.buffer).setBigUint64(0, 8n, true),
  ]) {
    const connection = await connect();
    const pending = connection.client.query(center);
    const bytes = result(1n);
    mutation(bytes);
    connection.emit(bytes);
    await assert.rejects(pending);
    assert.ok(connection.closes > 0);
    assert.throws(
      () =>
        connection.client.sendCommand({
          type: "RenderStateUpdateCommand",
          changes: { showAllDebugGeometries: true },
        }),
      /closed/,
    );
    await connection.client.close();
  }
  const connection = await connect(true);
  assert.throws(
    () =>
      connection.client.sendCommand({
        type: "RenderStateUpdateCommand",
        changes: { showAllDebugGeometries: true },
      }),
    /transport unavailable/,
  );
  assert.ok(connection.closes > 0);
  await connection.client.close();
});

test("a malformed notification cannot consume a pending batch correlation", async () => {
  const { client, emit } = await connect();
  const pending = client.batch([]);
  emit(renderState(1n));
  await assert.rejects(pending, /reserved response identity/);
  await client.close();
});

test("projection queries preserve their own result type and reject cross-query replies", async () => {
  const projection = {
    type: "CameraProjectQuery",
    view,
    x: 1.25,
    y: -0.5,
    plane: { point: [0, 0, 0], normal: [0, 0, -1] },
  };
  const connection = await connect();
  try {
    const pendingProjection = connection.client.query(projection);
    const pendingPick = connection.client.query(center);
    connection.emit(result(2n));
    connection.emit(
      layout("response-camera-project", {
        session: 7n,
        request_id: 1n,
        tick: 1n,
        tag: tag("RESPONSE_CAMERA_PROJECT"),
        view: descriptorLayout,
        ok: true,
        position: layout("world-point", { x: 1, y: 2, z: 0 }),
        error: null,
      }).bytes,
    );
    const projected = await pendingProjection;
    assert.deepEqual(projected.view, descriptor);
    assert.deepEqual(projected.position, [1, 2, 0]);
    assert.equal((await pendingPick).hit, null);
    for (const plane of [
      null,
      { point: [0, 0], normal: [0, 0, 1] },
      { point: [0, 0, 0], normal: [0, NaN, 1] },
      { point: [0, 0, 0], normal: [0, 0, 1], extra: true },
    ]) {
      await assert.rejects(connection.client.query({ ...projection, plane }), {
        code: "IPP_REQUEST_NOT_SENT",
      });
    }
  } finally {
    await connection.client.close();
  }
  const wrong = await connect();
  const pending = wrong.client.query(projection);
  wrong.emit(result(1n));
  await assert.rejects(pending, /query response correlation/);
  await wrong.client.close();
});

test("geometry encoding grows beyond prior part and byte quotas and rejects cycles", () => {
  const leaf = { type: "sphere", radius: 1 };
  const encoded = codec.encodeBoundingShape({
    type: "compound",
    parts: Array(14_000).fill(leaf),
  });
  assert.equal(new DataView(encoded.buffer).getUint32(8, true), 14_000);
  assert.ok(encoded.length > 1 << 20);
  let nested = leaf;
  for (let i = 0; i < 40; i++) nested = { type: "compound", parts: [nested] };
  assert.deepEqual(
    codec.encodeBoundingShape(nested),
    codec.encodeBoundingShape(leaf),
  );
  const cyclic = { type: "compound", parts: [] };
  cyclic.parts.push(cyclic);
  assert.throws(() => codec.encodeBoundingShape(cyclic), /nesting/);
});
