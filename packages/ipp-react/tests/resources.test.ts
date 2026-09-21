/** Local source equality/diff invariants; actual provider rendering lives in tests/render. */
import assert from "node:assert/strict";
import test from "node:test";
import { BoundingGeometry } from "../src/components.js";
import { ReactWorldTree } from "../src/tree.js";
import { ReactWorldCommits } from "../src/commits.js";
import type { ReactWorldClient } from "../src/contract.js";
import type { Command, StateOverlayAlias } from "@ipp/client";

test("resource strings retain target offsets, compare by value, update and clear", async () => {
  const { client, calls } = recordingClient({
    Scalar: { id: 1, fields: { value: { offset: 0, kind: 1 } } },
    MeshInstance: {
      id: 5,
      fields: {
        source: { offset: 24, kind: 5 },
        variant: { offset: 48, kind: 3 },
      },
    },
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
  const attach = calls[0]?.find(
    (operation) => operation.kind === "attachComponentStateOverlay",
  );
  assert.ok(attach);
  assert.equal(attach.kind, "attachComponentStateOverlay");
  assert.deepEqual(attach.fields, [
    {
      offset: 24,
      value: { kind: "string", value: "https://example.test/é.ippm" },
    },
  ]);
  mesh.props = {
    ...mesh.props,
    source: ["https://example.test/", "é.ippm"].join(""),
  };
  await commits.capture(tree.describe());
  assert.equal(
    calls.length,
    1,
    "equal source strings do not emit a new overlay",
  );
  for (const source of ["ipp://mesh/cube?width=1&height=1&length=1", ""]) {
    mesh.props = { ...mesh.props, source };
    await commits.capture(tree.describe());
    assert.deepEqual(calls.at(-1), [
      {
        kind: "updateComponentStateOverlay",
        owner: { kind: "handle", id: 1n },
        overlay: { kind: "handle", id: 3n },
        fields: [{ offset: 24, value: { kind: "string", value: source } }],
        clear: [],
      },
    ]);
  }
  mesh.props = {};
  await commits.capture(tree.describe());
  assert.deepEqual(calls.at(-1), [
    {
      kind: "updateComponentStateOverlay",
      owner: { kind: "handle", id: 1n },
      overlay: { kind: "handle", id: 3n },
      fields: [],
      clear: [24],
    },
  ]);
  assert.throws(
    () => tree.validate("ipp-mesh-instance", { source: 9n }),
    /must be a string/,
  );
  assert.throws(
    () => tree.validate("ipp-mesh-instance", { asset: 9n }),
    /Unsupported.*asset/,
  );
  await commits.dispose();
});

function recordingClient(components: ReactWorldClient["components"]) {
  const calls: Command[][] = [];
  const client: ReactWorldClient = {
    session: 1n,
    schemaHash: 1n,
    capabilities: {
      stateOverlays: true,
      spatial: true,
      textures: true,
      builtinAssets: false,
      picking: false,
      debugGeometry: false,
      pbr: false,
      shadows: false,
      skeletalAnimation: false,
      meshPoses: false,
    },
    components,
    onDiagnostic: () => () => {},
    async batch(operations) {
      calls.push(operations);
      const stateOverlays: StateOverlayAlias[] = [];
      for (const operation of operations) {
        if (
          operation.kind === "createStateOverlayOwner" ||
          operation.kind === "attachEntityOverlayBinding" ||
          operation.kind === "attachComponentStateOverlay"
        )
          stateOverlays.push({
            alias: operation.alias,
            id: BigInt(operation.alias),
            kind:
              operation.kind === "createStateOverlayOwner"
                ? "owner"
                : operation.kind === "attachEntityOverlayBinding"
                  ? "entityOverlayBinding"
                  : "componentStateOverlay",
            entity: operation.kind === "createStateOverlayOwner" ? null : 100n,
          });
      }
      return {
        ok: true,
        batchId: BigInt(calls.length),
        tick: BigInt(calls.length),
        aliases: [],
        stateOverlays,
      };
    },
  };
  return { client, calls };
}

test("bound debug declarations preserve omitted producer shape and color and clear removed overrides", async () => {
  const fields = {
    geometry: { offset: 0, kind: 6 },
    is_rendered: { offset: 8, kind: 7 },
    has_color_override: { offset: 16, kind: 7 },
    r: { offset: 20, kind: 1 },
    g: { offset: 24, kind: 1 },
    b: { offset: 28, kind: 1 },
  } as const;
  const { client, calls } = recordingClient({
    BoundingGeometry: { id: 9, fields },
  });
  const tree = new ReactWorldTree(client);
  const entity = tree.instance("ipp-entity", {
    bindTo: "producer-debug-sphere",
  });
  const debug = tree.instance(
    "ipp-bounding-geometry",
    BoundingGeometry({ bound: true, is_rendered: true }).props,
  );
  entity.children.push(debug);
  tree.children.push(entity);
  const commits = new ReactWorldCommits(client, {});
  try {
    await commits.capture(tree.describe());
    const binding = calls[0]?.find(
      (operation) => operation.kind === "attachEntityOverlayBinding",
    );
    assert.equal(binding?.mode, "bound");
    const attached = calls[0]?.find(
      (operation) => operation.kind === "attachComponentStateOverlay",
    );
    assert.equal(attached?.mode, "bound");
    assert.deepEqual(
      attached?.fields,
      [
        {
          offset: fields.is_rendered.offset,
          value: { kind: "bool", value: true },
        },
      ],
      "omitted shape and color must leave producer fields unshadowed",
    );

    debug.props = BoundingGeometry({
      bound: true,
      is_rendered: true,
      geometry: new Uint8Array([1, 2, 3]),
      color: [1, 0, 0],
    }).props;
    await commits.capture(tree.describe());
    const overridden = calls
      .at(-1)
      ?.find((operation) => operation.kind === "updateComponentStateOverlay");
    assert.ok(overridden);
    assert.deepEqual(overridden.fields, [
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
    ]);

    debug.props = BoundingGeometry({ bound: true, is_rendered: false }).props;
    await commits.capture(tree.describe());
    const restored = calls
      .at(-1)
      ?.find((operation) => operation.kind === "updateComponentStateOverlay");
    assert.ok(restored);
    assert.deepEqual(restored.fields, [
      {
        offset: fields.is_rendered.offset,
        value: { kind: "bool", value: false },
      },
    ]);
    assert.deepEqual(
      restored.clear,
      [
        fields.geometry.offset,
        fields.has_color_override.offset,
        fields.r.offset,
        fields.g.offset,
        fields.b.offset,
      ],
      "omission releases previous overlay fields to reveal producer values",
    );
    const submitted = calls.length;
    debug.props = BoundingGeometry({ bound: true, is_rendered: false }).props;
    await commits.capture(tree.describe());
    assert.equal(
      calls.length,
      submitted,
      "an unchanged false boolean must not emit another update",
    );
    assert.throws(
      () => tree.validate("ipp-bounding-geometry", { is_rendered: 1 }),
      /must be a boolean/,
    );
  } finally {
    await commits.dispose();
  }
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
