// Manifest-derived byte fixtures mechanically validate the generated TypeScript codec.
import assert from "node:assert/strict";
import test from "node:test";
import { executeCancellationConformance } from "./presentation-cancellation.mjs";
import {
  ROWS_FIXTURE_PROPERTIES,
  encodeManifestLayout,
  generateClient,
  manifestVariant,
  nonemptyBytesDefaultContract,
  noncreatableContract,
  rowsFieldContract,
} from "./generated-client.mjs";

const client = await generateClient("manifest", []);
const { codec, manifest, source } = client;
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

const rows = await generateClient("rows", [], rowsFieldContract);

/** Table bytes in the documented encoding; `rows` maps slot to present values. */
function rowsTable(nextSlot, entries) {
  const bytes = [];
  const view = new DataView(new ArrayBuffer(4));
  const u32 = (value) => {
    view.setUint32(0, value, true);
    bytes.push(...new Uint8Array(view.buffer));
  };
  const f32 = (value) => {
    view.setFloat32(0, value, true);
    bytes.push(...new Uint8Array(view.buffer));
  };
  u32(nextSlot);
  u32(entries.length);
  for (const [slot, values] of entries) {
    u32(slot);
    let mask = 0;
    ROWS_FIXTURE_PROPERTIES.forEach((property, index) => {
      if (property.name in values) mask |= 1 << index;
    });
    bytes.push(mask);
    for (const property of ROWS_FIXTURE_PROPERTIES) {
      const value = values[property.name];
      if (value === undefined) continue;
      if (property.kind === "text") {
        const text =
          value instanceof Uint8Array ? value : new TextEncoder().encode(value);
        u32(text.length);
        bytes.push(...text);
      } else if (property.kind === "asset") {
        const source = new TextEncoder().encode(value.source);
        bytes.push(value.kind & 0xff, value.kind >> 8);
        u32(value.variant);
        u32(source.length);
        bytes.push(...source);
      } else if (property.kind === "bool") u32(value ? 1 : 0);
      else if (property.kind === "i32") {
        view.setInt32(0, value, true);
        bytes.push(...new Uint8Array(view.buffer));
      } else if (property.kind === "u32") u32(value);
      else for (const lane of [value].flat()) f32(lane);
    }
  }
  return new Uint8Array(bytes);
}

