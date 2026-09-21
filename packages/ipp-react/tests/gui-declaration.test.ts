/** GUI asset binding and enabled-lane declaration tests (P07 R12 + disabled lane).
 *
 * Headless and node-runnable: pure tree description, commit signatures and
 * diffs with no transport and no reconciler. Real runtime coverage stays in
 * the pipeline `react` suite.
 */
import assert from "node:assert/strict";
import test from "node:test";
import type { GuiNodeHandle } from "@ipp/client";
import { ENTITY_HOST_TYPE } from "../src/components.js";
import type { ReactWorldClient } from "../src/contract.js";
import {
  GUI_CHECKBOX_HOST_TYPE,
  GUI_COLUMN_HOST_TYPE,
  GUI_IMAGE_HOST_TYPE,
  GUI_ROOT_HOST_TYPE,
} from "../src/gui/components.js";
import { normalizeGuiStyle } from "../src/gui/description.js";
import { diffGuiTree, type GuiAcknowledgedNode } from "../src/gui/diff.js";
import { ReactWorldTree } from "../src/tree.js";

function guiClient(): ReactWorldClient {
  return {
    session: 1n,
    components: { GuiRoot: { id: 26, fields: {} } },
  } as unknown as ReactWorldClient;
}

function makeHandle(nodeId: number, lifetime: number): GuiNodeHandle {
  return {
    session: 1n,
    entity: 100n,
    rootIncarnation: 1n,
    nodeId,
    nodeLifetime: lifetime,
  };
}

function describePanel(
  tree: ReactWorldTree,
  imageProps: Record<string, unknown>,
  checkboxProps: Record<string, unknown>,
): void {
  const entity = tree.instance(ENTITY_HOST_TYPE, { id: "panel" });
  const root = tree.instance(GUI_ROOT_HOST_TYPE, {});
  const column = tree.instance(GUI_COLUMN_HOST_TYPE, {});
  const checkbox = tree.instance(GUI_CHECKBOX_HOST_TYPE, {
    checked: false,
    ...checkboxProps,
  });
  const image = tree.instance(GUI_IMAGE_HOST_TYPE, {
    size: [1, 1],
    ...imageProps,
  });
  column.children.push(checkbox, image);
  root.children.push(column);
  entity.children.push(root);
  tree.children.push(entity);
}

const drawingA = { kind: 18, source: "asset://18/7", variant: 0 };
const drawingB = { kind: 18, source: "asset://18/8", variant: 0 };

test("gui asset binds inline with no overlays and dedupes identical sources", () => {
  const tree = new ReactWorldTree(guiClient());
  describePanel(tree, { asset: drawingA }, { asset: drawingA });
  const description = tree.describe();

  // Inline sources create no asset declarations and no component overlays:
  // nothing tears down when assets bind or swap.
  assert.deepEqual(description.assets, []);
  assert.deepEqual(description.overlays, []);
  assert.equal(description.gui.length, 1);
  const nodes = description.gui[0]!.nodes;
  assert.equal(nodes.length, 3);
  const bound = nodes.filter((node) => node.style.asset !== undefined);
  assert.equal(bound.length, 2);
  for (const node of bound) assert.deepEqual(node.style.asset, drawingA);
});

test("gui asset and enabled props validate loudly", () => {
  const tree = new ReactWorldTree(guiClient());
  assert.throws(
    () =>
      tree.instance(GUI_IMAGE_HOST_TYPE, {
        size: [1, 1],
        asset: { kind: 0, source: "" },
      }),
    /GUI asset must be an asset source/,
  );
  assert.throws(
    () =>
      tree.instance(GUI_CHECKBOX_HOST_TYPE, {
        checked: false,
        enabled: "no" as unknown as boolean,
      }),
    /GUI enabled must be a boolean/,
  );
});

test("asset swap resubmits through the style signature as one update", () => {
  const before = new ReactWorldTree(guiClient());
  describePanel(before, { asset: drawingA }, {});
  const rootA = before.describe().gui[0]!;
  const after = new ReactWorldTree(guiClient());
  describePanel(after, { asset: drawingB }, {});
  const rootB = after.describe().gui[0]!;
  assert.notEqual(rootA.signature, rootB.signature);

  const acked = new Map<number, GuiAcknowledgedNode>(
    rootA.nodes.map((node) => [
      node.identity,
      {
        nodeId: node.identity,
        parent: node.parent,
        parentId: node.parent,
        lifetime: 1,
        content: node.content,
        style: node.style,
      },
    ]),
  );
  const ids = new Map(
    rootA.nodes.map((node) => [node.identity, node.identity]),
  );
  const order = new Map<number | undefined, number[]>();
  for (const node of rootA.nodes) {
    const list = order.get(node.parent);
    if (list) list.push(node.identity);
    else order.set(node.parent, [node.identity]);
  }
  const plan = diffGuiTree(rootB.nodes, acked, ids, 100, order, makeHandle);
  // Exactly one in-place update naming the new source: no remove, no
  // insert, no overlay work.
  assert.equal(plan.edits.length, 1);
  const edit = plan.edits[0]!;
  assert.equal(edit.action, "update");
  if (edit.action !== "update") throw new Error("expected update");
  assert.deepEqual(edit.patch.style?.asset, drawingB);
});

test("enabled lane disables through insert style and update patches", () => {
  const tree = new ReactWorldTree(guiClient());
  describePanel(tree, {}, { enabled: false });
  const description = tree.describe();
  const checkbox = description.gui[0]!.nodes[1]!;
  assert.equal(checkbox.style.enabled, false);

  const enabled = new ReactWorldTree(guiClient());
  describePanel(enabled, {}, {});
  const enabledRoot = enabled.describe().gui[0]!;
  assert.equal(enabledRoot.nodes[1]!.style.enabled, true);
  assert.notEqual(description.gui[0]!.signature, enabledRoot.signature);

  // Toggling the lane diffs to an update carrying the lane.
  const acked = new Map<number, GuiAcknowledgedNode>(
    enabledRoot.nodes.map((node) => [
      node.identity,
      {
        nodeId: node.identity,
        parent: node.parent,
        parentId: node.parent,
        lifetime: 1,
        content: node.content,
        style: node.style,
      },
    ]),
  );
  const ids = new Map(
    enabledRoot.nodes.map((node) => [node.identity, node.identity]),
  );
  const plan = diffGuiTree(
    description.gui[0]!.nodes,
    acked,
    ids,
    100,
    new Map(),
    makeHandle,
  );
  const updates = plan.edits.filter((edit) => edit.action === "update");
  assert.equal(updates.length, 1);
  const patch = (
    updates[0] as Extract<(typeof plan.edits)[number], { action: "update" }>
  ).patch;
  assert.equal(
    (patch.style as { enabled?: boolean } | undefined)?.enabled,
    false,
  );

  // Sparse reads default the lane to true.
  assert.equal(normalizeGuiStyle({}).enabled, true);
});
