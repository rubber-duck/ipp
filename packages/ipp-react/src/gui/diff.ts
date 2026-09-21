/** Pure diff of desired GUI declarations against acknowledged state.
 *
 * Render-time safe: no transport. The commit phase submits the returned edits
 * in order through the generated GUI client.
 */
import type {
  GuiEdit,
  GuiNodeContent,
  GuiNodeHandle,
  GuiNodePatchStyle,
  GuiNodeStyle,
} from "@ipp/client";
import {
  equalGuiAsset,
  equalGuiContent,
  normalizeGuiStyle,
  type GuiDescribedNode,
} from "./description.js";
import type { GuiDeclarationStyle } from "./components.js";

/** Style patch lanes including the authoring-only enabled lane, which
 * rides the runtime object until the shared contract regenerates it. */
export type GuiDeclarationPatchStyle = GuiNodePatchStyle & {
  enabled?: boolean | undefined;
};

/** Acknowledged runtime state for one declared node. */
export interface GuiAcknowledgedNode {
  readonly nodeId: number;
  /** Parent instance identity, or undefined for the root node. */
  readonly parent: number | undefined;
  readonly parentId: number | undefined;
  readonly lifetime: number;
  readonly content: GuiNodeContent;
  readonly style: GuiDeclarationStyle;
}

export interface GuiDiffResult {
  /** Ordered edits: removes, then inserts, then moves, then updates. */
  readonly edits: readonly GuiEdit[];
  /** Identity to node ID mapping including fresh allocations. */
  readonly ids: ReadonlyMap<number, number>;
  readonly nextId: number;
}

function tuplesEqual(
  a: readonly number[] | undefined,
  b: readonly number[] | undefined,
): boolean {
  if (a === undefined || b === undefined) return a === b;
  return (
    a.length === b.length &&
    a.every((value, index) => Object.is(value, b[index]))
  );
}

function stylePatch(
  desiredInput: GuiNodeStyle | GuiDeclarationStyle,
  ackedInput: GuiNodeStyle | GuiDeclarationStyle,
): GuiDeclarationPatchStyle | undefined {
  const desired = normalizeGuiStyle(desiredInput);
  const acked = normalizeGuiStyle(ackedInput);
  const patch: GuiDeclarationPatchStyle = {};
  let changed = false;
  const set = <K extends keyof GuiDeclarationPatchStyle>(
    key: K,
    value: GuiDeclarationPatchStyle[K],
  ): void => {
    patch[key] = value;
    changed = true;
  };
  const nullable = (
    key:
      | "width"
      | "height"
      | "minWidth"
      | "minHeight"
      | "maxWidth"
      | "maxHeight",
  ): void => {
    const next = desired[key] ?? null;
    const previous = acked[key] ?? null;
    if (!Object.is(next, previous)) set(key, next);
  };
  nullable("width");
  nullable("height");
  nullable("minWidth");
  nullable("minHeight");
  nullable("maxWidth");
  nullable("maxHeight");
  if (!tuplesEqual(desired.padding, acked.padding))
    set("padding", desired.padding === undefined ? null : [...desired.padding]);
  if (!tuplesEqual(desired.margin, acked.margin))
    set("margin", desired.margin === undefined ? null : [...desired.margin]);
  if (!Object.is(desired.flex ?? null, acked.flex ?? null))
    set("flex", desired.flex ?? null);
  if (!Object.is(desired.alignX ?? null, acked.alignX ?? null))
    set("alignX", desired.alignX ?? null);
  if (!Object.is(desired.alignY ?? null, acked.alignY ?? null))
    set("alignY", desired.alignY ?? null);
  if (!tuplesEqual(desired.color, acked.color))
    set("color", [...(desired.color ?? [1, 1, 1, 1])]);
  if (!tuplesEqual(desired.backgroundColor, acked.backgroundColor))
    set(
      "backgroundColor",
      desired.backgroundColor === undefined
        ? null
        : [...desired.backgroundColor],
    );
  if (!Object.is(desired.opacity ?? 1, acked.opacity ?? 1))
    set("opacity", desired.opacity ?? 1);
  if (!Object.is(desired.fontSize ?? 0.1, acked.fontSize ?? 0.1))
    set("fontSize", desired.fontSize ?? 0.1);
  if (!equalGuiAsset(desired.asset, acked.asset))
    set("asset", desired.asset ?? null);
  if (!Object.is(desired.enabled ?? true, acked.enabled ?? true))
    set("enabled", desired.enabled ?? true);
  return changed ? patch : undefined;
}