test("schema rows fields generate typed row helpers and decode inspected tables", () => {
  const { codec, source } = rows;
  const scalar = codec.components.Scalar;
  assert.deepEqual(scalar.fields.items.rows, {
    regionBase: 0x10000000,
    properties: ROWS_FIXTURE_PROPERTIES.map(
      ({ name, kind, optional, hint, maxBytes }) => ({
        name,
        kind,
        optional,
        hint,
        ...(maxBytes === undefined ? {} : { maxBytes }),
      }),
    ),
  });
  assert.equal(Object.isFrozen(scalar.fields.items.rows.properties[2]), true);
  assert.match(source, /export interface ScalarItemsRow \{/);
  assert.match(source, /offset\?: readonly \[number, number, number\];/);
  assert.match(source, /export interface ScalarItemsRowPatch \{/);
  assert.match(
    source,
    /rotation\?: readonly \[number, number, number, number\] \| null;/,
  );
  assert.match(source, /weight\?: number;\n/);
  assert.match(source, /label\?: string;\n/);
  assert.match(source, /label\?: string \| null;\n/);

  // Property addresses: region base + slot * property count + property index.
  const rotation = 0x10000000 + 3 * 8 + 2;
  assert.equal(codec.rowFieldOffset(scalar, "items", 3, "rotation"), rotation);
  assert.equal(codec.Scalar.itemsOffset(3, "rotation"), rotation);
  assert.throws(
    () => codec.Scalar.itemsOffset(3, "missing"),
    /unknown row property/,
  );
  assert.throws(() => codec.Scalar.itemsOffset(-1, "weight"), /row slot/);
  assert.throws(
    () => codec.Scalar.itemsOffset(Math.floor(0x10000000 / 8), "weight"),
    /row slot/,
  );
  assert.equal("setItems" in codec.Scalar, false);
  assert.equal(typeof codec.Scalar.setValue, "function");

  const entity = { kind: "handle", id: 41n };
  const commands = codec.Scalar.patchItems(entity, 3, {
    rotation: null,
    weight: 2,
    texture: { kind: 4, source: "a.png", variant: 1 },
    label: "hé✓ok",
  });
  assert.deepEqual(
    commands.map((command) => command.field),
    [
      {
        offset: 0x10000000 + 24,
        value: { kind: "dynamic", value: { kind: "f32", value: 2 } },
      },
      { offset: rotation, value: { kind: "unset" } },
      {
        offset: 0x10000000 + 28,
        value: {
          kind: "dynamic",
          value: {
            kind: "asset",
            value: { kind: 4, source: "a.png", variant: 1 },
          },
        },
      },
      // Text is a string value; "hé✓ok" is exactly the 8-byte bound.
      { offset: 0x10000000 + 31, value: { kind: "string", value: "hé✓ok" } },
    ],
  );
  assert.deepEqual(
    codec.Scalar.patchItems(entity, 3, { label: null })[0].field,
    { offset: 0x10000000 + 31, value: { kind: "unset" } },
  );
  assert.throws(
    () => codec.Scalar.patchItems(entity, 3, { label: "hé✓ok!" }),
    /exceeds 8 bytes/,
  );
  assert.throws(
    () => codec.Scalar.patchItems(entity, 3, { label: 7 }),
    /requires text/,
  );
  assert.throws(
    () => codec.Scalar.patchItems(entity, 3, { weight: null }),
    /cannot be cleared/,
  );
  assert.throws(
    () => codec.Scalar.patchItems(entity, 3, { unknown: 1 }),
    /unknown row property/,
  );

  const covered = new Set();
  const tag = (name) => {
    covered.add(name);
    return manifestVariant(client, name);
  };
  const layout = (name, values) => encodeManifestLayout(client, name, values);
  const field = (offset, value) => layout("field", { offset, value });
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 3n,
      body: {
        kind: "submitBatch",
        batchId: 9,
        last: true,
        operations: [...commands.slice(0, 2), commands[3]],
      },
    }),
    layout("request-submit-batch", {
      session: 7n,
      request_id: 3n,
      tag: tag("REQUEST_SUBMIT_BATCH"),
      batch_id: 9,
      last: true,
      operations: [
        layout("command-set", {
          tag: tag("COMMAND_SET"),
          entity: layout("reference-handle", {
            tag: tag("REF_HANDLE"),
            handle: 41n,
          }),
          component: scalar.id,
          field: field(
            0x10000000 + 24,
            layout("value-dynamic", {
              tag: tag("VALUE_DYNAMIC"),
              value: new Uint8Array([1, 0, 0, 0, 64]),
            }),
          ),
        }),
        layout("command-set", {
          tag: tag("COMMAND_SET"),
          entity: layout("reference-handle", {
            tag: tag("REF_HANDLE"),
            handle: 41n,
          }),
          component: scalar.id,
          field: field(
            rotation,
            layout("value-unset", { tag: tag("VALUE_UNSET") }),
          ),
        }),
        layout("command-set", {
          tag: tag("COMMAND_SET"),
          entity: layout("reference-handle", {
            tag: tag("REF_HANDLE"),
            handle: 41n,
          }),
          component: scalar.id,
          field: field(
            0x10000000 + 31,
            layout("value-string", {
              tag: tag("VALUE_STRING"),
              value: "hé✓ok",
            }),
          ),
        }),
      ],
    }).bytes,
  );

  const table = rowsTable(6, [
    [1, { weight: 0.5, enabled: true, delta: -2, offset: [1, 2, 3] }],
    [
      4,
      {
        weight: 1,
        enabled: false,
        delta: 7,
        rotation: [0, 0, 0, 1],
        texture: { kind: 4, source: "é.png", variant: 2 },
        count: 9,
        label: "é✓",
      },
    ],
  ]);
  const decoded = codec.Scalar.decodeItems(table);
  assert.equal(decoded.nextSlot, 6);
  assert.deepEqual([...decoded.rows.keys()], [1, 4]);
  assert.deepEqual(decoded.rows.get(1), {
    weight: 0.5,
    offset: [1, 2, 3],
    enabled: true,
    delta: -2,
  });
  assert.deepEqual(decoded.rows.get(4), {
    weight: 1,
    rotation: [0, 0, 0, 1],
    enabled: false,
    texture: { kind: 4, source: "é.png", variant: 2 },
    delta: 7,
    count: 9,
    label: "é✓",
  });
  for (const malformed of [
    rowsTable(1, [[1, { weight: 1, enabled: true, delta: 0 }]]),
    rowsTable(3, [
      [2, { weight: 1, enabled: true, delta: 0 }],
      [1, { weight: 1, enabled: true, delta: 0 }],
    ]),
    rowsTable(2, [[1, { weight: 1, delta: 0 }]]),
    rowsTable(2, [
      [1, { weight: 1, enabled: true, delta: 0, label: "ninebytes" }],
    ]),
    rowsTable(2, [
      [
        1,
        { weight: 1, enabled: true, delta: 0, label: new Uint8Array([0xff]) },
      ],
    ]),
    new Uint8Array([...table, 0]),
    table.slice(0, table.length - 1),
  ])
    assert.throws(() => codec.Scalar.decodeItems(malformed));

  const snapshotValue = (name, value) =>
    layout(`snapshot-value-${name}`, {
      tag: tag(`SNAPSHOT_VALUE_${name.toUpperCase()}`),
      value,
    });
  const component = layout("component", {
    type_id: scalar.id,
    fields: [
      layout("snapshot-field", {
        offset: scalar.fields.value.offset,
        value: snapshotValue("f32", 2.5),
      }),
      layout("snapshot-field", {
        offset: scalar.fields.items.offset,
        value: snapshotValue("rows", table),
      }),
    ],
  });
  const inspected = codec.decodeResponse(
    layout("response-inspect", {
      session: 7n,
      request_id: 20n,
      tick: 40n,
      tag: tag("RESPONSE_INSPECT"),
      next: 0n,
      time: 0,
      entities: [
        layout("entity", {
          id: 0x100000001n,
          metadata: layout("metadata", { symbolic_id: null, classes: [] }),
          parent: 0n,
          order_low: 0n,
          order_high: 0n,
          components: [component],
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes,
    7n,
  );
  const fields = inspected.body.entities[0].components[0].fields;
  assert.equal(fields.value, 2.5);
  assert.deepEqual(fields.items, decoded);
  assert.ok(covered.has("SNAPSHOT_VALUE_ROWS"));
});

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
          kind: "submitBatch",
          batchId: 1,
          last: true,
          operations: [
            {
              kind: "insertComponentValue",
              entity: { kind: "handle", id: 1n },
              value: {},
            },
          ],
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

test("baseline field, scene and lifecycle codecs conform to their manifest", async () => {
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
  assert.equal("request-upload-asset" in manifest.WIRE_LAYOUTS, false);
  assert.equal("uploadAsset" in codec.IppClient.prototype, false);
  assert.deepEqual(manifest.ASSET_FORMATS.ASSET_MESH, {
    capability: "textures",
    typeId: codec.WIRE.ASSET_MESH,
    format: manifest.ASSET_FORMATS.ASSET_MESH.format,
  });
  for (const descriptor of [
    codec.TARGET,
    codec.TARGET.features,
    codec.WIRE,
    manifest.WIRE_TAG_LAYOUTS,
    manifest.WIRE_LAYOUTS,
    manifest.WIRE_LAYOUTS["request-submit-batch"].fields,
    manifest.WIRE_LAYOUTS["request-submit-batch"].fields[0],
    manifest.ASSET_FORMATS,
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
    manifest.WIRE_LAYOUTS.component.fields.find(
      (field) => field.name === "fields",
    ).limit,
    65536,
  );
  assert.equal(
    manifest.WIRE_LAYOUTS["command-insert"].fields.find(
      (field) => field.name === "fields",
    ).limit,
    256,
  );
  // The generated codec derives the message and inspected-byte bounds from the
  // exported contract; they cannot drift from the Rust declarations.
  const inspectedBytes = manifest.WIRE_LAYOUTS[
    "snapshot-value-bytes"
  ].fields.find((field) => field.name === "value").limit;
  assert.equal(
    codec.MAX_MESSAGE_BYTES,
    Number(manifest.WIRE_CONVENTIONS["max-message-bytes"]),
  );
  assert.equal(codec.INSPECTED_BYTES_LIMIT, inspectedBytes);
  // The shipped client carries resolved bounds only; the descriptive manifest
  // is a separate module for tests and tools.
  for (const name of [
    "WIRE_TAG_LAYOUTS",
    "WIRE_LAYOUTS",
    "WIRE_CONVENTIONS",
    "ASSET_FORMATS",
    "ENCODING",
  ]) {
    assert.equal(name in codec, false, name);
    assert.equal(source.includes(name), false, name);
  }
  assert.equal(manifest.SCHEMA_HASH, codec.SCHEMA_HASH);
  assert.equal(codec.INSPECTED_BYTES_LIMIT, codec.MAX_MESSAGE_BYTES);
  assert.doesNotMatch(
    codec.decodeResponse.toString() + codec.encodeRequest.toString(),
    /1_?048_?576/,
  );
  const covered = new Set();
  const tag = (name) => {
    covered.add(name);
    return manifestVariant(client, name);
  };
  const layout = (name, values) => encodeManifestLayout(client, name, values);
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
  const placement = (parent, before) =>
    layout("entity-placement", {
      parent:
        parent === null ? null : layout("reference-value", { value: parent }),
      before:
        before === null ? null : layout("reference-value", { value: before }),
    });

  // A non-final page is uncorrelated and carries a full u32 client identity.
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 0n,
      body: {
        kind: "submitBatch",
        batchId: 0xffff_ffff,
        last: false,
        operations: [],
      },
    }),
    layout("request-submit-batch", {
      session: 7n,
      request_id: 0n,
      tag: tag("REQUEST_SUBMIT_BATCH"),
      batch_id: 0xffff_ffff,
      last: false,
      operations: [],
    }).bytes,
  );
  for (const [requestId, last] of [
    [17n, false],
    [0n, true],
  ])
    assert.throws(
      () =>
        codec.encodeRequest({
          session: 7n,
          requestId,
          body: { kind: "submitBatch", batchId: 1, last, operations: [] },
        }),
      /reserved request identity/,
    );
  for (const [kind, name, tagName, requestId, extra] of [
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
    symbol: { kind: "symbol", symbol: "sibling" },
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
    { offset: 99, value: { kind: "rows", value: new Uint8Array(8) } },
    { offset: 0x10000002, value: { kind: "unset" } },
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
    field(99, value("rows", new Uint8Array(8))),
    field(0x10000002, layout("value-unset", { tag: tag("VALUE_UNSET") })),
  ];
  const operations = [
    { kind: "create", alias: 1, metadata: meta },
    { kind: "create", alias: 2, metadata: meta, adopt: true },
    { kind: "delete", entity: refs.handle },
    {
      kind: "placeEntity",
      entity: refs.alias,
      placement: { parent: refs.handle, before: refs.symbol },
    },
    { kind: "deleteSubtree", root: refs.symbol },
    { kind: "setMetadata", entity: refs.alias, metadata: meta },
    {
      kind: "insertComponent",
      entity: refs.alias,
      component: 321,
      fields: writes,
    },
    {
      kind: "insertComponent",
      entity: refs.symbol,
      component: 324,
      fields: [writes[4]],
      adopt: true,
    },
    { kind: "setField", entity: refs.handle, component: 322, field: writes[4] },
    {
      kind: "setFieldIf",
      entity: refs.symbol,
      component: 325,
      field: writes[4],
      expected: { kind: "string", value: "current" },
    },
    { kind: "removeComponent", entity: refs.alias, component: 323 },
  ];
  const symbol = (value) =>
    layout("reference-symbol", { tag: tag("REF_SYMBOL"), symbol: value });
  const encodedOperations = [
    command("create", {
      alias: 1,
      metadata: metadata("oracle", ["first", "second"]),
      adopt: false,
    }),
    command("create", {
      alias: 2,
      metadata: metadata("oracle", ["first", "second"]),
      adopt: true,
    }),
    command("delete", { entity: handle(41n) }),
    command("place-entity", {
      entity: alias(31),
      placement: placement(handle(41n), symbol("sibling")),
    }),
    command("delete-subtree", { root: symbol("sibling") }),
    command("metadata", {
      entity: alias(31),
      metadata: metadata("oracle", ["first", "second"]),
    }),
    command("insert", {
      entity: alias(31),
      component: 321,
      fields: encodedWrites,
      adopt: false,
    }),
    command("insert", {
      entity: symbol("sibling"),
      component: 324,
      fields: [encodedWrites[4]],
      adopt: true,
    }),
    command("set", {
      entity: handle(41n),
      component: 322,
      field: encodedWrites[4],
    }),
    command("set-field-if", {
      entity: symbol("sibling"),
      component: 325,
      field: encodedWrites[4],
      expected: value("string", "current"),
    }),
    command("remove", { entity: alias(31), component: 323 }),
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
  );
  const request = {
    session: 7n,
    requestId: 17n,
    body: { kind: "submitBatch", batchId: 27, last: true, operations },
  };
  const expected = layout("request-submit-batch", {
    session: 7n,
    request_id: 17n,
    tag: tag("REQUEST_SUBMIT_BATCH"),
    batch_id: 27,
    last: true,
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
      max_depth: 0,
    }).bytes,
  );
  const aliasHandle = (aliasValue, id) =>
    layout("alias-handle", { alias: aliasValue, handle: id });
  const symbolHandle = (symbol, id) =>
    layout("symbol-handle", { symbol, handle: id });
  const success = layout("outcome-success", {
    batch_id: 27n,
    tick: 37n,
    tag: tag("OUTCOME_SUCCESS"),
    aliases: [aliasHandle(1, 0x100000001n)],
    symbols: [
      symbolHandle("oracle", 0x100000001n),
      symbolHandle("sibling", 0x100000002n),
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
    effects: [],
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
        symbols: [
          { symbol: "oracle", id: 0x100000001n },
          { symbol: "sibling", id: 0x100000002n },
        ],
        effects: [],
      },
    },
  });
  const worldReference = (id, incarnation) =>
    layout("world-reference", { id, incarnation });
  const attachmentEffects = [];
  const encodedAttachmentEffects = [];
  for (const [kind, variant, child] of [
    ["written", "ATTACHMENT_WRITTEN", { id: 8n, incarnation: 3n }],
    ["detached", "ATTACHMENT_DETACHED", null],
    ["superseded", "ATTACHMENT_SUPERSEDED", { id: 9n, incarnation: 4n }],
  ]) {
    assert.equal(manifest.WIRE_TAG_LAYOUTS[variant].space, 34);
    attachmentEffects.push({
      operation: 3,
      kind,
      receipt: {
        id: 71n,
        parent: { id: 7n, incarnation: 2n },
        anchor: 41n,
        incarnation: 5n,
        revision: 6n,
        child,
      },
    });
    encodedAttachmentEffects.push(
      layout("applied-operation-effect", {
        operation: 3,
        effect: layout("attachment-effect", {
          tag: tag(variant),
          receipt: 71n,
          parent: worldReference(7n, 2n),
          anchor: 41n,
          incarnation: 5n,
          revision: 6n,
          child: child && worldReference(child.id, child.incarnation),
        }),
      }),
    );
  }
  // An adopting operation that found its target reports adoption in core order.
  assert.equal(manifest.WIRE_TAG_LAYOUTS.OPERATION_ADOPTED.space, 34);
  attachmentEffects.push({ operation: 4, kind: "adopted" });
  encodedAttachmentEffects.push(
    layout("applied-operation-effect", {
      operation: 4,
      effect: layout("operation-effect-adopted", {
        tag: tag("OPERATION_ADOPTED"),
      }),
    }),
  );
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-batch", {
        session: 7n,
        request_id: 17n,
        tick: 37n,
        tag: tag("RESPONSE_BATCH"),
        outcome: success,
        effects: encodedAttachmentEffects,
      }).bytes,
      7n,
    ).body.outcome.effects,
    attachmentEffects,
  );
  const failure = layout("outcome-failure", {
    batch_id: 28n,
    tick: 38n,
    tag: tag("OUTCOME_FAILURE"),
    scope: tag("BATCH_ERROR_OPERATION"),
    operation: 7,
    reason: "InvalidField",
    aliases: [layout("alias-handle", { alias: 9, handle: 17n })],
    symbols: [symbolHandle("failed", 18n)],
  });
  for (const [encodedEffects, effects] of [
    [[], []],
    [encodedAttachmentEffects, attachmentEffects],
  ]) {
    assert.deepEqual(
      codec.decodeResponse(
        layout("response-batch", {
          session: 7n,
          request_id: 18n,
          tick: 38n,
          tag: tag("RESPONSE_BATCH"),
          outcome: failure,
          effects: encodedEffects,
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
            symbols: [{ symbol: "failed", id: 18n }],
            effects,
          },
        },
      },
    );
  }
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
    parent: 0n,
    order_low: 0n,
    order_high: 0n,
    components: [scalar, linearDriver, meshInstance],
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
  assert.equal(inspected.body.entities[0].components[0].fields.value, 2.5);
  assert.equal(
    inspected.body.entities[0].components[1].fields.source,
    0x100000002n,
  );
  assert.equal(inspected.body.entities[0].components[1].fields.scale, 1.5);
  assert.equal(inspected.body.entities[0].components[1].fields.bias, -0.25);
  assert.equal(
    inspected.body.entities[0].components[2].fields.source,
    "https://example.test/mesh",
  );
  assert.equal(
    inspected.body.entities[0].components[2].fields.variant,
    0xa1b2c3d4,
  );
  assert.equal(inspected.body.renderDiagnostics[0].entity, 0x100000001n);
  assert.equal(inspected.body.renderDiagnostics[0].reason, "InvalidAsset");

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

  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 0n,
        body: {
          kind: "command",
          command: { type: "CameraActivateCommand", entity: 41n },
        },
      }),
    /unsupported command/,
  );
  for (const name of [
    "request-camera-activate",
    "response-camera-state-changed",
    "camera-state-patch",
    "camera-entity",
    "camera-motion-rotate",
    "camera-motion-pan",
    "camera-motion-zoom",
  ])
    assert.equal(name in manifest.WIRE_LAYOUTS, false, name);
  for (const name of [
    "REQUEST_CAMERA_ACTIVATE",
    "RESPONSE_CAMERA_STATE_CHANGED",
    "CAMERA_MOTION_ROTATE",
    "CAMERA_MOTION_PAN",
    "CAMERA_MOTION_ZOOM",
  ])
    assert.equal(name in codec.WIRE, false, name);
  const output = {
    world: { id: 7n, incarnation: 2n },
    entity: 41n,
    kind: "camera",
    incarnation: 5n,
  };
  const encodedOutput = layout("output-reference", {
    world: worldReference(7n, 2n),
    target: layout("output-target-camera", {
      tag: tag("OUTPUT_TARGET_CAMERA"),
      entity: 41n,
      incarnation: 5n,
    }),
  });
  const viewport = { width: 640, height: 480, devicePixelRatio: 1 };
  const encodedViewport = layout("view-viewport", {
    width: 640,
    height: 480,
    device_pixel_ratio: 1,
  });
  const publication = { host: 9n, revision: 11n };
  const encodedPublication = layout("publication-reference", publication);
  const view = { output, publication, viewport };
  const encodedView = layout("view-descriptor", {
    output: encodedOutput,
    publication: encodedPublication,
    viewport: encodedViewport,
  });
  const rootView = { kind: "root", output, expectedViewport: viewport };
  const encodedRootView = layout("view-root", {
    tag: tag("VIEW_ROOT"),
    output: encodedOutput,
    expected_viewport: encodedViewport,
  });
  for (const [queryView, encodedQueryView] of [
    [rootView, encodedRootView],
    [
      { kind: "publication", ...view },
      layout("view-publication", {
        tag: tag("VIEW_PUBLICATION"),
        output: encodedOutput,
        publication: encodedPublication,
        viewport: encodedViewport,
      }),
    ],
  ]) {
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 25n,
        body: {
          kind: "query",
          query: {
            type: "GeometryPickQuery",
            view: queryView,
            x: 0.25,
            y: 0.75,
          },
        },
      }),
      layout("request-geometry-pick", {
        session: 7n,
        request_id: 25n,
        tag: tag("REQUEST_GEOMETRY_PICK"),
        view: encodedQueryView,
        x: 0.25,
        y: 0.75,
        include_view_plane: false,
      }).bytes,
    );
  }
  for (const motion of [
    { kind: "rotate", yaw: 0.25, pitch: -0.5 },
    { kind: "pan", x: 0.25, y: -0.5, width: 640, height: 480 },
    { kind: "zoom", amount: 0.25 },
  ]) {
    assert.throws(
      () =>
        codec.encodeRequest({
          session: 7n,
          requestId: 0n,
          body: {
            kind: "command",
            command: { type: "CameraNavigateCommand", motion },
          },
        }),
      /unsupported command/,
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
          view: rootView,
          x: 1.25,
          y: -0.5,
          plane: { point: [1, 2, 3], normal: [0, 0, -1] },
        },
      },
    }),
    layout("request-camera-project", {
      session: 7n,
      request_id: 29n,
      tag: tag("REQUEST_CAMERA_PROJECT"),
      view: encodedRootView,
      x: 1.25,
      y: -0.5,
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
      view: ok ? encodedView : null,
      ok,
      position: position && layout("world-point", { x: 1, y: 2, z: 3 }),
      error,
    }).bytes;
    assert.deepEqual(codec.decodeResponse(bytes, 7n).body.event, {
      type: "CameraProjectResultEvent",
      ok,
      ...(ok ? { view, position } : { error }),
    });
    assert.throws(() => codec.decodeResponse(bytes.slice(0, -1), 7n));
  }
  const hitFields = {
    view: encodedView,
    world: worldReference(7n, 2n),
    publication: encodedPublication,
    entity: 41n,
    incarnation: 5n,
    position_x: 1,
    position_y: 2,
    position_z: 3,
    distance: 4,
    part: 0,
    path: [
      layout("view-path-entry", { world: worldReference(8n, 3n), anchor: 42n }),
    ],
    view_plane: null,
  };
  const hit = {
    world: { id: 7n, incarnation: 2n },
    publication,
    entity: 41n,
    incarnation: 5n,
    position: [1, 2, 3],
    distance: 4,
    path: [{ world: { id: 8n, incarnation: 3n }, anchor: 42n }],
  };
  for (const [result, expected] of [
    [
      layout("pick-result-miss", {
        tag: tag("PICK_OUTCOME_MISS"),
        view: encodedView,
      }),
      { ok: true, view, hit: null },
    ],
    [
      layout("pick-result-hit", { tag: tag("PICK_OUTCOME_HIT"), ...hitFields }),
      { ok: true, view, hit: { ...hit, part: 0 } },
    ],
    [
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
        view,
        hit: {
          ...hit,
          part: 8,
          viewPlane: { point: [1, 2, 3], normal: [0, 0, -1] },
        },
      },
    ],
    [
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
          result,
        }).bytes,
        7n,
      ).body,
      {
        kind: "event",
        event: { type: "GeometryPickResultEvent", ...expected },
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
          parent: 0n,
          order_low: 0n,
          order_high: 0n,
          components: [debugComponent],
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes,
    7n,
  );
  assert.equal(
    debugInspection.body.entities[0].components[0].fields.is_rendered,
    false,
  );
  assert.equal(
    debugInspection.body.entities[0].components[0].fields.outline,
    false,
  );

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
          parent: 0n,
          order_low: 0n,
          order_high: 0n,
          components: [customSnapshot],
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes,
    7n,
  );
  assert.deepEqual(
    { ...dynamicInspection.body.entities[0].components[0].properties },
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
  const denseInspection = (components) =>
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
          parent: 0n,
          order_low: 0n,
          order_high: 0n,
          components,
        }),
      ],
      resources: [],
      controllers: [],
      render_diagnostics: [],
    }).bytes;
  // Each dynamic component carries its own descriptor table.
  const denseBytes = denseInspection([
    denseSnapshot(snapshotValue("bytes", denseTable), 0.5),
  ]);
  assert.ok(denseBytes.length < codec.MAX_MESSAGE_BYTES);
  const dense = codec.decodeResponse(denseBytes, 7n).body.entities[0];
  assert.equal(
    Object.keys(dense.components[0].properties).length,
    denseNames.length,
  );
  assert.deepEqual(dense.components[0].properties[denseNames.at(-1)], {
    kind: "f32",
    value: denseNames.length - 1 + 0.5,
  });

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
              parent: 0n,
              order_low: 0n,
              order_high: 0n,
              components: [oversizedSnapshot],
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
        max_depth: 0,
      }).bytes,
    );
  }
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 21n,
      body: {
        kind: "inspectTree",
        root: 40n,
        after: 41n,
        limit: 1,
        maxDepth: 2,
      },
    }),
    layout("request-inspect", {
      session: 7n,
      request_id: 21n,
      tag: tag("REQUEST_INSPECT"),
      collection: tag("INSPECT_ENTITY_TREE"),
      after: 41n,
      target: 40n,
      limit: 1,
      max_depth: 2,
    }).bytes,
  );
  const tree = codec.decodeResponse(
    layout("response-entity-tree", {
      session: 7n,
      request_id: 21n,
      tick: 40n,
      tag: tag("RESPONSE_ENTITY_TREE"),
      time: 2.25,
      next: 41n,
      nodes: [
        layout("entity-tree-node", {
          id: 41n,
          parent: 40n,
          order_low: 17n,
          order_high: 1n << 16n,
          depth: 1,
        }),
      ],
    }).bytes,
    7n,
  ).body;
  assert.deepEqual(tree, {
    kind: "entityTree",
    time: 2.25,
    next: 41n,
    nodes: [{ id: 41n, parent: 40n, order: (1n << 80n) | 17n, depth: 1 }],
  });
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
    symbols: [],
  });
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-batch", {
        session: 7n,
        request_id: 18n,
        tick: 38n,
        tag: tag("RESPONSE_BATCH"),
        outcome: commit,
        effects: [],
      }).bytes,
      7n,
    ).body.outcome,
    {
      batchId: 28n,
      tick: 38n,
      ok: false,
      error: {
        scope: "commit",
        operation: null,
        reason: "NonConvergentCommit",
      },
      aliases: [],
      symbols: [],
      effects: [],
    },
  );

  const declaredTag = (name, space, spaceId) => {
    assert.equal(manifest.WIRE_TAG_LAYOUTS[name].space, spaceId, name);
    return { space, value: tag(name).value };
  };
  const declaredUnion = (name, space, spaceId, fields = {}) => {
    const variant = declaredTag(name, space, spaceId);
    const layoutName = manifest.WIRE_TAG_LAYOUTS[name].layout;
    const firstField = manifest.WIRE_LAYOUTS[layoutName].fields[0];
    assert.equal(firstField.name, "tag");
    assert.equal(firstField.encoding, "variant");
    assert.equal(firstField.target, space);
    return {
      layout: space,
      bytes: layout(layoutName, { ...fields, tag: variant }).bytes,
    };
  };
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 31n,
      body: {
        kind: "submitBatch",
        batchId: 29,
        last: true,
        operations: [{ kind: "detachWorldAttachment", receipt: 71n }],
      },
    }),
    layout("request-submit-batch", {
      session: 7n,
      request_id: 31n,
      tag: tag("REQUEST_SUBMIT_BATCH"),
      batch_id: 29,
      last: true,
      operations: [command("detach-attachment-receipt", { receipt: 71n })],
    }).bytes,
  );
  for (const release of [false, true]) {
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 32n,
        body: { kind: "attachmentReceipt", receipt: 71n, release },
      }),
      layout("request-attachment-receipt", {
        session: 7n,
        request_id: 32n,
        tag: tag("REQUEST_ATTACHMENT_RECEIPT"),
        receipt: 71n,
        release,
      }).bytes,
    );
  }
  for (const state of ["pending", "retired", "released"]) {
    const bytes = layout("response-attachment-receipt", {
      session: 7n,
      request_id: 32n,
      tick: 48n,
      tag: tag("RESPONSE_ATTACHMENT_RECEIPT"),
      receipt: 71n,
      state: declaredTag(
        `RECEIPT_${state.toUpperCase()}`,
        "attachment-receipt-state",
        35,
      ),
    }).bytes;
    assert.deepEqual(codec.decodeResponse(bytes, 7n), {
      session: 7n,
      requestId: 32n,
      tick: 48n,
      body: { kind: "attachmentReceipt", receipt: 71n, state },
    });
    assert.throws(() => codec.decodeResponse(bytes.subarray(0, -1), 7n));
    const invalidState = bytes.slice();
    invalidState[invalidState.length - 1] = 255;
    assert.throws(
      () => codec.decodeResponse(invalidState, 7n),
      /invalid attachment receipt state/,
    );
  }

  const watchWorld = { id: 3n, incarnation: 5n };
  const watchOutput = 9n;
  const watchRequest = (control) =>
    codec.encodeRequest({
      session: 7n,
      requestId: 2n,
      body: {
        kind: "lifecycleWatch",
        control: { world: watchWorld, ...control },
      },
    });
  const watchTarget = (kind, entity = 42n, component = 1) =>
    declaredUnion(
      `LIFECYCLE_WATCH_${kind.toUpperCase()}`,
      "lifecycle-watch-target",
      40,
      {
        entity,
        ...(kind === "component" ? { component } : {}),
      },
    );
  for (const [kinds, name, kind] of [
    [1, "ENTITY_CREATED", "entity"],
    [2, "ENTITY_METADATA_CHANGED", "entity"],
    [4, "ENTITY_DELETED", "entity"],
    [8, "COMPONENT_INSERTED", "component"],
    [16, "COMPONENT_UPDATED", "component"],
    [32, "COMPONENT_REPLACED", "component"],
    [64, "COMPONENT_REMOVED", "component"],
  ]) {
    const target = {
      kind,
      entity: 42n,
      ...(kind === "component" ? { component: 1 } : {}),
    };
    const kindsTag = declaredTag(
      `LIFECYCLE_WATCH_${name}`,
      "lifecycle-watch-kinds",
      45,
    );
    assert.equal(kindsTag.value, kinds);
    assert.deepEqual(
      watchRequest({ kind: "add", targets: [{ target, kinds }] }),
      layout("request-lifecycle-watch", {
        session: 7n,
        request_id: 2n,
        tag: tag("REQUEST_LIFECYCLE_WATCH"),
        world: worldReference(3n, 5n),
        change: declaredUnion(
          "LIFECYCLE_WATCH_ADD",
          "lifecycle-watch-change",
          39,
          {
            members: [
              layout("lifecycle-watch-selection", {
                target: watchTarget(kind),
                kinds: kindsTag,
              }),
            ],
          },
        ),
      }).bytes,
    );
  }
  assert.deepEqual(
    watchRequest({
      kind: "remove",
      output: watchOutput,
      generations: [2n, 6n],
    }),
    layout("request-lifecycle-watch", {
      session: 7n,
      request_id: 2n,
      tag: tag("REQUEST_LIFECYCLE_WATCH"),
      world: worldReference(3n, 5n),
      change: declaredUnion(
        "LIFECYCLE_WATCH_REMOVE",
        "lifecycle-watch-change",
        39,
        {
          output: watchOutput,
          generations: [2n, 6n],
        },
      ),
    }).bytes,
  );
  for (const generations of [[], [0n], [2n, 2n], [6n, 2n]])
    assert.throws(() =>
      watchRequest({ kind: "remove", output: watchOutput, generations }),
    );
  assert.throws(
    () => watchRequest({ kind: "remove", output: 0n, generations: [2n] }),
    /lifecycle remove page/,
  );
  assert.throws(
    () => watchRequest({ kind: "add", targets: [] }),
    /empty membership page/,
  );
  for (const target of [
    { kind: "entity", entity: 42n },
    { kind: "component", entity: 42n, component: 1 },
  ]) {
    for (const kinds of [0, 128, 1.5, target.kind === "entity" ? 8 : 1])
      assert.throws(() =>
        watchRequest({ kind: "add", targets: [{ target, kinds }] }),
      );
  }
  for (const target of [
    { kind: "entity", entity: 0n },
    { kind: "component", entity: 0n, component: 1 },
    { kind: "component", entity: 42n, component: 0 },
  ])
    assert.throws(() =>
      watchRequest({
        kind: "add",
        targets: [{ target, kinds: target.kind === "entity" ? 4 : 32 }],
      }),
    );

  const watchResponse = (record, requestId = 2n, envelope = {}) =>
    layout("response-lifecycle-watch", {
      session: 7n,
      request_id: requestId,
      tick: 0n,
      tag: tag("RESPONSE_LIFECYCLE_WATCH"),
      world: worldReference(3n, 5n),
      output: watchOutput,
      record,
      ...envelope,
    }).bytes;
  const watchCut = layout("lifecycle-watch-cut", { sequence: 6n, tick: 8n });
  const watchAck = (action, result, cut = watchCut) =>
    declaredUnion("LIFECYCLE_WATCH_ACK", "lifecycle-watch-record", 41, {
      action: declaredTag(
        `LIFECYCLE_WATCH_${action.toUpperCase()}`,
        "lifecycle-watch-change",
        39,
      ),
      cut,
      result,
    });
  const watchApplied = (baselines) =>
    declaredUnion(
      "LIFECYCLE_MEMBERSHIP_APPLIED",
      "lifecycle-membership-result",
      42,
      {
        baselines,
      },
    );
  const entityLifetime = (live) =>
    declaredUnion(
      "LIFECYCLE_LIFETIME_ENTITY",
      "lifecycle-target-lifetime",
      43,
      {
        live,
      },
    );
  const componentLifetime = (entityLive, incarnation) =>
    declaredUnion(
      "LIFECYCLE_LIFETIME_COMPONENT",
      "lifecycle-target-lifetime",
      43,
      {
        entity_live: entityLive,
        incarnation,
      },
    );
  const removedLifetime = declaredUnion(
    "LIFECYCLE_LIFETIME_REMOVED",
    "lifecycle-target-lifetime",
    43,
  );
  const baseline = (generation, target, lifetime) =>
    layout("lifecycle-watch-baseline", { generation, target, lifetime });
  const baselineCases = [
    ["entity", entityLifetime(true), { kind: "entity", live: true }],
    ["entity", entityLifetime(false), { kind: "entity", live: false }],
    [
      "component",
      componentLifetime(true, 12n),
      { kind: "component", entityLive: true, incarnation: 12n },
    ],
    [
      "component",
      componentLifetime(true, 0n),
      { kind: "component", entityLive: true, incarnation: null },
    ],
    [
      "component",
      componentLifetime(false, 0n),
      { kind: "component", entityLive: false, incarnation: null },
    ],
  ];
  for (const action of ["add", "remove"]) {
    const encodedBaselines = baselineCases.map(([kind, lifetime], index) =>
      baseline(
        BigInt(index + 1),
        watchTarget(kind),
        action === "add" ? lifetime : removedLifetime,
      ),
    );
    const bytes = watchResponse(
      watchAck(action, watchApplied(encodedBaselines)),
    );
    assert.deepEqual(codec.decodeResponse(bytes, 7n), {
      session: 7n,
      requestId: 2n,
      tick: 0n,
      body: {
        kind: "lifecycleWatch",
        record: {
          world: watchWorld,
          output: watchOutput,
          kind: "ack",
          action,
          cut: { sequence: 6n, tick: 8n },
          result: {
            kind: "applied",
            baselines: baselineCases.map(([kind, , lifetime], index) => ({
              member: { output: watchOutput, generation: BigInt(index + 1) },
              target: {
                kind,
                entity: 42n,
                ...(kind === "component" ? { component: 1 } : {}),
              },
              lifetime: action === "add" ? lifetime : { kind: "removed" },
            })),
          },
        },
      },
    });
    for (let length = 0; length < bytes.length; length++)
      assert.throws(() => codec.decodeResponse(bytes.subarray(0, length), 7n));
    assert.throws(() => codec.decodeResponse(Uint8Array.of(...bytes, 0), 7n));
    assert.throws(() => codec.decodeResponse(bytes, 8n), /session mismatch/);
  }
  const watchRejected = (name) =>
    declaredUnion(
      "LIFECYCLE_MEMBERSHIP_REJECTED",
      "lifecycle-membership-result",
      42,
      {
        reason: declaredTag(
          `LIFECYCLE_MEMBERSHIP_${name}`,
          "lifecycle-membership-rejection",
          44,
        ),
      },
    );
  for (const [name, reason] of [
    ["STALE_WORLD", "StaleWorld"],
    ["STALE_SESSION", "StaleSession"],
    ["STALE_MEMBER", "StaleMember"],
    ["ALREADY_ACTIVE", "AlreadyActive"],
    ["TRACKING_ENDED", "TrackingEnded"],
    ["CAPACITY", "Capacity"],
  ]) {
    assert.deepEqual(
      codec.decodeResponse(
        watchResponse(watchAck("add", watchRejected(name))),
        7n,
      ).body.record,
      {
        world: watchWorld,
        output: watchOutput,
        kind: "ack",
        action: "add",
        cut: { sequence: 6n, tick: 8n },
        result: { kind: "rejected", reason },
      },
    );
  }
  const cancelled = declaredUnion(
    "LIFECYCLE_MEMBERSHIP_CANCELLED",
    "lifecycle-membership-result",
    42,
  );
  assert.deepEqual(
    codec.decodeResponse(watchResponse(watchAck("remove", cancelled, null)), 7n)
      .body.record,
    {
      world: watchWorld,
      output: watchOutput,
      kind: "ack",
      action: "remove",
      cut: null,
      result: { kind: "cancelled" },
    },
  );
  const watchEvent = (observation, generation = 4n, sequence = 7n) =>
    declaredUnion("LIFECYCLE_WATCH_EVENT", "lifecycle-watch-record", 41, {
      generation,
      sequence,
      tick: 8n,
      observation,
    });
  for (let index = 0; index < 7; index++) {
    assert.deepEqual(
      codec.decodeResponse(
        watchResponse(watchEvent(encodedObservations[index]), 0n),
        7n,
      ),
      {
        session: 7n,
        requestId: 0n,
        tick: 0n,
        body: {
          kind: "lifecycleWatch",
          record: {
            world: watchWorld,
            output: watchOutput,
            kind: "event",
            member: { output: watchOutput, generation: 4n },
            sequence: 7n,
            tick: 8n,
            observation: lifecycleObservations[index],
          },
        },
      },
    );
  }

  // Value targets select value changes over ascending schema offsets; the ACK
  // echoes them whole and value records carry snapshot fields or absence.
  const valueTarget = declaredUnion(
    "LIFECYCLE_WATCH_VALUE",
    "lifecycle-watch-target",
    40,
    { entity: 42n, component: 1, fields: [0, 12] },
  );
  const decodedValueTarget = {
    kind: "value",
    entity: 42n,
    component: 1,
    fields: [0, 12],
  };
  const valueChanged = declaredTag(
    "LIFECYCLE_WATCH_VALUE_CHANGED",
    "lifecycle-watch-kinds",
    45,
  );
  assert.equal(valueChanged.value, 128);
  assert.deepEqual(
    watchRequest({
      kind: "add",
      targets: [{ target: decodedValueTarget, kinds: 128 }],
    }),
    layout("request-lifecycle-watch", {
      session: 7n,
      request_id: 2n,
      tag: tag("REQUEST_LIFECYCLE_WATCH"),
      world: worldReference(3n, 5n),
      change: declaredUnion(
        "LIFECYCLE_WATCH_ADD",
        "lifecycle-watch-change",
        39,
        {
          members: [
            layout("lifecycle-watch-selection", {
              target: valueTarget,
              kinds: valueChanged,
            }),
          ],
        },
      ),
    }).bytes,
  );
  assert.deepEqual(
    codec.decodeResponse(
      watchResponse(
        watchAck(
          "add",
          watchApplied([
            baseline(4n, valueTarget, componentLifetime(true, 12n)),
          ]),
        ),
      ),
      7n,
    ).body.record.result.baselines,
    [
      {
        member: { output: watchOutput, generation: 4n },
        target: decodedValueTarget,
        lifetime: { kind: "component", entityLive: true, incarnation: 12n },
      },
    ],
  );
  const valueRecord = (values) =>
    declaredUnion(
      "LIFECYCLE_WATCH_VALUE_RECORD",
      "lifecycle-watch-record",
      41,
      { generation: 4n, tick: 8n, values },
    );
  for (const [values, decoded] of [
    [
      layout("lifecycle-watch-values", {
        fields: [
          snapshotField(0, snapshotValue("f32", 1.5)),
          snapshotField(12, snapshotValue("string", "é")),
        ],
      }),
      [
        { offset: 0, value: 1.5 },
        { offset: 12, value: "é" },
      ],
    ],
    [null, null],
  ])
    assert.deepEqual(
      codec.decodeResponse(watchResponse(valueRecord(values), 0n), 7n).body
        .record,
      {
        world: watchWorld,
        output: watchOutput,
        kind: "value",
        member: { output: watchOutput, generation: 4n },
        tick: 8n,
        values: decoded,
      },
    );
  const validBaseline = baseline(
    4n,
    watchTarget("entity"),
    entityLifetime(true),
  );
  const validAck = watchAck("add", watchApplied([validBaseline]));
  for (const [record, requestId, envelope] of [
    [validAck, 0n, {}],
    [validAck, 2n, { session: 0n }],
    [validAck, 2n, { output: 0n }],
    [validAck, 2n, { tick: 1n }],
    [watchEvent(encodedObservations[0]), 2n, {}],
    [watchEvent(encodedObservations[0], 0n), 0n, {}],
    [watchEvent(encodedObservations[0], 4n, 0n), 0n, {}],
    [watchAck("add", watchApplied([validBaseline]), null), 2n, {}],
    [watchAck("add", watchRejected("CAPACITY"), null), 2n, {}],
    [watchAck("remove", cancelled, watchCut), 2n, {}],
    [watchAck("add", watchApplied([])), 2n, {}],
    [watchAck("add", watchApplied([validBaseline, validBaseline])), 2n, {}],
    [
      watchAck(
        "add",
        watchApplied([
          validBaseline,
          baseline(2n, watchTarget("entity"), entityLifetime(true)),
        ]),
      ),
      2n,
      {},
    ],
    [watchAck("remove", watchApplied([validBaseline])), 2n, {}],
  ])
    assert.throws(() =>
      codec.decodeResponse(watchResponse(record, requestId, envelope), 7n),
    );
  for (const malformedBaseline of [
    baseline(0n, watchTarget("entity"), entityLifetime(true)),
    baseline(4n, watchTarget("entity", 0n), entityLifetime(true)),
    baseline(
      4n,
      watchTarget("component", 42n, 0),
      componentLifetime(true, 12n),
    ),
    baseline(4n, watchTarget("entity"), componentLifetime(true, 12n)),
    baseline(4n, watchTarget("component"), entityLifetime(true)),
    baseline(4n, watchTarget("component"), componentLifetime(false, 12n)),
    baseline(4n, watchTarget("entity"), removedLifetime),
  ])
    assert.throws(() =>
      codec.decodeResponse(
        watchResponse(watchAck("add", watchApplied([malformedBaseline]))),
        7n,
      ),
    );
  for (const observation of [
    layout("lifecycle-entity", {
      tag: tag("LIFECYCLE_ENTITY_CREATED"),
      entity: 0n,
    }),
    layout("lifecycle-component", {
      tag: tag("LIFECYCLE_COMPONENT_REPLACED"),
      entity: 42n,
      component: 0,
      previous_incarnation: 1n,
      incarnation: 2n,
    }),
    layout("lifecycle-component", {
      tag: tag("LIFECYCLE_COMPONENT_REPLACED"),
      entity: 42n,
      component: 1,
      previous_incarnation: 0n,
      incarnation: 2n,
    }),
    layout("lifecycle-component", {
      tag: tag("LIFECYCLE_COMPONENT_INSERTED"),
      entity: 42n,
      component: 1,
      previous_incarnation: 1n,
      incarnation: 2n,
    }),
    layout("lifecycle-component", {
      tag: tag("LIFECYCLE_COMPONENT_REMOVED"),
      entity: 42n,
      component: 1,
      previous_incarnation: 1n,
      incarnation: 2n,
    }),
  ])
    assert.throws(() =>
      codec.decodeResponse(watchResponse(watchEvent(observation), 0n), 7n),
    );
  for (const offset of [0, 1, 2]) {
    const bytes = validAck.bytes.slice();
    bytes[offset] = 255;
    assert.throws(() =>
      codec.decodeResponse(
        watchResponse({ layout: validAck.layout, bytes }),
        7n,
      ),
    );
  }
  const invalidRejection = watchRejected("CAPACITY");
  const invalidRejectionBytes = invalidRejection.bytes.slice();
  invalidRejectionBytes[invalidRejectionBytes.length - 1] = 255;
  assert.throws(
    () =>
      codec.decodeResponse(
        watchResponse(
          watchAck("add", {
            layout: invalidRejection.layout,
            bytes: invalidRejectionBytes,
          }),
        ),
        7n,
      ),
    /membership rejection/,
  );
  const invalidTarget = watchTarget("entity");
  const invalidTargetBytes = invalidTarget.bytes.slice();
  invalidTargetBytes[0] = 255;
  for (const malformedBaseline of [
    baseline(
      4n,
      { layout: invalidTarget.layout, bytes: invalidTargetBytes },
      entityLifetime(true),
    ),
    baseline(4n, watchTarget("entity"), {
      layout: removedLifetime.layout,
      bytes: Uint8Array.of(255),
    }),
  ])
    assert.throws(() =>
      codec.decodeResponse(
        watchResponse(watchAck("add", watchApplied([malformedBaseline]))),
        7n,
      ),
    );

  const canvasOutput = { world: { id: 8n, incarnation: 3n }, kind: "canvas" };
  const encodedCanvasOutput = layout("output-reference", {
    world: worldReference(8n, 3n),
    target: layout("output-target-canvas", {
      tag: tag("OUTPUT_TARGET_CANVAS"),
    }),
  });
  const attachment = codec.components.WorldAttachment;
  for (const [child, selectedOutput, encodedChild, encodedSelectedOutput] of [
    [null, null, null, null],
    [
      canvasOutput.world,
      canvasOutput,
      worldReference(8n, 3n),
      encodedCanvasOutput,
    ],
    [output.world, output, worldReference(7n, 2n), encodedOutput],
  ]) {
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 33n,
        body: {
          kind: "submitBatch",
          batchId: 30,
          last: true,
          operations: [
            codec.WorldAttachment.insert(refs.handle, {
              child,
              output: selectedOutput,
              mode: 0,
            }),
          ],
        },
      }),
      layout("request-submit-batch", {
        session: 7n,
        request_id: 33n,
        tag: tag("REQUEST_SUBMIT_BATCH"),
        batch_id: 30,
        last: true,
        operations: [
          command("insert", {
            entity: handle(41n),
            component: attachment.id,
            fields: [
              field(
                attachment.fields.child.offset,
                value("world", encodedChild),
              ),
              field(
                attachment.fields.output.offset,
                value("output", encodedSelectedOutput),
              ),
              field(attachment.fields.mode.offset, value("u32", 0)),
            ],
            adopt: false,
          }),
        ],
      }).bytes,
    );
    const inspectedAttachment = layout("component", {
      type_id: attachment.id,
      fields: [
        snapshotField(
          attachment.fields.child.offset,
          snapshotValue("world", encodedChild),
        ),
        snapshotField(
          attachment.fields.output.offset,
          snapshotValue("output", encodedSelectedOutput),
        ),
        snapshotField(attachment.fields.mode.offset, snapshotValue("u32", 0)),
      ],
    });
    const inspected = codec.decodeResponse(
      layout("response-inspect", {
        session: 7n,
        request_id: 34n,
        tick: 49n,
        tag: tag("RESPONSE_INSPECT"),
        next: 0n,
        time: 2.25,
        entities: [
          layout("entity", {
            id: 41n,
            metadata: metadata(null, []),
            parent: 0n,
            order_low: 0n,
            order_high: 0n,
            components: [inspectedAttachment],
          }),
        ],
        resources: [],
        controllers: [],
        render_diagnostics: [],
      }).bytes,
      7n,
    ).body.entities[0];
    for (const components of [inspected.components])
      assert.deepEqual(components, [
        {
          component: attachment.id,
          fields: Object.assign(Object.create(null), {
            child,
            output: selectedOutput,
            mode: 0,
          }),
        },
      ]);
  }

  const presentationSurface = {
    id: 11n,
    context: 12n,
    maxWidth: 64,
    maxHeight: 64,
  };
  const encodedSurface = layout("presentation-surface", {
    id: 11n,
    context: 12n,
    max_width: 64,
    max_height: 64,
  });
  const rootBinding = {
    output,
    viewport: { width: 2, height: 3, devicePixelRatio: 1 },
    generation: { host: 13n, serial: 14n },
  };
  const encodedBinding = layout("root-binding", {
    output: encodedOutput,
    width: 2,
    height: 3,
    device_pixel_ratio: 1,
    generation: layout("presentation-identity", { host: 13n, serial: 14n }),
  });
  for (const navigationPublication of [
    undefined,
    { host: 13n, revision: 17n },
  ]) {
    const selectedSource =
      navigationPublication === undefined
        ? {}
        : { publication: navigationPublication };
    const encodedSource =
      navigationPublication === undefined
        ? null
        : layout("publication-reference", navigationPublication);
    for (const [motion, kind, first, second] of [
      [{ kind: "rotate", yaw: 0.25, pitch: -0.5 }, 0, 0.25, -0.5],
      [{ kind: "pan", x: -0.25, y: 0.5 }, 1, -0.25, 0.5],
      [{ kind: "zoom", amount: 0.75 }, 2, 0.75, 0],
    ])
      assert.deepEqual(
        codec.encodeRequest({
          session: 7n,
          requestId: 37n,
          body: {
            kind: "cameraNavigate",
            request: { binding: rootBinding, ...selectedSource, motion },
          },
        }),
        layout("request-camera-navigate", {
          session: 7n,
          request_id: 37n,
          tag: tag("REQUEST_CAMERA_NAVIGATE"),
          binding: encodedBinding,
          publication: encodedSource,
          kind,
          first,
          second,
        }).bytes,
      );
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 38n,
        body: {
          kind: "query",
          query: {
            type: "GeometryPickQuery",
            view: { kind: "bound", binding: rootBinding, ...selectedSource },
            x: 0.25,
            y: 0.75,
            includeViewPlane: true,
          },
        },
      }),
      layout("request-geometry-pick", {
        session: 7n,
        request_id: 38n,
        tag: tag("REQUEST_GEOMETRY_PICK"),
        view: layout("view-bound", {
          tag: tag("VIEW_BOUND"),
          binding: encodedBinding,
          publication: encodedSource,
        }),
        x: 0.25,
        y: 0.75,
        include_view_plane: true,
      }).bytes,
    );
  }
  assert.deepEqual(
    codec.decodeResponse(
      layout("response-camera-navigated", {
        session: 7n,
        request_id: 37n,
        tick: 19n,
        tag: tag("RESPONSE_CAMERA_NAVIGATED"),
      }).bytes,
      7n,
    ),
    {
      session: 7n,
      requestId: 37n,
      tick: 19n,
      body: { kind: "cameraNavigated" },
    },
  );
  const presentationView = {
    surface: presentationSurface,
    selection: 15n,
    binding: rootBinding,
  };
  const encodedPresentationView = layout("presentation-view", {
    surface: encodedSurface,
    selection: 15n,
    binding: encodedBinding,
  });
  const presentedFrame = {
    view: presentationView,
    sequence: 16n,
    publication: { host: 13n, revision: 17n },
    drawCalls: 3,
    triangles: 7,
    failedDrawCalls: 1,
    sources: [],
  };
  const encodedPresentedFrame = layout("presented-frame", {
    view: encodedPresentationView,
    sequence: 16n,
    publication: layout("presentation-identity", { host: 13n, serial: 17n }),
    draw_calls: 3,
    triangles: 7,
    failed_draw_calls: 1,
    sources: [],
  });
  const presentationRequest = (name, fields = {}) =>
    declaredUnion(
      `PRESENTATION_REQUEST_${name}`,
      "presentation-request",
      36,
      fields,
    );
  const presentationResponse = (name, fields = {}) =>
    declaredUnion(
      `PRESENTATION_RESPONSE_${name}`,
      "presentation-response",
      37,
      fields,
    );
  const exchanges = [];
  let transportEvents;
  let bootstrapped = false;
  let nextRequest = 1n;
  let closes = 0;
  const host = await codec.IppHostClient.connectTransport({
    start(events) {
      transportEvents = events;
      events.ready();
    },
    send(bytes) {
      if (!bootstrapped) {
        assert.deepEqual(bytes, codec.bootstrap());
        const response = new Uint8Array(24);
        response.set(bytes);
        new DataView(response.buffer).setBigUint64(16, 7n, true);
        bootstrapped = true;
        transportEvents.message(response);
        return;
      }
      const exchange = exchanges.shift();
      assert.ok(exchange, "unexpected Host request");
      assert.deepEqual(
        bytes,
        layout("host-request-presentation", {
          magic: 0x0000000248505049n,
          connection: 7n,
          request_id: nextRequest,
          tag: manifestVariant(client, "HOST_REQUEST_PRESENTATION"),
          body: exchange.request,
        }).bytes,
      );
      transportEvents.message(
        layout("host-response-presentation", {
          magic: 0x0000000241505049n,
          connection: 7n,
          request_id: nextRequest++,
          tag: manifestVariant(client, "HOST_RESPONSE_PRESENTATION"),
          body: exchange.response,
        }).bytes,
      );
    },
    async close() {
      closes++;
    },
  });
  try {
    exchanges.push({
      request: presentationRequest("SURFACE"),
      response: presentationResponse("SURFACE", { surface: encodedSurface }),
    });
    assert.deepEqual(await host.presentation.surface(), presentationSurface);
    exchanges.push({
      request: presentationRequest("SELECT", {
        surface: encodedSurface,
        binding: encodedBinding,
      }),
      response: presentationResponse("VIEW", { view: encodedPresentationView }),
    });
    assert.deepEqual(
      await host.presentation.select(presentationSurface, rootBinding),
      presentationView,
    );
    for (const options of [
      {},
      { afterSequence: 15n, publication: presentedFrame.publication },
    ]) {
      exchanges.push({
        request: presentationRequest("FRAME", {
          view: encodedPresentationView,
          after_sequence: options.afterSequence ?? null,
          publication: options.publication
            ? layout("presentation-identity", { host: 13n, serial: 17n })
            : null,
          capture: false,
          after_outputs: [],
        }),
        response: presentationResponse("FRAME", {
          frame: encodedPresentedFrame,
        }),
      });
      assert.deepEqual(
        await host.presentation.frame(presentationView, options),
        presentedFrame,
      );
    }
    for (const stale of [false, true]) {
      const source = {
        output: stale
          ? { ...output, incarnation: output.incarnation + 1n }
          : output,
        minimumTick: 21n,
        publication: { host: 13n, revision: 20n },
        tick: 22n,
      };
      const frame = layout("presented-frame", {
        view: encodedPresentationView,
        sequence: 16n,
        publication: layout("presentation-identity", {
          host: 13n,
          serial: 17n,
        }),
        draw_calls: 3,
        triangles: 7,
        failed_draw_calls: 0,
        sources: [
          layout("presented-source", {
            output: layout("output-reference", {
              world: layout("world-reference", source.output.world),
              target: layout("output-target-camera", {
                tag: tag("OUTPUT_TARGET_CAMERA"),
                entity: source.output.entity,
                incarnation: source.output.incarnation,
              }),
            }),
            minimum_tick: 21n,
            publication: layout("presentation-identity", {
              host: 13n,
              serial: 20n,
            }),
            tick: 22n,
          }),
        ],
      });
      exchanges.push({
        request: presentationRequest("FRAME", {
          view: encodedPresentationView,
          after_sequence: null,
          publication: null,
          capture: false,
          after_outputs: [encodedOutput],
        }),
        response: presentationResponse("FRAME", { frame }),
      });
      const pending = host.presentation.frame(presentationView, {
        afterOutputs: [output, output],
      });
      if (stale) await assert.rejects(pending, /requested output cut/);
      else
        assert.deepEqual(await pending, {
          ...presentedFrame,
          failedDrawCalls: 0,
          sources: [source],
        });
    }
    exchanges.push({
      request: presentationRequest("CLEAR", { view: encodedPresentationView }),
      response: presentationResponse("COMPLETE"),
    });
    assert.equal(await host.presentation.clear(presentationView), undefined);
    const pixels = Uint8Array.from({ length: 24 }, (_, index) => index);
    exchanges.push(
      {
        request: presentationRequest("FRAME", {
          view: encodedPresentationView,
          after_sequence: 15n,
          publication: layout("presentation-identity", {
            host: 13n,
            serial: 17n,
          }),
          capture: true,
          after_outputs: [],
        }),
        response: presentationResponse("CAPTURE", {
          frame: encodedPresentedFrame,
          capture: 18n,
          bytes: 24n,
        }),
      },
      {
        request: presentationRequest("READ_CAPTURE", {
          capture: 18n,
          offset: 0n,
        }),
        response: presentationResponse("CHUNK", {
          capture: 18n,
          offset: 0n,
          bytes: pixels,
        }),
      },
      {
        request: presentationRequest("RELEASE_CAPTURE", { capture: 18n }),
        response: presentationResponse("COMPLETE"),
      },
    );
    assert.deepEqual(
      await host.presentation.capture(presentationView, {
        afterSequence: 15n,
        publication: presentedFrame.publication,
      }),
      { ...presentedFrame, pixels: pixels.buffer },
    );
    for (const [name, reason] of [
      ["UNSUPPORTED", "unsupported"],
      ["UNAVAILABLE", "unavailable"],
      ["STALE_VIEW", "staleView"],
      ["INVALID_VIEWPORT", "invalidViewport"],
      ["OBSOLETE_PUBLICATION", "obsoletePublication"],
      ["CAPACITY", "capacity"],
      ["TIMEOUT", "timeout"],
      ["DRAW_FAILED", "drawFailed"],
    ]) {
      exchanges.push({
        request: presentationRequest("SURFACE"),
        response: presentationResponse("ERROR", {
          error: declaredUnion(
            `PRESENTATION_ERROR_${name}`,
            "presentation-error",
            38,
          ),
        }),
      });
      await assert.rejects(host.presentation.surface(), (error) => {
        assert.ok(error instanceof codec.PresentationError);
        assert.equal(error.reason, reason);
        return true;
      });
    }
    exchanges.push({
      request: presentationRequest("SURFACE"),
      response: presentationResponse("ERROR", {
        error: { layout: "presentation-error", bytes: Uint8Array.of(255) },
      }),
    });
    await assert.rejects(
      host.presentation.surface(),
      /Invalid presentation failure/,
    );
    const complete = presentationResponse("COMPLETE");
    exchanges.push({
      request: presentationRequest("CLEAR", { view: encodedPresentationView }),
      response: {
        layout: complete.layout,
        bytes: Uint8Array.of(...complete.bytes, 0),
      },
    });
    await assert.rejects(
      host.presentation.clear(presentationView),
      /trailing/i,
    );
    assert.equal(exchanges.length, 0);
    assert.equal(nextRequest, 21n);
  } finally {
    await host.close();
  }
  assert.equal(closes, 1);

  executeCancellationConformance(client);
  covered.add("PRESENTATION_REQUEST_CANCEL_FRAME");

  // Playback/controller branches are covered against this same manifest in animation-client.mjs.
  const animationTag = (name) =>
    manifest.WIRE_TAG_LAYOUTS[name].capability === "animation";
  const unreachableSnapshotKinds = new Set([
    "SNAPSHOT_VALUE_U64",
    "SNAPSHOT_VALUE_ROWS",
    "SNAPSHOT_VALUE_UNSET",
  ]);
  // No component in this compiled target exposes resolved u64 or rows fields yet, and
  // absence is never a field kind. Their authored encoders and Rust resolved writer
  // remain covered (inspected rows tables by the synthetic rows contract below); a
  // registered field makes this set drift.
  const compiledKinds = new Set(
    Object.values(codec.components).flatMap((component) =>
      Object.values(component.fields).map((field) => field.kind),
    ),
  );
  for (const name of unreachableSnapshotKinds)
    assert.equal(compiledKinds.has(codec.WIRE[name]), false, name);
  assert.deepEqual(
    [...covered].sort(),
    Object.keys(manifest.WIRE_TAG_LAYOUTS)
      .filter(
        (name) =>
          !unreachableSnapshotKinds.has(name) &&
          !animationTag(name) &&
          // Physical Host controls/selectors are exercised by the maintained
          // native and worker Host lifecycle/persistence suites, separately
          // from this World-envelope fixture. Output kinds (space 32) now
          // appear only in the Host bind-output request.
          ![23, 24, 25, 32].includes(manifest.WIRE_TAG_LAYOUTS[name].space),
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
