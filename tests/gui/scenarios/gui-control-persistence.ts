import type {
  Client,
  Command,
  EntitySnapshot,
  GuiAction,
  GuiCommittedEffect,
  GuiEffectSubscription,
  GuiObservedEffect,
  GuiWorldClient,
  WorldGraphLoadError,
  WorldPersistenceHostClient,
  WorldReference,
} from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import {
  compareAndSet,
  control,
  setFields,
  type GuiControlState,
  type GuiControlValue,
  TREE_PAGE_MAX_DEPTH,
} from "./gui-lifecycle.js";
import { guiAction } from "../../fixtures/gui-actions.js";
import { check } from "../../harness/page/checks.js";

function encoded(value: unknown): string {
  return JSON.stringify(value, (_key, entry) =>
    typeof entry === "bigint" ? `${entry}n` : entry,
  );
}

function equal(actual: unknown, expected: unknown, message: string): void {
  check(
    encoded(actual) === encoded(expected),
    `${message}: ${encoded(actual)} != ${encoded(expected)}`,
  );
}

function controlOf(client: GuiWorldClient, entity: bigint) {
  return control(client, entity);
}

async function entities(client: Client) {
  const page = await client.inspectPage({ collection: "entities", limit: 32 });
  equal(page.next, 0n, "Small fixture inspection unexpectedly paged");
  return new Map(
    page.entities.map((entity) => [entity.metadata.symbolicId!, entity]),
  );
}

function named(rows: ReadonlyMap<string, EntitySnapshot>, name: string) {
  const result = rows.get(name);
  check(result, `Missing ordinary entity ${name}`);
  return result;
}

async function orderedTree(
  client: GuiWorldClient,
  rows: ReadonlyMap<string, EntitySnapshot>,
) {
  const names = new Map([...rows].map(([name, entity]) => [entity.id, name]));
  const result: { name: string; parent: string | null; depth: number }[] = [];
  let after: bigint | undefined;
  for (let pageIndex = 0; pageIndex < rows.size; pageIndex++) {
    const page = await client.inspectTreePage({
      root: named(rows, "root").id,
      ...(after === undefined ? {} : { after }),
      limit: 2,
      maxDepth: TREE_PAGE_MAX_DEPTH,
    });
    check(page.nodes.length > 0 && page.nodes.length <= 2, "Invalid tree page");
    for (const node of page.nodes) {
      const entity = [...rows.values()].find((entry) => entry.id === node.id);
      check(entity, "Unknown tree entity");
      equal(
        { parent: node.parent, order: node.order },
        entity.link,
        "Tree traversal diverged from the entity's link",
      );
      result.push({
        name: names.get(node.id)!,
        parent: node.parent === null ? null : names.get(node.parent)!,
        depth: node.depth,
      });
    }
    if (page.next === 0n) return result;
    check(page.next !== after, "Tree cursor did not advance");
    after = page.next;
  }
  throw new Error("GUI fixture tree did not terminate");
}

/** The committed path from the root to `entity`, read from links. */
async function ancestryOf(client: Client, entity: bigint) {
  const path: bigint[] = [];
  for (let current: bigint | null = entity; current !== null; ) {
    path.unshift(current);
    const page = await client.inspectPage({
      collection: "entities",
      target: current,
      limit: 1,
    });
    const found = page.entities.find((item) => item.id === current);
    check(found, `Missing ancestor ${current}`);
    current = found.link.parent;
  }
  return path;
}

function committed(state: GuiControlState) {
  return state.value;
}

/** What an effect says, apart from its World-local ordinal. */
function effectIdentity(effect: GuiCommittedEffect) {
  return [
    effect.target,
    effect.source,
    effect.tick,
    effect.ancestry,
    effect.effect,
  ];
}

/**
 * Apply one semantic action as a batch and check its outcome, the momentary
 * effect it publishes, recorded in `receipts` for the observer to match
 * (`expected`, or null for value actions and focus no-ops), and the control's
 * resulting value (`value`, or unchanged).
 */
