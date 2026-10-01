import type {
  BatchOutcome,
  Client,
  GuiWorldClient,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import type { GuiTransportProbe } from "../gui-local-transport.js";
import { accepted, effectLog, guiAction, refused } from "../gui-actions.js";
import {
  compareAndSet,
  control,
  type GuiControlState,
  TREE_PAGE_MAX_DEPTH,
} from "./gui-lifecycle.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function observed(
  client: GuiWorldClient,
  entity: bigint,
): Promise<GuiControlState> {
  return control(client, entity);
}

/** The committed parent of `entity`, from its one link. */
async function parentOf(client: GuiWorldClient, entity: bigint) {
  const page = await client.inspectPage({
    collection: "entities",
    target: entity,
    limit: 1,
  });
  return page.entities.find((item) => item.id === entity)?.link.parent;
}

export async function ordinaryGui(
  host: WorldPersistenceHostClient<Client>,
  probe: GuiTransportProbe,
) {
  const owned = await host.createWorld({
    selectedSystems: [
      "ipp.canvas",
      "ipp.gui",
      "ipp.lifecycle-publisher",
      "ipp.world-attachment",
    ],
  });
  const peerWorld = await host.createWorld({ selectedSystems: [] });
  const client = (await host.openWorld(owned.reference)) as GuiWorldClient;
  const peer = await host.openWorld(peerWorld.reference);
  const observer = (await host.openWorld(owned.reference)) as GuiWorldClient;
  const effects = await effectLog(observer);
  try {
    const initial = await client.batch([
      createEntity(1, "root"),
      createEntity(2, "button"),
      insertComponent(client, "GuiButton", { kind: "alias", alias: 2 }),
      createEntity(3, "checkbox"),
      insertComponent(client, "GuiCheckbox", { kind: "alias", alias: 3 }),
      createEntity(4, "slider"),
      insertComponent(client, "GuiSlider", { kind: "alias", alias: 4 }),
      createEntity(5, "text"),
      insertComponent(client, "GuiTextInput", { kind: "alias", alias: 5 }),
      ...[2, 3, 4, 5].map((alias) => ({
        kind: "placeEntity" as const,
        entity: { kind: "alias" as const, alias },
        placement: {
          parent: { kind: "alias" as const, alias: 1 },
          before: null,
        },
      })),
      insertComponent(client, "WorldAttachment", { kind: "alias", alias: 1 }),
      { kind: "delete", entity: { kind: "handle", id: 0xffffffffffffffffn } },
    ]);
    check(
      !initial.ok &&
        initial.aliases.length === 5 &&
        initial.effects.length === 1,
      "Declaration failure lost known prefix/receipt",
    );
    const entities = new Map(
      initial.aliases.map(({ alias, id }) => [alias, id]),
    );
    const root = entities.get(1)!;
    const button = await observed(client, entities.get(2)!);
    const checkbox = await observed(client, entities.get(3)!);
    const slider = await observed(client, entities.get(4)!);
    const text = await observed(client, entities.get(5)!);
    check(
      (await host.getRootOutputBinding(owned.reference)) === null,
      "Headless GUI selected presentation",
    );
    // Controls are ordinary entities in core tree pages.
    const first = await client.inspectTreePage({
      root,
      limit: 2,
      maxDepth: TREE_PAGE_MAX_DEPTH,
    });
    check(
      first.nodes.length === 2 &&
        first.nodes[0]!.id === root &&
        first.next !== 0n,
      "Page did not bound visited ordinary entities",
    );
    const second = await client.inspectTreePage({
      root,
      after: first.next,
      limit: 2,
      maxDepth: TREE_PAGE_MAX_DEPTH,
    });
    check(
      second.nodes.length === 2 && second.nodes[0]!.id !== first.nodes[1]!.id,
      "Cursor repeated a row",
    );
    const pressOutcome = await guiAction(client, button.target, {
      kind: "press",
    });
    accepted(pressOutcome, "Press");
    const press = await effects.next(
      (effect) =>
        effect.effect.kind === "pressed" &&
        effect.target.entity === button.target.entity,
      "Press published no effect",
    );
    check(
      press.source === "semantic" &&
        press.tick === pressOutcome.tick &&
        press.ancestry[0] === root &&
        press.target.world.id === owned.reference.id &&
        press.target.incarnation === button.target.incarnation,
      "Press effect lost exact committed identity/ancestry",
    );
    const buttonAfter = await observed(client, button.target.entity);
    check(
      buttonAfter.target.incarnation === button.target.incarnation &&
        buttonAfter.value.kind === "none" &&
        !buttonAfter.focused,
      "Momentary press mutated value/focus",
    );
    const toggle = await guiAction(client, checkbox.target, {
      kind: "toggle",
    });
    accepted(toggle, "Toggle");
    const toggled = await observed(observer, checkbox.target.entity);
    check(
      toggled.value.kind === "bool" && toggled.value.value,
      "Checkbox did not commit",
    );
    // A write conditioned on the value another client read before the toggle
    // conflicts and changes nothing.
    const stale = await compareAndSet(
      observer,
      checkbox.target.entity,
      "GuiCheckbox",
      "checked",
      false,
      false,
    );
    check(
      !stale.ok && stale.error.reason === "ValueMismatch",
      "Compare-and-set on a stale value was not refused",
    );
    const current = await observed(observer, checkbox.target.entity);
    check(
      current.value.kind === "bool" && current.value.value,
      "Refused compare-and-set changed the checkbox",
    );
    // An equal compare-and-set applies and leaves the value as it was.
    const replacement = successfulBatch(
      await compareAndSet(
        client,
        checkbox.target.entity,
        "GuiCheckbox",
        "checked",
        true,
        true,
      ),
    );
    const replaced = await observed(observer, checkbox.target.entity);
    check(
      replaced.target.incarnation === current.target.incarnation &&
        replaced.value.kind === "bool" &&
        replaced.value.value,
      "Equal compare-and-set changed the control",
    );
    const scalar = await guiAction(client, slider.target, {
      kind: "scalar",
      value: 0.25,
    });
    accepted(scalar, "Scalar");
    const scalarNow = await observed(observer, slider.target.entity);
    check(
      scalarNow.value.kind === "scalar" && scalarNow.value.value === 0.25,
      "Scalar commit changed",
    );
    // A value outside the range is refused and changes nothing.
    refused(
      await guiAction(client, slider.target, { kind: "scalar", value: 2 }),
      "InvalidValue",
    );
    const unchanged = (await observed(observer, slider.target.entity)).value;
    check(
      unchanged.kind === "scalar" && unchanged.value === 0.25,
      "Refused scalar changed the slider",
    );
    refused(
      await guiAction(client, slider.target, { kind: "toggle" }),
      "UnsupportedAction",
    );
    const unicode = "Aé🙂" + "x".repeat(4000);
    accepted(
      await guiAction(client, text.target, { kind: "text", value: unicode }),
      "Text",
    );
    const textNow = await observed(client, text.target.entity);
    check(
      textNow.value.kind === "text" && textNow.value.value === unicode,
      "Unicode commit truncated",
    );
    check(
      !textNow.interaction.hovered &&
        !textNow.interaction.pressed &&
        !textNow.interaction.captured,
      "Headless snapshot invented pointer interaction",
    );
    // Logical focus set by commands has no owning session: any client may
    // blur the focused control, and a repeated action changes nothing.
    for (const [owner, kind, focused, changed] of [
      [client, "focus", true, true],
      [client, "focus", true, false],
      [observer, "blur", false, true],
      [client, "blur", false, false],
      [client, "focus", true, true],
    ] as const) {
      const outcome = await guiAction(owner, textNow.target, { kind });
      accepted(outcome, kind);
      if (changed) {
        const effect = await effects.next(
          (effect) =>
            effect.effect.kind === "focusChanged" &&
            effect.target.entity === text.target.entity,
          "Focus change published no effect",
        );
        check(
          effect.effect.kind === "focusChanged" &&
            effect.effect.focused === focused &&
            effect.effect.changed &&
            effect.tick === outcome.tick,
          "Focus effect lost its result",
        );
      }
      const afterFocus = await observed(observer, text.target.entity);
      check(
        afterFocus.value.kind === "text" &&
          afterFocus.value.value === unicode &&
          afterFocus.focused === focused,
        "Focus/Blur changed the committed text",
      );
    }
    check(
      effects.effects.filter(
        (effect) =>
          effect.effect.kind === "focusChanged" &&
          effect.target.entity === text.target.entity,
      ).length === 3,
      "An unchanged focus action published an effect",
    );
    const oldSession = client.session;
    await client.close();
    check(
      (await observed(observer, text.target.entity)).focused,
      "Session close revoked focus that no session owns",
    );
    const reopened = (await host.openWorld(owned.reference)) as GuiWorldClient;
    check(reopened.session !== oldSession, "Session lifetime reused");
    successfulBatch(await peer.batch([createEntity(1, "healthy-peer")]));
    const nextRoot = successfulBatch(
      await reopened.batch([createEntity(1, "next-parent")]),
    ).aliases[0]!.id;
    const pendingPress = guiAction(reopened, button.target, { kind: "press" });
    const reparent = reopened.batch([
      {
        kind: "placeEntity",
        entity: { kind: "handle", id: button.target.entity },
        placement: { parent: { kind: "handle", id: nextRoot }, before: null },
      },
    ]);
    accepted(await pendingPress, "Pending press");
    successfulBatch(await reparent);
    const pinned = await effects.next(
      (effect) =>
        effect.effect.kind === "pressed" &&
        effect.target.entity === button.target.entity,
      "Pending press published no effect",
    );
    check(
      pinned.ancestry[0] === root &&
        (await parentOf(observer, button.target.entity)) === nextRoot,
      "Applied effect ancestry followed a later reparent instead of the committed path",
    );
    successfulBatch(
      await observer.batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id: checkbox.target.entity },
          component: checkbox.target.component,
        },
        insertComponent(observer, "GuiCheckbox", {
          kind: "handle",
          id: checkbox.target.entity,
        }),
      ]),
    );
    refused(
      await guiAction(reopened, current.target, { kind: "toggle" }),
      "StaleTarget",
    );
    const latest = await observed(reopened, checkbox.target.entity);
    check(
      latest.target.incarnation !== current.target.incarnation,
      "Recreated control reused incarnation",
    );
    check(
      latest.value.kind === "bool" && !latest.value.value,
      "A stale action changed the replacement control",
    );
    const corruptedOutcomes: BatchOutcome[] = [];
    for (const corruption of ["outcome", "error"] as const) {
      const latest = await observed(observer, checkbox.target.entity);
      const victim = (await host.openWorld(owned.reference)) as GuiWorldClient;
      const delivered = probe.corruptApplied(victim.session, corruption);
      const expected =
        corruption === "error"
          ? "Host 3: committed outcome unavailable"
          : "Batch response correlation mismatch";
      const action = guiAction(victim, latest.target, { kind: "toggle" });
      // Held by the probe until after the corrupted outcome.
      const waiting = victim.inspectPage({
        collection: "entities",
        target: latest.target.entity,
        limit: 1,
      });
      void waiting.catch(() => {});
      let uncertain = false;
      try {
        await action;
      } catch (error) {
        check(
          error instanceof Error && error.message.includes(expected),
          "Correlated corruption did not reject honestly",
        );
        uncertain = true;
      }
      check(uncertain, "Corrupt outcome resolved successfully");
      const actualOutcome = await delivered;
      corruptedOutcomes.push(actualOutcome);
      check(actualOutcome.ok, "Fault injection did not follow a real commit");
      const committed = await observed(observer, latest.target.entity);
      check(
        committed.target.incarnation === latest.target.incarnation &&
          committed.value.kind === "bool" &&
          latest.value.kind === "bool" &&
          committed.value.value !== latest.value.value,
        "Unknown reply was replayed or rolled back instead of preserving the actual commit",
      );
      check(probe.batches(victim.session) === 1, "Uncertain mutation retried");
      if (corruption === "outcome") {
        // A reply of another kind breaks correlation: the client stops.
        check(victim.closure, "Corrupted logical Client stayed live");
        check(
          (await victim.closed).reason.message.includes(expected),
          "Corrupted logical Client stayed live",
        );
        const stopped = await Promise.allSettled([
          waiting,
          guiAction(victim, latest.target, { kind: "toggle" }),
        ]);
        check(
          stopped.every((result) => result.status === "rejected"),
          "Unknown GUI commit left pending or future work usable",
        );
        check(
          probe.batches(victim.session) === 1,
          "Closed Client submitted fresh work",
        );
      } else {
        // A Host error after commit only makes this batch's outcome
        // uncertain; the session stays usable and nothing was retried.
        await waiting;
        check(!victim.closure, "Uncertain outcome closed the Client");
      }
      await victim.close();
      successfulBatch(
        await peer.batch([
          createEntity(3, `healthy-after-corruption-${corruption}`),
        ]),
      );
    }
    const pending = Array.from({ length: 8 }, () =>
      guiAction(reopened, button.target, { kind: "press" }),
    );
    const destruction = host.destroyWorld(owned.reference);
    const settled = await Promise.allSettled(pending);
    await destruction;
    await reopened.closed;
    check(settled.length === 8, "World destroy stranded accepted request");
    successfulBatch(
      await peer.batch([createEntity(2, "healthy-after-destroy")]),
    );
    return {
      first,
      second,
      press,
      toggle,
      replacement,
      scalar,
      textBytes: new TextEncoder().encode(unicode).length,
      oldSession,
      newSession: reopened.session,
      corruptedOutcomes,
      settled: settled.map((value) => value.status),
    };
  } finally {
    await effects.close();
    await observer.close().catch(() => {});
    await client.close().catch(() => {});
    await peer.close().catch(() => {});
    await host.destroyWorld(owned.reference).catch(() => {});
    await host.destroyWorld(peerWorld.reference);
  }
}
