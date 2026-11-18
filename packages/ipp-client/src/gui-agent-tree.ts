import type { Client } from "./client.js";
import type {
  GuiAgentControl,
  GuiAgentKind,
  GuiAgentObservation,
} from "./gui-agent-types.js";
import type { EntitySnapshot } from "./types.js";

/** Equal sibling order labels are broken by persistent identities known only to the World. */
async function orderSiblings(
  siblings: EntitySnapshot[],
  parent: bigint | null,
  client: Client,
  maxEntities: number,
  observation: GuiAgentObservation,
) {
  siblings.sort((a, b) =>
    a.link.order < b.link.order ? -1 : a.link.order > b.link.order ? 1 : 0,
  );
  const tied = new Map<bigint, bigint>();
  for (let i = 1; i < siblings.length; i++) {
    const left = siblings[i - 1]!;
    const right = siblings[i]!;
    if (left.link.order === right.link.order) {
      tied.set(left.id, left.link.order);
      tied.set(right.id, right.link.order);
    }
  }
  if (!tied.size) return siblings;

  const ranks = new Map<bigint, number>();
  let after = parent ?? 0n;
  let remaining = maxEntities;
  do {
    const page = await client.inspectTreePage({
      ...(parent === null ? { maxDepth: 0 } : { root: parent, maxDepth: 1 }),
      after,
      limit: Math.min(100, remaining),
    });
    observation.lastTick = String(page.tick);
    for (const node of page.nodes) {
      if (
        node.parent === parent &&
        tied.get(node.id) === node.order &&
        !ranks.has(node.id)
      )
        ranks.set(node.id, ranks.size);
    }
    remaining -= page.nodes.length;
    after = page.next;
  } while (ranks.size < tied.size && after !== 0n && remaining > 0);

  if (ranks.size !== tied.size) {
    observation.truncated ||= after !== 0n;
    observation.invalidated += tied.size - ranks.size;
    siblings = siblings.filter(
      (node) => !tied.has(node.id) || ranks.has(node.id),
    );
  }
  return siblings.sort((a, b) =>
    a.link.order === b.link.order
      ? (ranks.get(a.id) ?? 0) - (ranks.get(b.id) ?? 0)
      : a.link.order < b.link.order
        ? -1
        : 1,
  );
}

/** Return parent-before-child order without building an ancestry path per control. */
export async function guiAgentTreeOrder(
  entities: readonly EntitySnapshot[],
  client: Client,
  maxEntities: number,
  observation: GuiAgentObservation,
): Promise<EntitySnapshot[]> {
  const ids = new Set(entities.map((entity) => entity.id));
  const children = new Map<bigint | null, EntitySnapshot[]>();
  for (const entity of entities) {
    const parent = entity.link.parent;
    const siblings = children.get(parent);
    if (siblings) siblings.push(entity);
    else children.set(parent, [entity]);
  }
  const pending: EntitySnapshot[] = [];
  async function pushChildren(
    parent: bigint | null,
    siblings: EntitySnapshot[],
  ) {
    const ordered =
      siblings.length > 1
        ? await orderSiblings(
            siblings,
            parent,
            client,
            maxEntities,
            observation,
          )
        : siblings;
    for (let i = ordered.length - 1; i >= 0; i--) pending.push(ordered[i]!);
  }
  // An absent parent in a complete scan proves that the pages disagree.
  // Its subtree has no verified position and a tree query cannot rank it.
  if (!observation.truncated) {
    for (const parent of children.keys()) {
      if (parent !== null && !ids.has(parent)) observation.invalidated++;
    }
  }
  await pushChildren(null, children.get(null) ?? []);
  const result: EntitySnapshot[] = [];
  while (pending.length) {
    const entity = pending.pop()!;
    result.push(entity);
    const descendants = children.get(entity.id);
    if (descendants) await pushChildren(entity.id, descendants);
  }
  return result;
}

export function guiAgentGroupItems(
  tree: readonly EntitySnapshot[],
  controls: GuiAgentControl[],
  kinds: ReadonlyMap<number, GuiAgentKind>,
  observation: GuiAgentObservation,
): GuiAgentControl[] {
  const byEntity = new Map<bigint, GuiAgentControl[]>();
  for (const control of controls) {
    const id = BigInt(control.entity);
    const entries = byEntity.get(id);
    if (entries) entries.push(control);
    else byEntity.set(id, [control]);
  }
  const descendantGroup = new Map<bigint, string>();
  const ordered: GuiAgentControl[] = [];
  for (const entity of tree) {
    const rows = byEntity.get(entity.id) ?? [];
    byEntity.delete(entity.id);
    ordered.push(...rows.sort((a, b) => a.component - b.component));
    if (
      entity.components.some((entry) => kinds.get(entry.component) === "group")
    ) {
      const group = rows.find((row) => row.kind === "group");
      if (group) descendantGroup.set(entity.id, group.ref);
      continue;
    }
    const inherited =
      entity.link.parent !== null
        ? descendantGroup.get(entity.link.parent)
        : undefined;
    if (!inherited) continue;
    // Core selects the first present control in CONTROL_COMPONENTS order, even
    // when multiple controls make this entity unavailable for input.
    let item = false;
    for (const [component, kind] of kinds) {
      if (kind === "meter") continue;
      if (!entity.components.some((entry) => entry.component === component))
        continue;
      item = kind !== "scrollView" && kind !== "virtualList";
      if (item) {
        const primary = rows.find((row) => row.component === component);
        if (primary) primary.group = inherited;
      }
      break;
    }
    if (!item) descendantGroup.set(entity.id, inherited);
  }
  // Rows outside the rooted tree have no verified position or group. A stable
  // cyclic branch only contains unavailable controls. An eligible row here
  // signals inconsistent reads if no missing parent or bound already did.
  for (const rows of byEntity.values()) {
    if (
      !observation.truncated &&
      !observation.invalidated &&
      rows.some((row) => row.available === true)
    )
      observation.invalidated++;
    ordered.push(...rows.sort((a, b) => a.component - b.component));
  }
  return ordered;
}
