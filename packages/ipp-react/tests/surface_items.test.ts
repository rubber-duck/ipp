import assert from "node:assert/strict";
import test from "node:test";
import type {
  BatchOutcome,
  Command,
  StateOverlayAlias,
  SurfaceCollection,
} from "@ipp/client";
import type { ReactWorldClient } from "../src/contract.js";
import { ReactWorldCommits } from "../src/commits.js";
import { SurfaceItemDeclarations } from "../src/surface_items.js";
import { ReactWorldTree } from "../src/tree.js";

function surfaceClient() {
  const calls: Command[][] = [];
  let encodes = 0;
  const client: ReactWorldClient = {
    session: 1n,
    schemaHash: 1n,
    capabilities: {
      stateOverlays: true,
      spatial: false,
      textures: false,
      builtinAssets: false,
      picking: false,
      debugGeometry: false,
      pbr: false,
      shadows: false,
      skeletalAnimation: false,
      meshPoses: false,
      surfaces: true,
    },
    components: {
      Surface: {
        id: 40,
        fields: {
          width: { offset: 0, kind: 1 },
          height: { offset: 4, kind: 1 },
          items: { offset: 8, kind: 6 },
        },
        dynamicProperties: true,
      },
    },
    encodeSurfaceItems(collection: SurfaceCollection) {
      encodes += 1;
      return new TextEncoder().encode(JSON.stringify(collection));
    },
    onDiagnostic: () => () => {},
    async batch(operations): Promise<BatchOutcome> {
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
            entity: operation.kind === "createStateOverlayOwner" ? null : 10n,
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
  return { client, calls, encodes: () => encodes };
}

test("Surface declarations reuse structural bytes and only write genuine changes", async () => {
  const recording = surfaceClient();
  const tree = new ReactWorldTree(recording.client);
  const entity = tree.instance("ipp-entity", { id: "surface" });
  const surface = tree.instance("ipp-surface", {
    bound: false,
    width: 2,
    height: 1,
    items: [
      {
        key: "label",
        content: { kind: "label", text: "ready" },
        opacity: 1,
      },
    ],
  });
  entity.children.push(surface);
  tree.children.push(entity);
  const commits = new ReactWorldCommits(recording.client, {});

  await commits.capture(tree.describe());
  assert.equal(recording.encodes(), 1);
  const initialCalls = recording.calls.length;

  surface.props = {
    ...surface.props,
    items: [
      {
        key: "label",
        content: { kind: "label", text: ["rea", "dy"].join("") },
        opacity: 0.5,
      },
    ],
  };
  await commits.capture(tree.describe());
  assert.equal(recording.encodes(), 1, "a style edit does not re-encode items");
  assert.equal(recording.calls.length, initialCalls + 1);
  assert.ok(
    recording.calls
      .at(-1)
      ?.every((operation) => operation.kind !== "updateComponentStateOverlay"),
    "a style edit does not resend structural bytes",
  );

  surface.props = {
    ...surface.props,
    items: [
      {
        key: "label",
        content: { kind: "label", text: "changed" },
        opacity: 0.5,
      },
    ],
  };
  await commits.capture(tree.describe());
  assert.equal(recording.encodes(), 2);
  assert.ok(
    recording.calls
      .at(-1)
      ?.some((operation) => operation.kind === "updateComponentStateOverlay"),
    "content changes resend structural bytes",
  );

  await commits.dispose();
});

test("equal newly allocated byte fields do not produce overlay writes", async () => {
  const recording = surfaceClient();
  const tree = new ReactWorldTree(recording.client);
  const entity = tree.instance("ipp-entity", { id: "surface" });
  const surface = tree.instance("ipp-surface", {
    bound: false,
    width: 1,
    items: new Uint8Array([1, 2, 3]),
  });
  entity.children.push(surface);
  tree.children.push(entity);
  const commits = new ReactWorldCommits(recording.client, {});

  await commits.capture(tree.describe());
  const calls = recording.calls.length;
  surface.props = {
    ...surface.props,
    width: 2,
    items: new Uint8Array([1, 2, 3]),
  };
  await commits.capture(tree.describe());
  assert.equal(recording.calls.length, calls + 1);
  const update = recording.calls
    .at(-1)
    ?.find((operation) => operation.kind === "updateComponentStateOverlay");
  assert.ok(update);
  assert.deepEqual(update.fields, [
    { offset: 0, value: { kind: "f32", value: 2 } },
  ]);

  await commits.dispose();
});

test("Surface item snapshots detect nested mutation and preserve keyed identities", () => {
  const declarations = new SurfaceItemDeclarations();
  const collections: SurfaceCollection[] = [];
  const encode = (collection: SurfaceCollection) => {
    collections.push(collection);
    return new Uint8Array([collections.length]);
  };
  const glyph = {
    key: "glyph",
    content: {
      kind: "glyphRun" as const,
      glyphs: [{ glyphId: 7, position: [1, 2] as [number, number] }],
    },
  };
  const drawing = { key: "drawing", content: { kind: "drawing" as const } };

  declarations.describe([glyph, drawing], encode);
  const [glyphId, drawingId] = collections[0]!.items.map(({ id }) => id);
  glyph.content.glyphs[0]!.position[0] = 3;
  declarations.describe([glyph, drawing], encode);
  assert.equal(collections.length, 2, "nested glyph edits invalidate bytes");

  declarations.describe([drawing, glyph], encode);
  assert.deepEqual(
    collections.at(-1)!.items.map(({ id }) => id),
    [drawingId, glyphId],
    "reorder retains keyed identities",
  );
  declarations.describe([drawing], encode);
  declarations.describe([drawing, glyph], encode);
  assert.ok(
    collections.at(-1)!.items[1]!.id > Math.max(glyphId!, drawingId!),
    "a removed and reinserted key receives a fresh monotonic identity",
  );
});
