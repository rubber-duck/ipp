/** Local source equality/diff invariants; actual provider rendering lives in tests/render. */
import assert from "node:assert/strict";
import test from "node:test";
import { BoundingGeometry } from "../src/components.js";
import { ReactWorldTree } from "../src/tree.js";
import { ReactWorldCommits } from "../src/commits.js";
import type { ReactWorldClient } from "../src/contract.js";
import type { BatchOutcome, Command, EntityRef, FieldValue } from "@ipp/client";

/** MeshInstance fields as the native contract lays them out. */
const meshFields = {
  source: { offset: 0, kind: 5 },
  variant: { offset: 16, kind: 3 },
} as const;

test("resource strings write their contract offsets, compare by value and keep removed values", async () => {
  const { client, calls } = recordingClient({
    Scalar: { id: 1, fields: { value: { offset: 0, kind: 1 } } },
    MeshInstance: { id: 5, fields: meshFields },
  });
  const tree = new ReactWorldTree(client);
  const entity = tree.instance("ipp-entity", {
    id: "subject",
  });
  const mesh = tree.instance("ipp-mesh-instance", {
    source: "https://example.test/é.ippm",
  });
  entity.children.push(mesh);
  tree.children.push(entity);
  const commits = new ReactWorldCommits(client, {});
  await commits.capture(tree.describe());
  assert.deepEqual(calls[0], [
    {
      kind: "create",
      alias: 1,
      metadata: { symbolicId: "subject", classes: [] },
      adopt: true,
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: 5,
      fields: [
        {
          offset: meshFields.source.offset,
          value: { kind: "string", value: "https://example.test/é.ippm" },
        },
      ],
      adopt: true,
    },
  ]);
  mesh.props = {
    ...mesh.props,
    source: ["https://example.test/", "é.ippm"].join(""),
  };
  await commits.capture(tree.describe());
  assert.equal(calls.length, 1, "equal source strings do not write again");
  const handle: EntityRef = { kind: "handle", id: 101n };
  const written = (source: string): Command[] => [
    {
      kind: "setField",
      entity: handle,
      component: 5,
      field: {
        offset: meshFields.source.offset,
        value: { kind: "string", value: source },
      },
    },
  ];
  for (const source of ["ipp://mesh/cube?width=1&height=1&length=1", ""]) {
    mesh.props = { ...mesh.props, source };
    await commits.capture(tree.describe());
    assert.deepEqual(calls.at(-1), written(source));
  }
  mesh.props = {};
  await commits.capture(tree.describe());
  assert.equal(calls.length, 3, "a removed prop leaves its last value");
  mesh.props = { source: "" };
  await commits.capture(tree.describe());
  assert.deepEqual(calls.at(-1), written(""), "declaring it again writes it");
  assert.throws(
    () => tree.validate("ipp-mesh-instance", { source: 9n }),
    /must be a string/,
  );
  assert.throws(
    () => tree.validate("ipp-mesh-instance", { asset: 9n }),
    /Unsupported.*asset/,
  );
  const beforeUnmount = calls.length;
  await commits.dispose();
  assert.equal(calls.length, beforeUnmount, "unmount deletes nothing");
});

/**
 * Records batches and answers them like a World: creations get handles
 * 100 + alias, each symbol resolves to its entry in `symbols`, and an adopting
 * insertion reports adoption when `existing` holds `symbol:component`.
 */