/** Desired child index among siblings in visit order. */
function siblingIndex(
  desired: readonly GuiDescribedNode[],
  node: GuiDescribedNode,
): number {
  let index = 0;
  for (const sibling of desired) {
    if (sibling === node) return index;
    if (sibling.parent === node.parent) index += 1;
  }
  return index;
}

export function diffGuiTree(
  desired: readonly GuiDescribedNode[],
  acked: ReadonlyMap<number, GuiAcknowledgedNode>,
  ids: ReadonlyMap<number, number>,
  nextId: number,
  order: ReadonlyMap<number | undefined, readonly number[]>,
  makeHandle: (nodeId: number, lifetime: number) => GuiNodeHandle,
): GuiDiffResult {
  const assigned = new Map(ids);
  let counter = nextId;
  for (const node of desired) {
    if (!assigned.has(node.identity)) assigned.set(node.identity, counter++);
  }
  const nodeIdOf = (identity: number): number => {
    const id = assigned.get(identity);
    if (id === undefined) throw new Error("Missing GUI node identity");
    return id;
  };

  const desiredIds = new Set(desired.map((node) => node.identity));
  const removed = [...acked]
    .filter(([identity]) => !desiredIds.has(identity))
    .map(([identity, ack]) => ({ identity, ack }));
  const removedIds = new Set(removed.map((entry) => entry.identity));
  // Only the topmost removed nodes need an edit; removal cascades.
  const topmost = removed
    .filter(
      ({ ack }) => ack.parent === undefined || !removedIds.has(ack.parent),
    )
    .sort((a, b) => a.ack.nodeId - b.ack.nodeId);
  const edits: GuiEdit[] = topmost.map(({ ack }) => ({
    action: "remove" as const,
    handle: makeHandle(ack.nodeId, ack.lifetime),
  }));

  const inserts = desired
    .filter((node) => !acked.has(node.identity))
    .sort((a, b) => nodeIdOf(a.identity) - nodeIdOf(b.identity));
  for (const node of inserts) {
    edits.push({
      action: "insert",
      entity: 0n,
      rootIncarnation: 0n,
      id: nodeIdOf(node.identity),
      ...(node.parent === undefined ? {} : { parent: nodeIdOf(node.parent) }),
      index: siblingIndex(desired, node),
      content: node.content,
      style: node.style,
    });
  }

  // Moves: reparented nodes, plus every persisting child of a parent whose
  // order changed. Emitted in desired visit order so sequential absolute
  // placements converge on the desired arrangement.
  const reorderedParents = new Set<number | undefined>();
  const desiredOrder = new Map<number | undefined, number[]>();
  for (const node of desired) {
    if (acked.has(node.identity)) {
      const list = desiredOrder.get(node.parent);
      if (list) list.push(node.identity);
      else desiredOrder.set(node.parent, [node.identity]);
    }
  }
  for (const [parent, list] of desiredOrder) {
    const previous = order.get(parent) ?? [];
    const same =
      previous.length === list.length &&
      previous.every((identity, index) => identity === list[index]);
    if (!same) reorderedParents.add(parent);
  }
  for (const node of desired) {
    const ack = acked.get(node.identity);
    if (!ack) continue;
    const nextParentId =
      node.parent === undefined ? undefined : nodeIdOf(node.parent);
    if (ack.parentId !== nextParentId || reorderedParents.has(node.parent)) {
      edits.push({
        action: "move",
        handle: makeHandle(ack.nodeId, ack.lifetime),
        ...(nextParentId === undefined ? {} : { parent: nextParentId }),
        index: siblingIndex(desired, node),
      });
    }
  }

  for (const node of desired) {
    const ack = acked.get(node.identity);
    if (!ack) continue;
    const patch: { content?: GuiNodeContent; style?: GuiNodePatchStyle } = {};
    if (!equalGuiContent(node.content, ack.content))
      patch.content = node.content;
    const style = stylePatch(node.style, ack.style);
    if (style) patch.style = style;
    if (patch.content !== undefined || patch.style !== undefined) {
      edits.push({
        action: "update",
        handle: makeHandle(ack.nodeId, ack.lifetime),
        patch,
      });
    }
  }

  return { edits, ids: assigned, nextId: counter };
}
