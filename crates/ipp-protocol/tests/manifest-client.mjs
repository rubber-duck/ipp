// Manifest-derived byte fixtures mechanically validate the generated TypeScript codec.
import assert from "node:assert/strict";
import test from "node:test";
import {
  encodeManifestLayout,
  generateClient,
  manifestVariant,
  nonemptyBytesDefaultContract,
  noncreatableContract,
} from "./generated-client.mjs";

const { codec } = await generateClient("manifest", []);
const bytesDefault = await generateClient(
  "bytes-default",
  [],
  nonemptyBytesDefaultContract,
  noncreatableContract,
);

const noCreation = await generateClient(
  "noncreatable",
  [],
  noncreatableContract,
);

test("noncreatable descriptors preserve field helpers and native insertion stays outside the wire", () => {
  assert.equal(noCreation.codec.components.Scalar.creatable, false);
  assert.equal(
    noCreation.codec.components.Scalar.fields.value.default,
    undefined,
  );
  assert.equal("insert" in noCreation.codec.Scalar, false);
  assert.equal(typeof noCreation.codec.Scalar.setValue, "function");
  assert.notEqual(noCreation.codec.SCHEMA_HASH, bytesDefault.codec.SCHEMA_HASH);
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 1n,
        body: {
          kind: "batch",
          batch: {
            id: 1n,
            operations: [
              {
                kind: "insertComponentValue",
                entity: { kind: "handle", id: 1n },
                value: {},
              },
            ],
          },
        },
      }),
    /unsupported command/,
  );
  assert.equal(
    Object.keys(codec.WIRE).some((name) =>
      name.includes("INSERT_COMPONENT_VALUE"),
    ),
    false,
  );
});

