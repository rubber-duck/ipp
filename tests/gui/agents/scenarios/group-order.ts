import type { Client, Command, EntityRef, createGuiAgent } from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../../fixtures/commands.js";
import { check } from "../../../harness/page/checks.js";

const alias = (alias: number): EntityRef => ({ kind: "alias", alias });
const handle = (id: bigint): EntityRef => ({ kind: "handle", id });
const place = (entity: EntityRef, parent: EntityRef): Command => ({
  kind: "placeEntity",
  entity,
  placement: { parent, before: null },
});

export async function verifyAgentGroupOrder(
  client: Client,
  createAgent: typeof createGuiAgent,
) {
  const agent = createAgent({ other: client });
  const created = successfulBatch(
    await client.batch([
      createEntity(1, "mixed-group"),
      insertComponent(client, "GuiGroup", alias(1)),
      createEntity(2, "mixed-button-scroll"),
      place(alias(2), alias(1)),
      insertComponent(client, "GuiButton", alias(2), { label: "Mixed button" }),
      insertComponent(client, "GuiScrollView", alias(2)),
      createEntity(3, "mixed-scroll-target"),
      place(alias(3), alias(1)),
      insertComponent(client, "GuiScrollView", alias(3)),
      insertComponent(client, "GuiInputTarget", alias(3)),
      createEntity(4, "option-under-scroll-target"),
      place(alias(4), alias(3)),
      insertComponent(client, "GuiButton", alias(4), {
        label: "Through scroll",
      }),
      createEntity(5, "first-former-parent"),
      createEntity(6, "second-former-parent"),
      createEntity(7, "reuse-for-first-sibling"),
    ]),
  );
  successfulBatch(
    await client.batch([
      { kind: "delete", entity: handle(aliasId(created, 7)) },
    ]),
  );
  const siblings = successfulBatch(
    await client.batch([
      createEntity(1, "older-promoted-button"),
      insertComponent(client, "GuiButton", alias(1), {
        label: "Older promoted",
      }),
      place(alias(1), handle(aliasId(created, 5))),
      createEntity(2, "newer-promoted-button"),
      insertComponent(client, "GuiButton", alias(2), {
        label: "Newer promoted",
      }),
      place(alias(2), handle(aliasId(created, 6))),
      { kind: "delete", entity: handle(aliasId(created, 5)) },
      { kind: "delete", entity: handle(aliasId(created, 6)) },
    ]),
  );
  const older = aliasId(siblings, 1);
  const newer = aliasId(siblings, 2);
  const native = (
    await client.inspectTreePage({ maxDepth: 0, limit: 256 })
  ).nodes.filter((n) => n.id === older || n.id === newer);
  check(
    native.length === 2 &&
      native[0]!.order === native[1]!.order &&
      native[0]!.id > native[1]!.id,
    "Promoted siblings share their native order label and traverse opposite to handle order",
  );
  const snapshot = await agent.snapshot();
  check(
    snapshot.observations[0]?.status === "observed",
    "Group test World was observed",
  );
  const promoted = snapshot.controls.filter(
    (row) => row.entity === String(older) || row.entity === String(newer),
  );
  check(
    promoted.length === 2 &&
      promoted.every((row, index) => row.entity === String(native[index]!.id)),
    "Agent preserves the World's durable order for tied siblings",
  );
  const group = snapshot.controls.find(
    (row) => row.entity === String(aliasId(created, 1)) && row.kind === "group",
  );
  check(group, "Native mixed-role group is present");
  const entries = snapshot.controls.filter((row) => row.group === group.ref);
  check(
    entries.length === 2 &&
      entries[0]!.label === "Mixed button" &&
      entries[0]!.available === false &&
      entries[1]!.label === "Through scroll",
    "Group lists the primary button once and reaches descendants through a primary scroll view",
  );
  const cyclic = successfulBatch(
    await client.batch([
      createEntity(1, "cyclic-checkbox"),
      insertComponent(client, "GuiCheckbox", alias(1)),
      insertComponent(client, "GuiBehavior", alias(1), {
        semantic_label: "Cyclic checkbox",
      }),
    ]),
  );
  const target = handle(aliasId(cyclic, 1));
  const rejected = await client.batch([place(target, target)]);
  check(
    !rejected.ok && rejected.error.reason === "UnsupportedDependency",
    "World retains the cyclic branch after the failed placement",
  );
  await client.waitForFrame(rejected.tick);
  const unaffected = await agent.act({
    steps: [{ target: { label: "Older promoted" }, action: { kind: "press" } }],
  });
  check(
    unaffected.steps[0]?.status === "acknowledged",
    "An invalid branch preserves label actions on reachable controls",
  );
  const checkbox = unaffected.observed.controls.find(
    (row) => row.entity === String(aliasId(cyclic, 1)),
  );
  check(
    checkbox?.kind === "checkbox" &&
      checkbox.available === false &&
      checkbox.group === null &&
      checkbox.actions.length === 0,
    "The cyclic checkbox is still discoverable as an unavailable, ungrouped control",
  );
  successfulBatch(
    await client.batch([
      {
        kind: "placeEntity",
        entity: target,
        placement: { parent: null, before: null },
      },
    ]),
  );
  await verifyParentRetirement(client, createAgent);
}

/** Retire an unobserved plain parent between pages of the watched entity scan. */
async function verifyParentRetirement(
  client: Client,
  createAgent: typeof createGuiAgent,
) {
  const pair = successfulBatch(
    await client.batch([
      createEntity(1, "pagination-a"),
      createEntity(2, "pagination-b"),
    ]),
  );
  const members = [aliasId(pair, 1), aliasId(pair, 2)];
  const page = await client.inspectPage({ collection: "entities", limit: 256 });
  const pairInPageOrder = page.entities.filter((row) =>
    members.includes(row.id),
  );
  check(pairInPageOrder.length === 2, "Both pagination fixtures are visible");
  const child = pairInPageOrder[0]!.id;
  const parent = handle(pairInPageOrder[1]!.id);
  successfulBatch(
    await client.batch([
      insertComponent(client, "GuiButton", handle(child), {
        label: "Paged child",
      }),
      place(handle(child), parent),
    ]),
  );
  let scans = 0;
  let retired = false;
  const reader = new Proxy(client, {
    get(target, key) {
      if (key === "inspectPage")
        return async (options: Parameters<Client["inspectPage"]>[0]) => {
          if (options?.after === 0n) scans++;
          const split = scans === 2 && !retired;
          const page = await target.inspectPage(
            split && options ? { ...options, limit: 1 } : options,
          );
          if (split && page.entities.some((row) => row.id === child)) {
            successfulBatch(
              await target.batch([{ kind: "delete", entity: parent }]),
            );
            retired = true;
          }
          return page;
        };
      const value: unknown = Reflect.get(target, key, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const result = await createAgent({ other: reader }).act({
    steps: [{ target: { label: "Paged child" }, action: { kind: "press" } }],
  });
  check(
    retired &&
      result.steps[0]?.error === "IncompleteObservation" &&
      result.steps[0].batch === null,
    "Losing a plain parent between pages prevents a label action on the inconsistent observation",
  );
  check(
    result.observed.controls.some(
      (row) => row.label === "Paged child" && row.parent === null,
    ),
    "Fresh observation finds the child promoted to the root",
  );
}