async function mutate(
  client: GuiWorldClient,
  state: GuiControlState,
  operation: GuiAction,
  expected: GuiCommittedEffect["effect"] | null,
  receipts: GuiCommittedEffect[],
  value: GuiControlValue = state.value,
) {
  const outcome = await guiAction(client, state.target, operation);
  check(outcome.ok, `Action rejected: ${encoded(outcome)}`);
  if (expected)
    receipts.push({
      id: null,
      target: state.target,
      source: "semantic",
      tick: outcome.tick,
      ancestry: await ancestryOf(client, state.target.entity),
      effect: expected,
    });
  const current = await controlOf(client, state.target.entity);
  equal(current.target, state.target, "Action changed control incarnation");
  equal(committed(current), value, "Fields do not hold the applied value");
  return current;
}

async function drainEffects(
  subscription: GuiEffectSubscription,
  received: GuiObservedEffect[],
  receipts: GuiCommittedEffect[],
) {
  const cut = await subscription.unsubscribe();
  equal(cut.kind, "unsubscribed", "Missing unsubscribe cut");
  equal(cut.world, subscription.start.world, "Unsubscribe changed World");
  equal(cut.session, subscription.start.session, "Unsubscribe changed session");
  equal(cut.subscription, subscription.id, "Unsubscribe changed subscription");
  equal(
    await subscription.closed,
    { kind: "unsubscribed", cut },
    "Observer closed before drain",
  );
  equal(
    received.map(effectIdentity),
    receipts.map(effectIdentity),
    "Observer lost, duplicated or reordered published effects",
  );
  check(
    received.every(
      (effect, index) =>
        effect.id.world.id === subscription.start.world.id &&
        (index === 0 || effect.id.ordinal > received[index - 1]!.id.ordinal),
    ),
    "Observed effect identities are not ordered within their World",
  );
}