test("baseline field, scene and lifecycle codecs conform to their manifest", () => {
  assert.deepEqual(
    bytesDefault.codec.components.Scalar.fields.value.default,
    [1, 2, 3],
  );
  assert.equal(
    Object.isFrozen(bytesDefault.codec.components.Scalar.fields.value.default),
    true,
  );
  assert.throws(() => {
    bytesDefault.codec.components.Scalar.fields.value.default[0] = 9;
  }, TypeError);
  assert.deepEqual(codec.TARGET.features["skeletal-animation"], {
    id: 16,
    enabled: false,
  });
  assert.equal("request-upload-asset" in codec.WIRE_LAYOUTS, false);
  assert.equal("uploadAsset" in codec.IppClient.prototype, false);
  assert.deepEqual(codec.ASSET_FORMATS.ASSET_MESH, {
    capability: "textures",
    typeId: codec.WIRE.ASSET_MESH,
    format: codec.ASSET_FORMATS.ASSET_MESH.format,
  });
  for (const descriptor of [
    codec.TARGET,
    codec.TARGET.features,
    codec.WIRE,
    codec.WIRE_TAG_LAYOUTS,
    codec.WIRE_LAYOUTS,
    codec.WIRE_LAYOUTS["request-batch"].fields,
    codec.WIRE_LAYOUTS["request-batch"].fields[0],
    codec.ASSET_FORMATS,
    codec.components,
    codec.components.Scalar,
    codec.components.Scalar.fields,
    codec.components.Scalar.fields.value,
  ])
    assert.equal(Object.isFrozen(descriptor), true);
  assert.throws(() => {
    codec.components.Scalar.fields.value.offset = 101;
  }, TypeError);
  assert.equal(
    codec.WIRE_LAYOUTS.component.fields.find((field) => field.name === "fields")
      .limit,
    65536,
  );
  assert.equal(
    codec.WIRE_LAYOUTS["command-insert"].fields.find(
      (field) => field.name === "fields",
    ).limit,
    256,
  );
  // The generated codec derives the message and inspected-byte bounds from the
  // exported contract; they cannot drift from the Rust declarations.
  const inspectedBytes = codec.WIRE_LAYOUTS["snapshot-value-bytes"].fields.find(
    (field) => field.name === "value",
  ).limit;
  assert.equal(
    codec.MAX_MESSAGE_BYTES,
    Number(codec.WIRE_CONVENTIONS["max-message-bytes"]),
  );
  assert.equal(codec.INSPECTED_BYTES_LIMIT, inspectedBytes);
  assert.equal(codec.INSPECTED_BYTES_LIMIT, codec.MAX_MESSAGE_BYTES);
  assert.doesNotMatch(
    codec.decodeResponse.toString() + codec.encodeRequest.toString(),
    /1_?048_?576/,
  );
  const covered = new Set();
  const tag = (name) => {
    covered.add(name);
    return manifestVariant(codec, name);
  };
  const layout = (name, values) => encodeManifestLayout(codec, name, values);
  const alias = (value) =>
    layout("reference-alias", { tag: tag("REF_ALIAS"), alias: value });
  const handle = (value) =>
    layout("reference-handle", { tag: tag("REF_HANDLE"), handle: value });
  const metadata = (symbolicId, classes) =>
    layout("metadata", { symbolic_id: symbolicId, classes });
  const value = (name, value) =>
    layout(`value-${name}`, {
      tag: tag(`VALUE_${name.toUpperCase()}`),
      value,
    });
  const field = (offset, encodedValue) =>
    layout("field", { offset, value: encodedValue });
  const snapshotField = (offset, encodedValue) =>
    layout("snapshot-field", { offset, value: encodedValue });
  const command = (name, values) =>
    layout(`command-${name}`, {
      tag: tag(`COMMAND_${name.replaceAll("-", "_").toUpperCase()}`),
      ...values,
    });

  for (const [kind, name, tagName, extra] of [
    ["beginBatch", "request-begin-batch", "REQUEST_BEGIN_BATCH", {}],
    ["endBatch", "request-end-batch", "REQUEST_END_BATCH", { batch_id: 27n }],
    [
      "batchChunk",
      "request-batch",
      "REQUEST_BATCH_CHUNK",
      { batch_id: 27n, operations: [] },
    ],
  ]) {
    const body =
      kind === "beginBatch"
        ? { kind }
        : kind === "endBatch"
          ? { kind, batchId: 27n }
          : { kind, batch: { id: 27n, operations: [] } };
    assert.deepEqual(
      codec.encodeRequest({ session: 7n, requestId: 17n, body }),
      layout(name, {
        session: 7n,
        request_id: 17n,
        tag: tag(tagName),
        ...extra,
      }).bytes,
    );
  }
  for (const [kind, name, tagName, requestId, extra] of [
    [
      "batchStarted",
      "response-batch-identity",
      "RESPONSE_BATCH_STARTED",
      17n,
      {},
    ],
    [
      "batchFinished",
      "response-batch-identity",
      "RESPONSE_BATCH_FINISHED",
      18n,
      {},
    ],
    [
      "batchAborted",
      "response-batch-aborted",
      "RESPONSE_BATCH_ABORTED",
      0n,
      { message: "deadline" },
    ],
  ]) {
    const bytes = layout(name, {
      session: 7n,
      request_id: requestId,
      tick: 1n,
      tag: tag(tagName),
      batch_id: 27n,
      ...extra,
    });
    assert.deepEqual(codec.decodeResponse(bytes.bytes, 7n), {
      session: 7n,
      requestId,
      tick: 1n,
      body: { kind, batchId: 27n, ...extra },
    });
  }

  const refs = {
    alias: { kind: "alias", alias: 31 },
    handle: { kind: "handle", id: 41n },
  };
  const meta = { symbolicId: "oracle", classes: ["first", "second"] };
  const writes = [
    { offset: 11, value: { kind: "f32", value: 1.25 } },
    { offset: 22, value: { kind: "entity", value: refs.handle } },
    { offset: 33, value: { kind: "u32", value: 0x12345678 } },
    { offset: 44, value: { kind: "u64", value: 0x1020304050607080n } },
    { offset: 55, value: { kind: "string", value: "oracle-value" } },
    { offset: 66, value: { kind: "bytes", value: new Uint8Array([9, 7, 5]) } },
    { offset: 77, value: { kind: "bool", value: true } },
    {
      offset: 88,
      value: { kind: "dynamic", value: { kind: "f32", value: 1 } },
    },
  ];
  const encodedWrites = [
    field(11, value("f32", 1.25)),
    field(22, value("entity", handle(41n))),
    field(33, value("u32", 0x12345678)),
    field(44, value("u64", 0x1020304050607080n)),
    field(55, value("string", "oracle-value")),
    field(66, value("bytes", new Uint8Array([9, 7, 5]))),
    field(77, value("bool", true)),
    field(88, value("dynamic", new Uint8Array([1, 0, 0, 128, 63]))),
  ];
  const operations = [
    { kind: "create", alias: 1, metadata: meta },
    { kind: "delete", entity: refs.handle },
    { kind: "setMetadata", entity: refs.alias, metadata: meta },
    {
      kind: "insertComponent",
      entity: refs.alias,
      component: 321,
      fields: writes,
    },
    { kind: "setField", entity: refs.handle, component: 322, field: writes[4] },
    { kind: "removeComponent", entity: refs.alias, component: 323 },
    { kind: "createStateOverlayOwner", alias: 2 },
    { kind: "releaseStateOverlayOwner", owner: refs.handle },
    {
      kind: "attachEntityOverlayBinding",
      owner: refs.alias,
      alias: 3,
      symbolicId: "owned",
      mode: "owned",
    },
    {
      kind: "attachEntityOverlayBinding",
      owner: refs.handle,
      alias: 4,
      symbolicId: "bound",
      mode: "bound",
    },
    {
      kind: "releaseEntityOverlayBinding",
      owner: refs.alias,
      binding: refs.handle,
    },
    ...["auto", "bound", "owned"].map((mode, index) => ({
      kind: "attachComponentStateOverlay",
      owner: refs.alias,
      binding: refs.handle,
      alias: 10 + index,
      component: 330 + index,
      mode,
      fields: [writes[index]],
    })),
    {
      kind: "updateComponentStateOverlay",
      owner: refs.handle,
      overlay: refs.alias,
      fields: [writes[3]],
      clear: [71, 72],
    },
    {
      kind: "releaseComponentStateOverlay",
      owner: refs.alias,
      overlay: refs.handle,
    },
  ];
  const encodedOperations = [
    command("create", {
      alias: 1,
      metadata: metadata("oracle", ["first", "second"]),
    }),
    command("delete", { entity: handle(41n) }),
    command("metadata", {
      entity: alias(31),
      metadata: metadata("oracle", ["first", "second"]),
    }),
    command("insert", {
      entity: alias(31),
      component: 321,
      fields: encodedWrites,
    }),
    command("set", {
      entity: handle(41n),
      component: 322,
      field: encodedWrites[4],
    }),
    command("remove", { entity: alias(31), component: 323 }),
    command("create-state-overlay-owner", { alias: 2 }),
    command("release-state-overlay-owner", { owner: handle(41n) }),
    command("attach-entity-overlay-binding", {
      owner: alias(31),
      alias: 3,
      symbolic_id: "owned",
      mode: tag("ENTITY_OVERLAY_MODE_OWNED"),
    }),
    command("attach-entity-overlay-binding", {
      owner: handle(41n),
      alias: 4,
      symbolic_id: "bound",
      mode: tag("ENTITY_OVERLAY_MODE_BOUND"),
    }),
    command("release-entity-overlay-binding", {
      owner: alias(31),
      binding: handle(41n),
    }),
    ...[
      ["AUTO", 10],
      ["BOUND", 11],
      ["OWNED", 12],
    ].map(([mode, aliasValue], index) =>
      command("attach-component-state-overlay", {
        owner: alias(31),
        binding: handle(41n),
        alias: aliasValue,
        component: 330 + index,
        mode: tag(`COMPONENT_OVERLAY_MODE_${mode}`),
        fields: [encodedWrites[index]],
      }),
    ),
    command("update-component-state-overlay", {
      owner: handle(41n),
      overlay: alias(31),
      fields: [encodedWrites[3]],
      clear: [71, 72],
    }),
    command("release-component-state-overlay", {
      owner: alias(31),
      overlay: handle(41n),
    }),
  ];
  operations.push(
    {
      kind: "setDynamicProperty",
      entity: refs.handle,
      component: 20,
      name: "x",
      value: { kind: "f32", value: 1 },
    },
    {
      kind: "removeDynamicProperty",
      entity: refs.handle,
      component: 20,
      name: "x",
    },
    {
      kind: "updateDynamicComponentStateOverlay",
      owner: refs.handle,
      overlay: refs.alias,
      properties: { x: { kind: "f32", value: 1 } },
      clear: ["y"],
    },
  );
  encodedOperations.push(
    command("set-dynamic-property", {
      entity: handle(41n),
      component: 20,
      name: "x",
      value: new Uint8Array([1, 0, 0, 128, 63]),
    }),
    command("remove-dynamic-property", {
      entity: handle(41n),
      component: 20,
      name: "x",
    }),
    command("update-dynamic-component-state-overlay", {
      owner: handle(41n),
      overlay: alias(31),
      properties: [
        layout("dynamic-property-write", {
          name: "x",
          value: new Uint8Array([1, 0, 0, 128, 63]),
        }),
      ],
      clear: [layout("dynamic-property-name", { name: "y" })],
    }),
  );
  const request = {
    session: 7n,
    requestId: 17n,
    body: { kind: "batch", batch: { id: 27n, operations } },
  };
  const expected = layout("request-batch", {
    session: 7n,
    request_id: 17n,
    tag: tag("REQUEST_BATCH"),
    batch_id: 27n,
    operations: encodedOperations,
  }).bytes;
  assert.deepEqual(codec.encodeRequest(request), expected);
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 18n,
      body: { kind: "inspect", collection: "summary" },
    }),
    layout("request-inspect", {
      session: 7n,
      request_id: 18n,
      tag: tag("REQUEST_INSPECT"),
      collection: tag("INSPECT_SUMMARY"),
      after: 0n,
      target: 0n,
      limit: 256,
    }).bytes,
  );
  const aliasHandle = (aliasValue, id) =>
    layout("alias-handle", { alias: aliasValue, handle: id });
  const resourceAlias = (aliasValue, id, kind, entity) =>
    layout("state-overlay-alias", {
      alias: aliasValue,
      id,
      kind: tag(`STATE_OVERLAY_KIND_${kind}`),
      entity,
    });
  const success = layout("outcome-success", {
    batch_id: 27n,
    tick: 37n,
    tag: tag("OUTCOME_SUCCESS"),
    aliases: [aliasHandle(1, 0x100000001n)],
    stateOverlays: [
      resourceAlias(2, 51n, "OWNER", null),
      resourceAlias(3, 52n, "ENTITY_BINDING", 0x100000001n),
      resourceAlias(4, 53n, "COMPONENT", 0x100000001n),
    ],
  });
  covered.add("OPTION_NONE");
  covered.add("OPTION_SOME");
  const batchBytes = layout("response-batch", {
    session: 7n,
    request_id: 17n,
    tick: 37n,
    tag: tag("RESPONSE_BATCH"),
    outcome: success,
  }).bytes;
  assert.deepEqual(codec.decodeResponse(batchBytes, 7n), {
    session: 7n,
    requestId: 17n,
    tick: 37n,
    body: {
      kind: "batch",
      outcome: {
        batchId: 27n,
        tick: 37n,
        ok: true,
        aliases: [{ alias: 1, id: 0x100000001n }],
        stateOverlays: [
          { alias: 2, id: 51n, kind: "owner", entity: null },
          {
            alias: 3,
            id: 52n,
            kind: "entityOverlayBinding",
            entity: 0x100000001n,
          },
          {
            alias: 4,
            id: 53n,
            kind: "componentStateOverlay",
            entity: 0x100000001n,
          },
        ],
      },
    },
  });
  const failure = layout("outcome-failure", {
    batch_id: 28n,
    tick: 38n,
    tag: tag("OUTCOME_FAILURE"),
    scope: tag("BATCH_ERROR_OPERATION"),
    operation: 7,
    reason: "InvalidField",
    aliases: [layout("alias-handle", { alias: 9, handle: 17n })],
    stateOverlays: [resourceAlias(2, 51n, "OWNER", null)],
  });
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-batch", {
        session: 7n,
        request_id: 18n,
        tick: 38n,
        tag: tag("RESPONSE_BATCH"),
        outcome: failure,
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 18n,
      tick: 38n,
      body: {
        kind: "batch",
        outcome: {
          batchId: 28n,
          tick: 38n,
          ok: false,
          error: { scope: "operation", operation: 7, reason: "InvalidField" },
          aliases: [{ alias: 9, id: 17n }],
          stateOverlays: [{ alias: 2, id: 51n, kind: "owner", entity: null }],
        },
      },
    },
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-frame", {
        session: 7n,
        request_id: 0n,
        tick: 39n,
        tag: tag("RESPONSE_FRAME"),
        time: 1.75,
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 0n,
      tick: 39n,
      body: { kind: "frame", time: 1.75 },
    },
  );

  const status = (name, fields = {}) =>
    layout(`resource-status-${name}`, {
      tag: tag(`RESOURCE_${name.toUpperCase()}`),
      ...fields,
    });
  const resource = (id, statusValue) =>
    layout("resource", {
      representation: layout("asset-representation", {
        decoded: false,
        graphics_ready: null,
        source_bytes: 0n,
        resident_bytes: 0n,
        graphics_bytes: null,
      }),
      id,
      kind: 1,
      source: `https://example.test/${id}`,
      variant: Number(id),
      status: statusValue,
    });
  const resources = [
    resource(61n, status("unloaded")),
    resource(62n, status("start")),
    resource(63n, status("progress", { completed: 13n, total: 23n })),
    resource(64n, status("loaded")),
    resource(65n, status("failed", { error: "network" })),
  ];
  const snapshotValue = (name, value) =>
    layout(`snapshot-value-${name}`, {
      tag: tag(`SNAPSHOT_VALUE_${name.toUpperCase()}`),
      ...(name === "entity"
        ? { reference_tag: tag("SNAPSHOT_REF_HANDLE") }
        : {}),
      value,
    });
  const scalar = layout("component", {
    type_id: codec.components.Scalar.id,
    fields: [
      snapshotField(
        codec.components.Scalar.fields.value.offset,
        snapshotValue("f32", 2.5),
      ),
    ],
  });
  const linearDriver = layout("component", {
    type_id: codec.components.LinearDriver.id,
    fields: [
      snapshotField(
        codec.components.LinearDriver.fields.source.offset,
        snapshotValue("entity", 0x100000002n),
      ),
      snapshotField(
        codec.components.LinearDriver.fields.scale.offset,
        snapshotValue("f32", 1.5),
      ),
      snapshotField(
        codec.components.LinearDriver.fields.bias.offset,
        snapshotValue("f32", -0.25),
      ),
    ],
  });
  const meshInstance = layout("component", {
    type_id: codec.components.MeshInstance.id,
    fields: [
      snapshotField(
        codec.components.MeshInstance.fields.source.offset,
        snapshotValue("string", "https://example.test/mesh"),
      ),
      snapshotField(
        codec.components.MeshInstance.fields.variant.offset,
        snapshotValue("u32", 0xa1b2c3d4),
      ),
    ],
  });
  const entity = layout("entity", {
    id: 0x100000001n,
    metadata: metadata(null, ["inspected"]),
    base: [scalar, linearDriver, meshInstance],
    effective: [scalar],
  });
  const inspectBytes = layout("response-inspect", {
    session: 7n,
    request_id: 20n,
    tick: 40n,
    tag: tag("RESPONSE_INSPECT"),
    next: 0n,
    time: 2.25,
    entities: [entity],
    resources,
    controllers: [],
    render_diagnostics: [
      layout("render-diagnostic", {
        entity: 0x100000001n,
        reason: "InvalidAsset",
      }),
    ],
  }).bytes;
  const inspected = codec.decodeResponse(inspectBytes, 7n);
  assert.equal(inspected.session, 7n);
  assert.equal(inspected.requestId, 20n);
  assert.equal(inspected.tick, 40n);
  assert.equal(inspected.body.entities[0].id, 0x100000001n);
  assert.deepEqual(inspected.body.entities[0].metadata, {
    symbolicId: null,
    classes: ["inspected"],
  });
  assert.equal(inspected.body.entities[0].base[0].fields.value, 2.5);
  assert.equal(inspected.body.entities[0].base[1].fields.source, 0x100000002n);
  assert.equal(inspected.body.entities[0].base[1].fields.scale, 1.5);
  assert.equal(inspected.body.entities[0].base[1].fields.bias, -0.25);
  assert.equal(
    inspected.body.entities[0].base[2].fields.source,
    "https://example.test/mesh",
  );
  assert.equal(inspected.body.entities[0].base[2].fields.variant, 0xa1b2c3d4);
  assert.equal(inspected.body.renderDiagnostics[0].entity, 0x100000001n);
  assert.equal(inspected.body.renderDiagnostics[0].reason, "InvalidAsset");

  const lifecycle = [
    ["ENTITY_DELETED", null],
    ["COMPONENT_REPLACED", 321],
    ["COMPONENT_REMOVED", 322],
  ].map(([reason, component], index) =>
    layout("state-overlay-lifecycle-diagnostic", {
      owner: 71n + BigInt(index),
      stateOverlay: 81n + BigInt(index),
      entity: 0x100000001n,
      component,
      reason: tag(`STATE_OVERLAY_${reason}`),
    }),
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-state-overlay-lifecycle", {
        session: 7n,
        request_id: 0n,
        tick: 41n,
        tag: tag("RESPONSE_STATE_OVERLAY_LIFECYCLE"),
        diagnostics: lifecycle,
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 0n,
      tick: 41n,
      body: {
        kind: "lifecycle",
        diagnostics: [
          {
            owner: 71n,
            stateOverlay: 81n,
            entity: 0x100000001n,
            component: null,
            reason: "EntityDeleted",
          },
          {
            owner: 72n,
            stateOverlay: 82n,
            entity: 0x100000001n,
            component: 321,
            reason: "ComponentReplaced",
          },
          {
            owner: 73n,
            stateOverlay: 83n,
            entity: 0x100000001n,
            component: 322,
            reason: "ComponentRemoved",
          },
        ],
      },
    },
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-resources", {
        session: 7n,
        request_id: 0n,
        tick: 43n,
        tag: tag("RESPONSE_RESOURCES"),
        resources,
      }).bytes,
      7n,
    ).body.resources,
    [
      {
        id: 61n,
        kind: 1,
        source: "https://example.test/61",
        variant: 61,
        status: "unloaded",
        representation: {
          decoded: false,
          graphicsReady: null,
          sourceBytes: 0n,
          residentBytes: 0n,
          graphicsBytes: null,
        },
      },
      {
        id: 62n,
        kind: 1,
        source: "https://example.test/62",
        variant: 62,
        status: "start",
        representation: {
          decoded: false,
          graphicsReady: null,
          sourceBytes: 0n,
          residentBytes: 0n,
          graphicsBytes: null,
        },
      },
      {
        id: 63n,
        kind: 1,
        source: "https://example.test/63",
        variant: 63,
        status: "progress",
        representation: {
          decoded: false,
          graphicsReady: null,
          sourceBytes: 0n,
          residentBytes: 0n,
          graphicsBytes: null,
        },
        completed: 13n,
        total: 23n,
      },
      {
        id: 64n,
        kind: 1,
        source: "https://example.test/64",
        variant: 64,
        status: "loaded",
        representation: {
          decoded: false,
          graphicsReady: null,
          sourceBytes: 0n,
          residentBytes: 0n,
          graphicsBytes: null,
        },
      },
      {
        id: 65n,
        kind: 1,
        source: "https://example.test/65",
        variant: 65,
        status: "failed",
        representation: {
          decoded: false,
          graphicsReady: null,
          sourceBytes: 0n,
          residentBytes: 0n,
          graphicsBytes: null,
        },
        error: "network",
      },
    ],
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-error", {
        session: 7n,
        request_id: 24n,
        tick: 44n,
        tag: tag("RESPONSE_ERROR"),
        code: 3,
        message: "unavailable",
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 24n,
      tick: 44n,
      body: { kind: "error", code: 3, message: "unavailable" },
    },
  );

  for (const [event, name, fields] of [
    [
      { type: "CameraActivateCommand", entity: 41n },
      "camera-activate",
      { entity: 41n },
    ],
    [
      {
        type: "GeometryPickQuery",
        x: 0.25,
        y: 0.75,
        width: 640,
        height: 480,
      },
      "geometry-pick",
      { x: 0.25, y: 0.75, width: 640, height: 480, include_view_plane: false },
    ],
  ]) {
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: event.type === "GeometryPickQuery" ? 25n : 0n,
        body:
          event.type === "GeometryPickQuery"
            ? { kind: "query", query: event }
            : { kind: "command", command: event },
      }),
      layout(`request-${name}`, {
        session: 7n,
        request_id: event.type === "GeometryPickQuery" ? 25n : 0n,
        tag: tag(`REQUEST_${name.toUpperCase().replaceAll("-", "_")}`),
        ...fields,
      }).bytes,
    );
  }
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-camera-state-changed", {
        session: 7n,
        request_id: 0n,
        tick: 45n,
        tag: tag("RESPONSE_CAMERA_STATE_CHANGED"),
        changes: layout("camera-state-patch", {
          mask: 1,
          activeCamera: layout("camera-entity", { id: 41n }),
        }),
      }).bytes,
      7n,
    ).body,
    {
      kind: "event",
      event: {
        type: "CameraStateChangedEvent",
        changes: { activeCamera: 41n },
      },
    },
  );
  for (const motion of [
    { kind: "rotate", yaw: 0.25, pitch: -0.5 },
    { kind: "pan", x: 0.25, y: -0.5, width: 640, height: 480 },
    { kind: "zoom", amount: 0.25 },
  ]) {
    const { kind, ...fields } = motion;
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 0n,
        body: {
          kind: "command",
          command: { type: "CameraNavigateCommand", motion },
        },
      }),
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
  const plane = layout("pick-view-plane", {
    point_x: 1,
    point_y: 2,
    point_z: 3,
    normal_x: 0,
    normal_y: 0,
    normal_z: -1,
  });
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 29n,
      body: {
        kind: "query",
        query: {
          type: "CameraProjectQuery",
          x: 1.25,
          y: -0.5,
          width: 640,
          height: 480,
          plane: { point: [1, 2, 3], normal: [0, 0, -1] },
        },
      },
    }),
    layout("request-camera-project", {
      session: 7n,
      request_id: 29n,
      tag: tag("REQUEST_CAMERA_PROJECT"),
      x: 1.25,
      y: -0.5,
      width: 640,
      height: 480,
      plane,
    }).bytes,
  );
  for (const [ok, position, error] of [
    [true, [1, 2, 3], null],
    [true, null, null],
    [false, null, "InvalidViewport"],
  ]) {
    const bytes = layout("response-camera-project", {
      session: 7n,
      request_id: 29n,
      tick: 47n,
      tag: tag("RESPONSE_CAMERA_PROJECT"),
      camera: 41n,
      ok,
      position: position && layout("world-point", { x: 1, y: 2, z: 3 }),
      error,
    }).bytes;
    assert.deepEqual(codec.decodeResponse(bytes, 7n).body.event, {
      type: "CameraProjectResultEvent",
      camera: 41n,
      ok,
      ...(ok ? { position } : { error }),
    });
    assert.throws(() => codec.decodeResponse(bytes.slice(0, -1), 7n));
  }
  const hitFields = {
    entity: 41n,
    position_x: 1,
    position_y: 2,
    position_z: 3,
    distance: 4,
    part: 0,
    view_plane: null,
  };
  const hit = { entity: 41n, position: [1, 2, 3], distance: 4 };
  for (const [camera, result, expected] of [
    [
      41n,
      layout("pick-result-miss", { tag: tag("PICK_OUTCOME_MISS") }),
      { ok: true, hit: null },
    ],
    [
      41n,
      layout("pick-result-hit", { tag: tag("PICK_OUTCOME_HIT"), ...hitFields }),
      { ok: true, hit: { ...hit, part: 0 } },
    ],
    [
      41n,
      layout("pick-result-hit", {
        tag: tag("PICK_OUTCOME_HIT"),
        ...hitFields,
        part: 8,
        view_plane: layout("pick-view-plane", {
          point_x: 1,
          point_y: 2,
          point_z: 3,
          normal_x: 0,
          normal_y: 0,
          normal_z: -1,
        }),
      }),
      {
        ok: true,
        hit: {
          ...hit,
          part: 8,
          viewPlane: { point: [1, 2, 3], normal: [0, 0, -1] },
        },
      },
    ],
    [
      null,
      layout("pick-result-failure", {
        tag: tag("PICK_OUTCOME_FAILURE"),
        reason: "NoActiveCamera",
      }),
      { ok: false, error: "NoActiveCamera" },
    ],
  ]) {
    assert.deepEqual(
      codec.decodeResponse(
        layout("response-geometry-pick", {
          session: 7n,
          request_id: 26n,
          tick: 46n,
          tag: tag("RESPONSE_GEOMETRY_PICK"),
          camera,
          result,
        }).bytes,
        7n,
      ).body,
      {
        kind: "event",
        event: { type: "GeometryPickResultEvent", camera, ...expected },
      },
    );
  }

  const patch = layout("render-state-patch", {
    mask: 3,
    ambientLight: null,
    showAllDebugGeometries: false,
    debugGeometryColor: layout("linear-rgb", { r: 0.25, g: 0.5, b: 1 }),
  });
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 0n,
      body: {
        kind: "command",
        command: {
          type: "RenderStateUpdateCommand",
          changes: {
            showAllDebugGeometries: false,
            debugGeometryColor: [0.25, 0.5, 1],
          },
        },
      },
    }),
    layout("request-render-state-update", {
      session: 7n,
      request_id: 0n,
      tag: tag("REQUEST_RENDER_STATE_UPDATE"),
      changes: patch,
    }).bytes,
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-render-state-updated", {
        session: 7n,
        request_id: 0n,
        tick: 47n,
        tag: tag("RESPONSE_RENDER_STATE_UPDATED"),
        changes: patch,
      }).bytes,
      7n,
    ).body.event,
    {
      type: "RenderStateUpdatedEvent",
      changes: {
        showAllDebugGeometries: false,
        debugGeometryColor: [0.25, 0.5, 1],
      },
    },
  );
  const debug = codec.components.BoundingGeometry;
  const debugComponent = layout("component", {
    type_id: debug.id,
    fields: Object.values(debug.fields).map((field) =>
      snapshotField(
        field.offset,
        snapshotValue(
          {
            1: "f32",
            2: "entity",
            3: "u32",
            4: "u64",
            5: "string",
            6: "bytes",
            7: "bool",
          }[field.kind],
          field.kind === 6 ? new Uint8Array(field.default) : field.default,
        ),
      ),
    ),
  });
  const debugInspection = codec.decodeResponse(
    layout("response-inspect", {
      session: 7n,
      request_id: 28n,
      tick: 48n,
      tag: tag("RESPONSE_INSPECT"),
      next: 0n,
      time: 0,
      entities: [
        layout("entity", {
          id: 41n,
          metadata: metadata(null, []),
          base: [debugComponent],
          effective: [debugComponent],
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes,
    7n,
  );
  assert.equal(
    debugInspection.body.entities[0].effective[0].fields.is_rendered,
    false,
  );
  assert.equal(debugInspection.body.entities[0].base[0].fields.outline, false);

  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 1n,
      body: {
        kind: "subscribeLifecycle",
        subscription: 2n,
        filter: {
          entities: true,
          components: true,
          assets: true,
          entity: 42n,
          component: 1,
          asset: 9n,
        },
      },
    }),
    layout("request-lifecycle-subscribe", {
      session: 7n,
      request_id: 1n,
      tag: tag("REQUEST_LIFECYCLE_SUBSCRIBE"),
      subscription: 2n,
      domains: 7,
      entity: 42n,
      component: 1,
      asset: 9n,
    }).bytes,
  );
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 2n,
      body: { kind: "unsubscribeLifecycle", subscription: 2n },
    }),
    layout("request-lifecycle-unsubscribe", {
      session: 7n,
      request_id: 2n,
      tag: tag("REQUEST_LIFECYCLE_UNSUBSCRIBE"),
      subscription: 2n,
    }).bytes,
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-lifecycle-subscription", {
        session: 7n,
        request_id: 1n,
        tick: 3n,
        tag: tag("RESPONSE_LIFECYCLE_SUBSCRIPTION"),
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 1n,
      tick: 3n,
      body: { kind: "lifecycleSubscription" },
    },
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-lifecycle-overflow", {
        session: 7n,
        request_id: 0n,
        tick: 3n,
        tag: tag("RESPONSE_LIFECYCLE_OVERFLOW"),
        dropped: 129n,
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 0n,
      tick: 3n,
      body: { kind: "lifecycleOverflow", dropped: 129n },
    },
  );

  const lifecycleObservations = [];
  const encodedObservations = [];
  for (const [change, variant] of [
    ["created", "LIFECYCLE_ENTITY_CREATED"],
    ["metadataChanged", "LIFECYCLE_ENTITY_METADATA_CHANGED"],
    ["deleted", "LIFECYCLE_ENTITY_DELETED"],
  ]) {
    lifecycleObservations.push({ kind: "entity", entity: 42n, change });
    encodedObservations.push(
      layout("lifecycle-entity", { tag: tag(variant), entity: 42n }),
    );
  }
  for (const [change, variant, previousIncarnation, incarnation] of [
    ["inserted", "LIFECYCLE_COMPONENT_INSERTED", null, 1n],
    ["updated", "LIFECYCLE_COMPONENT_UPDATED", 1n, 1n],
    ["replaced", "LIFECYCLE_COMPONENT_REPLACED", 1n, 2n],
    ["removed", "LIFECYCLE_COMPONENT_REMOVED", 2n, null],
  ]) {
    lifecycleObservations.push({
      kind: "component",
      entity: 42n,
      component: 1,
      change,
      previousIncarnation,
      incarnation,
    });
    encodedObservations.push(
      layout("lifecycle-component", {
        tag: tag(variant),
        entity: 42n,
        component: 1,
        previous_incarnation: previousIncarnation ?? 0n,
        incarnation: incarnation ?? 0n,
      }),
    );
  }
  for (const [change, variant] of [
    ["graphicsInvalidated", "LIFECYCLE_ASSET_GRAPHICS_INVALIDATED"],
    ["statusChanged", "LIFECYCLE_ASSET_STATUS_CHANGED"],
    ["removed", "LIFECYCLE_ASSET_REMOVED"],
  ]) {
    lifecycleObservations.push({
      kind: "asset",
      change,
      resource: {
        id: 9n,
        kind: 1,
        source: "https://example.test/9",
        variant: 9,
        status: "unloaded",
        representation: {
          decoded: false,
          graphicsReady: null,
          sourceBytes: 0n,
          residentBytes: 0n,
          graphicsBytes: null,
        },
      },
    });
    encodedObservations.push(
      layout("lifecycle-asset", {
        tag: tag(variant),
        resource: resource(9n, status("unloaded")),
      }),
    );
  }
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-lifecycle-events", {
        session: 7n,
        request_id: 0n,
        tick: 3n,
        tag: tag("RESPONSE_LIFECYCLE_EVENTS"),
        events: encodedObservations.map((observation, index) =>
          layout("lifecycle-publication", {
            subscription: 2n,
            sequence: BigInt(index + 1),
            tick: 2n,
            observation,
          }),
        ),
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 0n,
      tick: 3n,
      body: {
        kind: "lifecycleEvents",
        events: lifecycleObservations.map((observation, index) => ({
          subscription: 2n,
          sequence: BigInt(index + 1),
          tick: 2n,
          observation,
        })),
      },
    },
  );

  const custom = codec.components.CustomMaterial;
  const kinds = { 1: "f32", 3: "u32", 5: "string", 7: "bool" };
  const customSnapshot = layout("component", {
    type_id: custom.id,
    fields: [
      ...Object.values(custom.fields).map((f) =>
        snapshotField(f.offset, snapshotValue(kinds[f.kind], f.default)),
      ),
      snapshotField(
        0x80000000,
        snapshotValue(
          "bytes",
          new Uint8Array([
            2, 0, 0, 128, 1, 0, 0, 0, 1, 0, 0, 0, 120, 1, 0, 0, 128, 1,
          ]),
        ),
      ),
      snapshotField(
        0x80000001,
        snapshotValue("dynamic", new Uint8Array([1, 0, 0, 128, 63])),
      ),
    ],
  });
  const dynamicInspection = codec.decodeResponse(
    layout("response-inspect", {
      session: 7n,
      request_id: 999n,
      tick: 40n,
      tag: tag("RESPONSE_INSPECT"),
      next: 0n,
      time: 2.25,
      entities: [
        layout("entity", {
          id: 0x100000001n,
          metadata: metadata(null, []),
          base: [customSnapshot],
          effective: [customSnapshot],
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes,
    7n,
  );
  assert.deepEqual(
    { ...dynamicInspection.body.entities[0].effective[0].properties },
    { x: { kind: "f32", value: 1 } },
  );
  // Dense descriptor tables exceed the old 64 KiB byte bound. The effective
  // snapshot refers to the base table instead of repeating it.
  const denseNames = Array.from(
    { length: 2500 },
    (_, index) => `node_${index}_background_corner_radius`,
  );
  const encoder = new TextEncoder();
  const denseTable = (() => {
    const parts = [];
    const u32 = (value) => {
      const bytes = new Uint8Array(4);
      new DataView(bytes.buffer).setUint32(0, value, true);
      parts.push(bytes);
    };
    u32(0x80000001 + denseNames.length);
    u32(denseNames.length);
    denseNames.forEach((name, index) => {
      const bytes = encoder.encode(name);
      u32(bytes.length);
      parts.push(bytes);
      u32(0x80000001 + index);
      parts.push(new Uint8Array([1]));
    });
    const table = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
    let at = 0;
    for (const part of parts) {
      table.set(part, at);
      at += part.length;
    }
    return table;
  })();
  assert.ok(denseTable.length > 65536, `table has ${denseTable.length} bytes`);
  const denseSnapshot = (descriptors, value) =>
    layout("component", {
      type_id: custom.id,
      fields: [
        ...Object.values(custom.fields).map((f) =>
          snapshotField(f.offset, snapshotValue(kinds[f.kind], f.default)),
        ),
        snapshotField(0x80000000, descriptors),
        ...denseNames.map((_, index) => {
          const bytes = new Uint8Array(5);
          bytes[0] = 1;
          new DataView(bytes.buffer).setFloat32(1, value + index, true);
          return snapshotField(
            0x80000001 + index,
            snapshotValue("dynamic", bytes),
          );
        }),
      ],
    });
  const baseDescriptors = layout("snapshot-value-base-descriptors", {
    tag: tag("SNAPSHOT_VALUE_BASE_DESCRIPTORS"),
  });
  const denseInspection = (base, effective) =>
    layout("response-inspect", {
      session: 7n,
      request_id: 1001n,
      tick: 40n,
      tag: tag("RESPONSE_INSPECT"),
      next: 0n,
      time: 2.25,
      entities: [
        layout("entity", {
          id: 0x100000001n,
          metadata: metadata(null, []),
          base,
          effective,
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes;
  const denseBytes = denseInspection(
    [denseSnapshot(snapshotValue("bytes", denseTable), 0)],
    [denseSnapshot(baseDescriptors, 0.5)],
  );
  assert.ok(denseBytes.length < codec.MAX_MESSAGE_BYTES);
  const dense = codec.decodeResponse(denseBytes, 7n).body.entities[0];
  for (const [snapshot, offset] of [
    [dense.base[0], 0],
    [dense.effective[0], 0.5],
  ]) {
    assert.equal(Object.keys(snapshot.properties).length, denseNames.length);
    assert.deepEqual(snapshot.properties[denseNames.at(-1)], {
      kind: "f32",
      value: denseNames.length - 1 + offset,
    });
  }
  // A reference is valid only after a base table for the same component.
  assert.throws(
    () =>
      codec.decodeResponse(
        denseInspection([], [denseSnapshot(baseDescriptors, 0)]),
        7n,
      ),
    /no base table/,
  );
  assert.throws(
    () =>
      codec.decodeResponse(
        denseInspection([denseSnapshot(baseDescriptors, 0)], []),
        7n,
      ),
    /invalid dynamic inspection/,
  );

  const oversizedSnapshot = {
    layout: customSnapshot.layout,
    bytes: customSnapshot.bytes.slice(),
  };
  new DataView(
    oversizedSnapshot.bytes.buffer,
    oversizedSnapshot.bytes.byteOffset,
  ).setUint32(2, 65537, true);
  assert.throws(
    () =>
      codec.decodeResponse(
        layout("response-inspect", {
          session: 7n,
          request_id: 1000n,
          tick: 40n,
          tag: tag("RESPONSE_INSPECT"),
          next: 0n,
          time: 2.25,
          entities: [
            layout("entity", {
              id: 0x100000001n,
              metadata: metadata(null, []),
              base: [oversizedSnapshot],
              effective: [],
            }),
          ],
          resources: [],
          controllers: [],
          render_diagnostics: [],
        }).bytes,
        7n,
      ),
    /count limit/,
  );

  for (const collection of [
    "entities",
    "resources",
    "controllers",
    "renderDiagnostics",
  ]) {
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 19n,
        body: { kind: "inspect", collection },
      }),
      layout("request-inspect", {
        session: 7n,
        request_id: 19n,
        tag: tag("REQUEST_INSPECT"),
        collection: tag(
          `INSPECT_${collection.replace(/[A-Z]/g, (letter) => `_${letter}`).toUpperCase()}`,
        ),
        after: 0n,
        target: 0n,
        limit: 256,
      }).bytes,
    );
  }
  for (const scope of ["draw", "resource", "context", "world"]) {
    const body = codec.decodeResponse(
      layout("response-runtime-failure", {
        session: 7n,
        request_id: 0n,
        tick: 1n,
        tag: tag("RESPONSE_RUNTIME_FAILURE"),
        scope: tag(`FAILURE_${scope.toUpperCase()}`),
        faulted: scope === "world",
        message: "fixture failure",
      }).bytes,
      7n,
    ).body;
    assert.deepEqual(body, {
      kind: "runtimeFailure",
      scope,
      faulted: scope === "world",
      message: "fixture failure",
    });
  }
  const commit = layout("outcome-failure", {
    batch_id: 28n,
    tick: 38n,
    tag: tag("OUTCOME_FAILURE"),
    scope: tag("BATCH_ERROR_COMMIT"),
    operation: null,
    reason: "NonConvergentCommit",
    aliases: [],
    stateOverlays: [],
  });
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-batch", {
        session: 7n,
        request_id: 18n,
        tick: 38n,
        tag: tag("RESPONSE_BATCH"),
        outcome: commit,
      }).bytes,
      7n,
    ).body.outcome.error,
    { scope: "commit", operation: null, reason: "NonConvergentCommit" },
  );

  // Playback/controller branches are covered against this same manifest in animation-client.mjs.
  const animationTag = (name) =>
    codec.WIRE_TAG_LAYOUTS[name].capability === "animation";
  const unreachableSnapshotKinds = new Set(["SNAPSHOT_VALUE_U64"]);
  // No component in this compiled target exposes resolved u64 fields yet. Their authored
  // encoders and Rust resolved writer remain covered; a registered field makes this set drift.
  const compiledKinds = new Set(
    Object.values(codec.components).flatMap((component) =>
      Object.values(component.fields).map((field) => field.kind),
    ),
  );
  for (const name of unreachableSnapshotKinds)
    assert.equal(compiledKinds.has(codec.WIRE[name]), false, name);
  assert.deepEqual(
    [...covered].sort(),
    Object.keys(codec.WIRE_TAG_LAYOUTS)
      .filter(
        (name) =>
          !unreachableSnapshotKinds.has(name) &&
          !animationTag(name) &&
          // Physical Host controls/selectors are exercised by the maintained
          // native and worker Host lifecycle/persistence suites, separately
          // from this World-envelope fixture.
          ![23, 24, 25].includes(codec.WIRE_TAG_LAYOUTS[name].space),
      )
      .sort(),
  );
});

test("dynamic value helpers expose explicit types and inference detaches numeric inputs", async () => {
  const { inferDynamicValue } = await import(
    "../../../packages/ipp-client/dist/index.js"
  );
  assert.deepEqual(codec.mat2(1, 0, 0, 1), {
    kind: "mat2",
    value: [1, 0, 0, 1],
  });
  assert.deepEqual(codec.mat2([1, 0, 0, 1]), codec.mat2(1, 0, 0, 1));
  for (const [name, values] of [
    ["vec2", [1, 2]],
    ["vec3", [1, 2, 3]],
    ["vec4", [1, 2, 3, 4]],
    ["mat3", Array(9).fill(1)],
    ["mat4", Array(16).fill(1)],
  ]) {
    assert.deepEqual(codec[name](values), { kind: name, value: values });
    assert.deepEqual(inferDynamicValue(values), { kind: name, value: values });
  }
  assert.deepEqual(inferDynamicValue(3), { kind: "f32", value: 3 });
  assert.deepEqual(inferDynamicValue(false), { kind: "bool", value: false });
  assert.deepEqual(codec.i32(-3), { kind: "i32", value: -3 });
  assert.deepEqual(codec.u32(3), { kind: "u32", value: 3 });
  assert.deepEqual(
    inferDynamicValue(codec.mat2(1, 0, 0, 1)),
    codec.mat2(1, 0, 0, 1),
  );
  assert.deepEqual(
    inferDynamicValue("file:///texture.png"),
    codec.texture2D("file:///texture.png"),
  );
  const authored = [1, 0, 0, 1];
  const captured = inferDynamicValue(authored);
  authored[0] = 9;
  assert.equal(captured.value[0], 1);
  for (const invalid of [
    null,
    [],
    [1],
    [1, 2, 3, 4, 5],
    [1, Number.NaN],
    Infinity,
    {},
  ])
    assert.throws(() => inferDynamicValue(invalid));
  assert.throws(() => codec.i32(1.5));
  assert.throws(() => codec.u32(-1));
  assert.throws(() => codec.mat2(1, 0));
});

test("general asset values preserve payload type and shaders alone require textures", async () => {
  const { encodeDynamicValue, decodeDynamicValue, inferDynamicValue } =
    await import("../../../packages/ipp-client/dist/index.js");
  const mesh = codec.asset(1, "file:///model.mesh", 7);
  const bytes = encodeDynamicValue(mesh);
  assert.deepEqual([...bytes.subarray(0, 7)], [12, 1, 0, 7, 0, 0, 0]);
  assert.deepEqual(decodeDynamicValue(bytes), mesh);
  assert.deepEqual(
    codec.texture2D("file:///image.png", 3),
    codec.asset(2, "file:///image.png", 3),
  );
  assert.equal(
    codec.shaderParameterKind(codec.texture2D("file:///image.png")),
    "texture2D",
  );
  assert.throws(() => codec.shaderParameterKind(mesh), /2D texture/);
  assert.throws(() => decodeDynamicValue(new Uint8Array([11, 0, 0, 0, 0])));
  assert.throws(() => decodeDynamicValue(bytes.subarray(0, 6)));
  assert.throws(() => codec.asset(65536, "file:///model.mesh"));
  assert.throws(() => codec.asset(-1, "file:///model.mesh"));
  const captured = inferDynamicValue(mesh);
  mesh.value.source = "file:///changed.mesh";
  assert.equal(captured.value.source, "file:///model.mesh");
});