function recordingClient(
  components: ReactWorldClient["components"],
  symbols: Readonly<Record<string, bigint>> = {},
  existing: ReadonlySet<string> = new Set(),
) {
  const calls: Command[][] = [];
  const client: ReactWorldClient = {
    session: 1n,
    schemaHash: 1n,
    components,
    async batch(operations) {
      calls.push(operations);
      const outcome: BatchOutcome = {
        ok: true,
        batchId: BigInt(calls.length),
        tick: BigInt(calls.length),
        aliases: [],
        symbols: [],
        effects: [],
      };
      const resolve = (reference: EntityRef) => {
        if (reference.kind !== "symbol") return;
        const id = symbols[reference.symbol];
        if (id === undefined) throw new Error(`No ${reference.symbol}`);
        if (!outcome.symbols.some((entry) => entry.symbol === reference.symbol))
          outcome.symbols.push({ symbol: reference.symbol, id });
      };
      operations.forEach((operation, index) => {
        if (operation.kind === "create")
          outcome.aliases.push({
            alias: operation.alias,
            id: 100n + BigInt(operation.alias),
          });
        if ("entity" in operation) resolve(operation.entity);
        if (
          operation.kind === "insertComponent" &&
          operation.adopt &&
          operation.entity.kind === "symbol" &&
          existing.has(`${operation.entity.symbol}:${operation.component}`)
        )
          outcome.effects.push({ operation: index, kind: "adopted" });
      });
      return outcome;
    },
  };
  return { client, calls };
}

const debugFields = {
  geometry: { offset: 0, kind: 6 },
  is_rendered: { offset: 8, kind: 7 },
  has_color_override: { offset: 16, kind: 7 },
  r: { offset: 20, kind: 1 },
  g: { offset: 24, kind: 1 },
  b: { offset: 28, kind: 1 },
} as const;

test("bound debug declarations write only declared fields and never clear removed ones", async () => {
  const fields = debugFields;
  const sphere: EntityRef = { kind: "symbol", symbol: "producer-debug-sphere" };
  const { client, calls } = recordingClient(
    { BoundingGeometry: { id: 9, fields } },
    { "producer-debug-sphere": 40n },
    new Set(["producer-debug-sphere:9"]),
  );
  const tree = new ReactWorldTree(client);
  const entity = tree.instance("ipp-entity", {
    bindTo: "producer-debug-sphere",
  });
  const debug = tree.instance(
    "ipp-bounding-geometry",
    BoundingGeometry({ is_rendered: true }).props,
  );
  entity.children.push(debug);
  tree.children.push(entity);
  const commits = new ReactWorldCommits(client, {});
  const setField = (offset: number, value: FieldValue): Command => ({
    kind: "setField",
    entity: sphere,
    component: 9,
    field: { offset, value },
  });
  try {
    await commits.capture(tree.describe());
    assert.deepEqual(
      calls[0],
      [
        {
          kind: "insertComponent",
          entity: sphere,
          component: 9,
          fields: [
            {
              offset: fields.is_rendered.offset,
              value: { kind: "bool", value: true },
            },
          ],
          adopt: true,
        },
      ],
      "omitted shape and color leave the producer's fields unwritten",
    );

    debug.props = BoundingGeometry({
      is_rendered: true,
      geometry: new Uint8Array([1, 2, 3]),
      color: [1, 0, 0],
    }).props;
    await commits.capture(tree.describe());
    // Several changed fields are one write, validated together.
    assert.deepEqual(calls.at(-1), [
      {
        kind: "insertComponent",
        entity: sphere,
        component: 9,
        fields: [
          {
            offset: fields.geometry.offset,
            value: { kind: "bytes", value: new Uint8Array([1, 2, 3]) },
          },
          {
            offset: fields.has_color_override.offset,
            value: { kind: "bool", value: true },
          },
          { offset: fields.r.offset, value: { kind: "f32", value: 1 } },
          { offset: fields.g.offset, value: { kind: "f32", value: 0 } },
          { offset: fields.b.offset, value: { kind: "f32", value: 0 } },
        ],
        adopt: true,
      },
    ]);

    debug.props = BoundingGeometry({ is_rendered: false }).props;
    await commits.capture(tree.describe());
    assert.deepEqual(
      calls.at(-1),
      [setField(fields.is_rendered.offset, { kind: "bool", value: false })],
      "omission leaves the written shape and color in place",
    );
    const submitted = calls.length;
    debug.props = BoundingGeometry({ is_rendered: false }).props;
    await commits.capture(tree.describe());
    assert.equal(
      calls.length,
      submitted,
      "an unchanged false boolean must not emit another write",
    );
    assert.throws(
      () => tree.validate("ipp-bounding-geometry", { is_rendered: 1 }),
      /must be a boolean/,
    );
  } finally {
    await commits.dispose();
  }
  assert.equal(
    calls.at(-1)?.[0]?.kind,
    "setField",
    "an adopted component on a bound entity stays after unmount",
  );
});