export async function exerciseOrdinaryGuiPersistence(
  host: WorldPersistenceHostClient<Client>,
) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let completed = false;
  const open = async (world: WorldReference) => {
    const client = await host.openWorld(world);
    sessions.push(client);
    check("subscribeGuiEffects" in client, "Target lacks ordinary GUI");
    return client as GuiWorldClient;
  };
  try {
    const created = await host.createWorld({
      symbolicId: "gui-persistence-source",
      selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.lifecycle-publisher"],
    });
    worlds.push(created.reference);
    const source = await open(created.reference);
    const observer = await open(created.reference);
    const observed: GuiObservedEffect[] = [];
    const receipts: GuiCommittedEffect[] = [];
    const subscription = await observer.subscribeGuiEffects(
      (effect) => observed.push(effect),
      { classes: "all" },
    );
    equal(
      subscription.start.world,
      created.reference,
      "Subscribe ACK changed World",
    );
    equal(
      subscription.start.session,
      observer.session,
      "Subscribe ACK changed session",
    );
    equal(subscription.start.kind, "subscribed", "Missing subscribe ACK");

    const names = [
      "root",
      "group",
      "button",
      "checkbox",
      "slider",
      "text",
      "extra-checkbox",
      "plain",
    ];
    const batch = successfulBatch(
      await source.batch([
        ...names.map((name, index) => createEntity(index + 1, name)),
        insertComponent(
          source,
          "GuiButton",
          { kind: "alias", alias: 3 },
          { label: "Save" },
        ),
        insertComponent(
          source,
          "GuiCheckbox",
          { kind: "alias", alias: 4 },
          { checked: false, label: "initial" },
        ),
        insertComponent(
          source,
          "GuiSlider",
          { kind: "alias", alias: 5 },
          { value: 0.25, min: 0, max: 1, step: 0.25 },
        ),
        insertComponent(
          source,
          "GuiTextInput",
          { kind: "alias", alias: 6 },
          { text: "seed", placeholder: "Type" },
        ),
        insertComponent(
          source,
          "GuiCheckbox",
          { kind: "alias", alias: 7 },
          { label: "extra" },
        ),
        ...[2, 3, 4, 5, 6, 7, 8].map(
          (alias): Command => ({
            kind: "placeEntity",
            entity: { kind: "alias", alias },
            placement: {
              parent: { kind: "alias", alias: alias === 2 ? 1 : 2 },
              before: null,
            },
          }),
        ),
        {
          kind: "placeEntity",
          entity: { kind: "alias", alias: 6 },
          placement: {
            parent: { kind: "alias", alias: 2 },
            before: { kind: "alias", alias: 5 },
          },
        },
      ]),
    );
    const button = await controlOf(source, aliasId(batch, 3));
    let checkbox = await controlOf(source, aliasId(batch, 4));
    let slider = await controlOf(source, aliasId(batch, 5));
    let text = await controlOf(source, aliasId(batch, 6));
    let extra = await controlOf(source, aliasId(batch, 7));
    const initial = [button, checkbox, slider, text, extra];
    equal(
      initial.map(committed),
      [
        { kind: "none" },
        { kind: "bool", value: false },
        { kind: "scalar", value: 0.25 },
        { kind: "text", value: "seed" },
        { kind: "bool", value: false },
      ],
      "Declared control values changed",
    );
    await mutate(
      source,
      button,
      { kind: "press" },
      { kind: "pressed" },
      receipts,
    );
    for (const value of [true, false]) {
      checkbox = await mutate(
        source,
        checkbox,
        { kind: "toggle" },
        null,
        receipts,
        { kind: "bool", value },
      );
    }
    slider = await mutate(
      source,
      slider,
      { kind: "scalar", value: 0.75 },
      null,
      receipts,
      { kind: "scalar", value: 0.75 },
    );
    // An equal compare-and-set applies and leaves the value.
    successfulBatch(
      await compareAndSet(
        source,
        slider.target.entity,
        "GuiSlider",
        "value",
        0.75,
        0.75,
      ),
    );
    slider = await controlOf(source, slider.target.entity);
    equal(
      committed(slider),
      { kind: "scalar", value: 0.75 },
      "Equal compare-and-set changed the slider",
    );
    text = await mutate(
      source,
      text,
      { kind: "text", value: "Saved café 🙂" },
      null,
      receipts,
      { kind: "text", value: "Saved café 🙂" },
    );
    text = await mutate(
      source,
      text,
      { kind: "focus" },
      { kind: "focusChanged", focused: true, changed: true, part: 0 },
      receipts,
    );
    check(text.focused, "Source focus was not established before save");

    // Configuration is an ordinary field: writing the label leaves the
    // checked value alone.
    const checkboxType = source.components.GuiCheckbox!.id;
    successfulBatch(
      await source.batch(
        setFields(source, checkbox.target.entity, "GuiCheckbox", {
          label: "saved label",
        }),
      ),
    );
    extra = await mutate(source, extra, { kind: "toggle" }, null, receipts, {
      kind: "bool",
      value: true,
    });
    const before = await entities(source);
    const configured = named(before, "checkbox");
    const configuredFields = configured.components.find(
      (entry) => entry.component === checkboxType,
    )?.fields;
    equal(
      [configuredFields?.label, configuredFields?.checked],
      ["saved label", false],
      "Checkbox fields do not hold the last writes",
    );
    for (const name of ["GuiBehavior", "CanvasBounds"])
      check(
        configured.components.some(
          (entry) => entry.component === source.components[name]!.id,
        ),
        `Required ${name} is not an ordinary component`,
      );
    const configuredState = await controlOf(source, checkbox.target.entity);
    equal(
      configuredState.label,
      "saved label",
      "GUI ignored the written configuration",
    );
    equal(
      committed(configuredState),
      committed(checkbox),
      "Configuration write changed the checked value",
    );
    const expectedTree = [
      { name: "root", parent: null, depth: 0 },
      { name: "group", parent: "root", depth: 1 },
      ...[
        "button",
        "checkbox",
        "text",
        "slider",
        "extra-checkbox",
        "plain",
      ].map((name) => ({ name, parent: "group", depth: 2 })),
    ];
    equal(
      await orderedTree(source, before),
      expectedTree,
      "Authored ordinary tree order changed",
    );
    const saved = await host.saveWorld(source.session);
    check(saved.byteLength > 0, "Save returned no owned graph bytes");
    const loaded = await host
      .loadWorld(saved, { symbolicId: "gui-persistence-restored" })
      .catch((error: unknown) => {
        if (error instanceof Error && error.name === "WorldGraphLoadError") {
          worlds.push(
            ...(error as WorldGraphLoadError).pendingCleanup.values(),
          );
        }
        throw error;
      });
    worlds.push(...loaded.created.values());
    equal(
      loaded.created.size,
      1,
      "Fixture unexpectedly saved descendant Worlds",
    );
    check(
      loaded.root.id !== created.reference.id ||
        loaded.root.incarnation !== created.reference.incarnation,
      "Restore reused the live World reference",
    );
    const restored = await open(loaded.root);
    equal(
      restored.world?.persistentId,
      created.persistentId,
      "Restore changed durable World identity",
    );
    const restoredObserver = await open(loaded.root);
    const restoredEffects: GuiObservedEffect[] = [];
    const restoredReceipts: GuiCommittedEffect[] = [];
    const restoredSubscription = await restoredObserver.subscribeGuiEffects(
      (effect) => restoredEffects.push(effect),
      { classes: "all" },
    );
    equal(
      restoredSubscription.start.world,
      loaded.root,
      "Restored subscription lost World",
    );
    equal(
      restoredSubscription.start.session,
      restoredObserver.session,
      "Restored subscription lost session",
    );
    const after = await entities(restored);
    equal(
      [...after.keys()].sort(),
      [...names].sort(),
      "Entity persistence lost or added entities",
    );
    equal(
      await orderedTree(restored, after),
      expectedTree,
      "Restored ordered core links changed",
    );
    // A save captures every stored component of every entity.
    for (const name of names) {
      equal(
        named(after, name).metadata,
        named(before, name).metadata,
        `Metadata changed: ${name}`,
      );
      equal(
        named(after, name).components,
        named(before, name).components,
        `Stored components changed: ${name}`,
      );
    }
    equal(named(after, "plain").components, [], "Plain entity gained values");
    const durable = [button, checkbox, slider, text, extra];
    const restoredStates: GuiControlState[] = [];
    for (const [index, name] of [
      "button",
      "checkbox",
      "slider",
      "text",
      "extra-checkbox",
    ].entries()) {
      const state = await controlOf(restored, named(after, name).id);
      restoredStates.push(state);
      equal(
        state.target.world,
        loaded.root,
        `Control retained old World: ${name}`,
      );
      equal(
        committed(state),
        committed(durable[index]!),
        `Committed value changed: ${name}`,
      );
      equal(state.focused, false, `Logical focus persisted: ${name}`);
      equal(
        state.interaction,
        { hovered: false, pressed: false, captured: false },
        `Restored control invented interaction: ${name}`,
      );
    }
    equal(
      restoredStates[1]!.label,
      "saved label",
      "Restore lost the written configuration",
    );
    check(
      (await controlOf(source, text.target.entity)).focused,
      "Loading another World cleared live source focus",
    );
    // Restore keeps entity and component identities; a command's target is
    // World-local, so actions below change only the World they are sent to.
    equal(
      committed(await controlOf(restored, restoredStates[1]!.target.entity)),
      committed(checkbox),
      "Restore changed the checkbox value",
    );
    await mutate(
      restored,
      restoredStates[0]!,
      { kind: "press" },
      { kind: "pressed" },
      restoredReceipts,
    );
    const fresh = await mutate(
      restored,
      restoredStates[1]!,
      { kind: "toggle" },
      null,
      restoredReceipts,
      { kind: "bool", value: true },
    );
    equal(
      committed(await controlOf(source, checkbox.target.entity)),
      committed(checkbox),
      "Restored action mutated original World",
    );
    await mutate(source, checkbox, { kind: "toggle" }, null, receipts, {
      kind: "bool",
      value: true,
    });
    equal(
      committed(await controlOf(restored, fresh.target.entity)),
      committed(fresh),
      "Original target mutated restored World",
    );
    await drainEffects(subscription, observed, receipts);
    await drainEffects(restoredSubscription, restoredEffects, restoredReceipts);
    completed = true;
    return {
      snapshotBytes: saved.byteLength,
      sourceWorld: encoded(created.reference),
      restoredWorld: encoded(loaded.root),
      tree: expectedTree,
      restoredCommitted: restoredStates.map(committed),
      configuration: named(after, "checkbox").components,
      sourceEffects: observed.length,
      restoredEffects: restoredEffects.length,
      originalTargetStillLive: true,
      restoredLogicalFocusCleared: true,
      headlessOnly: true,
    };
  } finally {
    const closed = await Promise.allSettled(
      sessions.map((client) => client.close()),
    );
    const destroyed = await Promise.allSettled(
      worlds.map((world) => host.destroyWorld(world)),
    );
    if (completed) {
      check(
        [...closed, ...destroyed].every(
          (result) => result.status === "fulfilled",
        ),
        "Persistence fixture cleanup failed",
      );
    }
  }
}
