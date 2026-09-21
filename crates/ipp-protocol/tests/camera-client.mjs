// Focused target generation and framing; shared scenarios exercise real hosts.
import assert from "node:assert/strict";
import test, { mock } from "node:test";
import {
  encodeManifestLayout,
  generateClient,
  replyToHostCreate,
  manifestVariant,
} from "./generated-client.mjs";

const minimal = await generateClient("camera-minimal");
const scene = await generateClient("camera-scene", []);
const { codec } = await generateClient("camera-picking", []);
const layout = (name, fields) => encodeManifestLayout(codec, name, fields);
const tag = (name) => manifestVariant(codec, name);
const center = {
  type: "GeometryPickQuery",
  x: 0.5,
  y: 0.5,
  width: 640,
  height: 480,
};

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
        x: 0.5,
        y: 0.5,
        width: 640,
        height: 480,
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
});

test("primitive and compound responses carry optional finite planes and reject malformed payloads", () => {
  for (const part of [0, 8]) {
    for (const include of [false, true]) {
      const bytes = layout("response-geometry-pick", {
        session: 7n,
        request_id: 1n,
        tick: 3n,
        tag: tag("RESPONSE_GEOMETRY_PICK"),
        camera: 41n,
        result: layout("pick-result-hit", {
          tag: tag("PICK_OUTCOME_HIT"),
          entity: 42n,
          position_x: 1,
          position_y: 2,
          position_z: 3,
          distance: 4,
          part,
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
      const event = codec.decodeResponse(bytes, 7n).body.event;
      assert.deepEqual(event.hit, {
        entity: 42n,
        position: [1, 2, 3],
        distance: 4,
        part,
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
    { logLevel: "off" },
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

function selection(camera, tick = 1n, requestId = 0n) {
  return layout("response-camera-state-changed", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_CAMERA_STATE_CHANGED"),
    changes: layout("camera-state-patch", {
      mask: 1,
      activeCamera: layout("camera-entity", { id: camera }),
    }),
  }).bytes;
}
function result(requestId, camera = 41n, error, tick = 1n) {
  return layout("response-geometry-pick", {
    session: 7n,
    request_id: requestId,
    tick,
    tag: tag("RESPONSE_GEOMETRY_PICK"),
    camera,
    result: error
      ? layout("pick-result-failure", {
          tag: tag("PICK_OUTCOME_FAILURE"),
          reason: error,
        })
      : layout("pick-result-miss", { tag: tag("PICK_OUTCOME_MISS") }),
  }).bytes;
}

test("baseline contracts expose camera commands, geometry queries and registrations", () => {
  for (const name of ["sendCommand", "query", "onCameraStateChanged"])
    assert.equal(name in minimal.codec.IppClient.prototype, true);
  assert.equal("query" in scene.codec.IppClient.prototype, true);
  assert.equal("sendEvent" in codec.IppClient.prototype, false);
  assert.equal("Camera" in minimal.codec.components, true);
  assert.equal("PickingGeometry" in scene.codec.components, true);
  assert.equal("REQUEST_CAMERA_NAVIGATE" in minimal.codec.WIRE, true);
  assert.equal("REQUEST_GEOMETRY_PICK" in scene.codec.WIRE, true);
  assert.equal(codec.Camera.id, codec.components.Camera.id);
  assert.equal(codec.GEOMETRY_TYPE, codec.WIRE.ASSET_GEOMETRY);
  assert.equal(codec.ASSET_FORMATS.ASSET_GEOMETRY.typeId, codec.GEOMETRY_TYPE);
  assert.ok(codec.ASSET_FORMATS.ASSET_GEOMETRY.format.startsWith("IPPG;"));
  assert.equal("encodeBoundingShape" in minimal.codec, true);
  assert.equal(codec.Camera.fields.focus_distance.default, 6);
  assert.equal(codec.SCHEMA_HASH, scene.codec.SCHEMA_HASH);
  assert.doesNotThrow(() =>
    scene.codec.encodeRequest({
      session: 7n,
      requestId: 1n,
      body: { kind: "query", query: center },
    }),
  );
});

test("commands allocate no identities or timers and queries preserve terminal correlation", async () => {
  const { client, sent, emit } = await connect();
  try {
    const timer = mock.method(globalThis, "setTimeout");
    try {
      for (let i = 0; i < 80; i++)
        assert.equal(
          client.sendCommand({ type: "CameraActivateCommand", entity: 41n }),
          undefined,
        );
      assert.equal(timer.mock.callCount(), 0);
    } finally {
      timer.mock.restore();
    }
    for (const bytes of sent.slice(1))
      assert.equal(new DataView(bytes.buffer).getBigUint64(8, true), 0n);
    const seen = [];
    client.onCameraStateChanged((event) => seen.push(event));
    const first = client.query(center);
    client.sendCommand({
      type: "CameraNavigateCommand",
      motion: { kind: "zoom", amount: 0.25 },
    });
    const second = client.query(center);
    assert.equal(new DataView(sent.at(-3).buffer).getBigUint64(8, true), 1n);
    assert.equal(new DataView(sent.at(-1).buffer).getBigUint64(8, true), 2n);
    emit(selection(41n));
    emit(result(1n));
    emit(result(2n, null, "NoActiveCamera"));
    assert.deepEqual(await first, {
      session: 7n,
      requestId: 1n,
      tick: 1n,
      type: "GeometryPickResultEvent",
      camera: 41n,
      ok: true,
      hit: null,
    });
    assert.equal((await second).error, "NoActiveCamera");
    assert.deepEqual(seen, [
      {
        session: 7n,
        requestId: 0n,
        tick: 1n,
        type: "CameraStateChangedEvent",
        changes: { activeCamera: 41n },
      },
    ]);
  } finally {
    await client.close();
  }
});

test("all motion variants conform to the canonical manifest and malformed fields reject locally", async () => {
  const { client, sent } = await connect();
  try {
    for (const motion of [
      { kind: "rotate", yaw: 0.25, pitch: -0.5 },
      { kind: "pan", x: 0.25, y: -0.5, width: 640, height: 480 },
      { kind: "zoom", amount: 0.25 },
    ]) {
      client.sendCommand({ type: "CameraNavigateCommand", motion });
      const { kind, ...fields } = motion;
      assert.deepEqual(
        sent.at(-1),
        layout("request-camera-navigate", {
          session: 7n,
          request_id: 0n,
          tag: tag("REQUEST_CAMERA_NAVIGATE"),
          motion: layout(`camera-motion-${kind}`, {
            tag: tag(`CAMERA_MOTION_${kind.toUpperCase()}`),
            ...fields,
          }),
        }).bytes,
      );
    }
    for (const motion of [
      { kind: "rotate", yaw: NaN, pitch: 0 },
      { kind: "rotate", yaw: 0, pitch: 0, extra: 1 },
      { kind: "pan", x: 0, y: 0, width: -1, height: 2 },
      { kind: "zoom", amount: Infinity },
      { kind: "teleport", amount: 1 },
      null,
    ])
      assert.throws(
        () => client.sendCommand({ type: "CameraNavigateCommand", motion }),
        { code: "IPP_REQUEST_NOT_SENT" },
      );
    assert.throws(
      () =>
        client.sendCommand({
          type: "CameraActivateCommand",
          entity: 41n,
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
  }
});

test("command and query ID rules and malformed sparse notifications reject", () => {
  for (const [requestId, body] of [
    [
      1n,
      {
        kind: "command",
        command: { type: "CameraActivateCommand", entity: 41n },
      },
    ],
    [0n, { kind: "query", query: center }],
  ])
    assert.throws(
      () => codec.encodeRequest({ session: 7n, requestId, body }),
      /reserved request identity/,
    );
  assert.throws(
    () => codec.decodeResponse(selection(41n, 1n, 1n), 7n),
    /reserved response identity/,
  );
  assert.throws(
    () => codec.decodeResponse(result(0n), 7n),
    /reserved response identity/,
  );
  for (const mask of [0, 2, 65535]) {
    const bytes = selection(41n);
    new DataView(bytes.buffer).setUint16(25, mask, true);
    assert.throws(() => codec.decodeResponse(bytes, 7n), /camera state mask/);
  }
  assert.throws(() => codec.decodeResponse(selection(41n).slice(0, -1), 7n));
});

test("camera observers receive each notification once with isolated patches and unsubscribe", async () => {
  const { client, emit } = await connect();
  const seen = [];
  try {
    client.onCameraStateChanged((event) => {
      event.changes.activeCamera = 999n;
      throw new Error("observer failed");
    });
    const remove = client.onCameraStateChanged((event) => seen.push(event));
    emit(selection(41n));
    emit(selection(42n));
    assert.deepEqual(
      seen.map((event) => event.changes.activeCamera),
      [41n, 42n],
    );
    assert.ok(
      seen.every(
        (event) =>
          !Object.hasOwn(event, "ok") && !Object.hasOwn(event, "error"),
      ),
    );
    remove();
    emit(selection(43n));
    assert.equal(seen.length, 2);
  } finally {
    await client.close();
  }
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
          type: "CameraActivateCommand",
          entity: 41n,
        }),
      /closed/,
    );
    await connection.client.close();
  }
  const connection = await connect(true);
  assert.throws(
    () =>
      connection.client.sendCommand({
        type: "CameraActivateCommand",
        entity: 41n,
      }),
    /transport unavailable/,
  );
  assert.ok(connection.closes > 0);
  await connection.client.close();
});

test("a malformed notification cannot consume a pending batch correlation", async () => {
  const { client, emit } = await connect();
  const pending = client.batch([]);
  emit(selection(41n, 1n, 1n));
  await assert.rejects(pending, /reserved response identity/);
  await client.close();
});

test("projection queries preserve their own result type and reject cross-query replies", async () => {
  const projection = {
    type: "CameraProjectQuery",
    x: 1.25,
    y: -0.5,
    width: 640,
    height: 480,
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
        camera: 41n,
        ok: true,
        position: layout("world-point", { x: 1, y: 2, z: 0 }),
        error: null,
      }).bytes,
    );
    assert.deepEqual((await pendingProjection).position, [1, 2, 0]);
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
  const encoded = minimal.codec.encodeBoundingShape({
    type: "compound",
    parts: Array(14_000).fill(leaf),
  });
  assert.equal(new DataView(encoded.buffer).getUint32(8, true), 14_000);
  assert.ok(encoded.length > 1 << 20);
  let nested = leaf;
  for (let i = 0; i < 40; i++) nested = { type: "compound", parts: [nested] };
  assert.deepEqual(
    minimal.codec.encodeBoundingShape(nested),
    minimal.codec.encodeBoundingShape(leaf),
  );
  const cyclic = { type: "compound", parts: [] };
  cyclic.parts.push(cyclic);
  assert.throws(() => minimal.codec.encodeBoundingShape(cyclic), /nesting/);
});