test("removed declarations delete their entities and components, adopted or not; unmount deletes nothing", async () => {
  const { client, calls } = recordingClient(
    { BoundingGeometry: { id: 9, fields: debugFields } },
    { adopted: 40n, inserted: 41n },
    new Set(["adopted:9"]),
  );
  const tree = new ReactWorldTree(client);
  for (const props of [
    { bindTo: "adopted" },
    { bindTo: "inserted" },
    { id: "declared" },
  ]) {
    const entity = tree.instance("ipp-entity", props);
    entity.children.push(
      tree.instance(
        "ipp-bounding-geometry",
        BoundingGeometry({ is_rendered: true }).props,
      ),
    );
    tree.children.push(entity);
  }
  const commits = new ReactWorldCommits(client, {});
  await commits.capture(tree.describe());
  assert.deepEqual(
    calls[0]!.map((command) =>
      command.kind === "insertComponent"
        ? [command.kind, command.entity, command.adopt]
        : [command.kind],
    ),
    [
      ["create"],
      ["insertComponent", { kind: "symbol", symbol: "adopted" }, true],
      ["insertComponent", { kind: "symbol", symbol: "inserted" }, true],
      ["insertComponent", { kind: "alias", alias: 1 }, true],
    ],
  );
  const mounted = calls.length;
  await commits.dispose();
  assert.equal(calls.length, mounted, "unmount deletes nothing");

  const remounted = new ReactWorldCommits(client, {});
  await remounted.capture(tree.describe());
  tree.children.length = 0;
  await remounted.capture(tree.describe());
  // A removed bound Entity deletes nothing, but its removed component
  // declarations remove their components, including an adopted one.
  assert.deepEqual(calls.at(-1), [
    {
      kind: "removeComponent",
      entity: { kind: "symbol", symbol: "adopted" },
      component: 9,
    },
    {
      kind: "removeComponent",
      entity: { kind: "symbol", symbol: "inserted" },
      component: 9,
    },
    { kind: "delete", entity: { kind: "handle", id: 101n } },
  ]);
});

test("immutable asset inputs encode only after data or encoder identity changes", () => {
  const { client } = recordingClient({});
  const tree = new ReactWorldTree(client);
  let calls = 0;
  const encode = (data: Uint8Array<ArrayBuffer>) => {
    calls++;
    return data;
  };
  const data = new Uint8Array([1, 2, 3]);
  const asset = tree.instance("ipp-asset", {
    id: "mesh",
    kind: 1,
    data,
    encode,
  });
  tree.children.push(asset);
  const first = tree.describe();
  for (let render = 0; render < 10; render++) {
    asset.props = { ...asset.props };
    assert.equal(tree.describe().signature, first.signature);
  }
  assert.equal(calls, 1);
  asset.props = { ...asset.props, data: data.slice() };
  assert.equal(tree.describe().signature, first.signature);
  assert.equal(
    calls,
    2,
    "new input is encoded even when its bytes compare equal",
  );
  asset.props = {
    ...asset.props,
    encode: (value: Uint8Array<ArrayBuffer>) => encode(value),
  };
  assert.equal(tree.describe().signature, first.signature);
  assert.equal(calls, 3);
  asset.props = { ...asset.props, data: new Uint8Array([4]) };
  assert.notEqual(tree.describe().signature, first.signature);
  assert.equal(calls, 4);
});
