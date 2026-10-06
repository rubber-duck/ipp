import {
  createContext,
  createRef,
  Fragment,
  StrictMode,
  Suspense,
  useContext,
} from "react";
import type { ReactNode } from "react";
import type {
  Client,
  Command,
  GuiWorldClient,
  WorldPersistenceHostClient,
  WorldReference,
} from "@ipp/client";
import {
  createRoot,
  Entity,
  Children,
  AttachedWorld,
  type AttachedWorldHandle,
} from "@ipp/react";
import {
  Style,
  Layout,
  Theme,
  Skin,
  Behavior,
  Button,
  Checkbox,
  Slider,
  TextInput,
  type GuiControlHandle,
} from "@ipp/react/gui";
import {
  controlRefLifetimes,
  controlRefReplacementBurst,
  controlRefSubscriptionRaces,
  sharedRefAcquisition,
} from "../scenarios/control-ref-lifetimes.js";
import { controlRefScaling } from "../scenarios/control-ref-scaling.js";
import {
  controlRefCleanup,
  controlRefGatedCleanup,
  standaloneRefCleanupRetry,
  reentrantRootUnmount,
} from "../scenarios/control-ref-cleanup.js";
export { createWorkerHost } from "@ipp/client";
export { webSocketTransport } from "../../../../packages/ipp-client/src/transport.js";
export { lifecycleTargetTransport as reactLifecycleTransport } from "../../../runtime/drivers/browser-lifecycle-targets.js";
import type { LifecycleTransportProbe } from "../../../runtime/drivers/browser-lifecycle-targets.js";
import { controlRefSubsetFailure } from "../scenarios/control-ref-subsets.js";
import { guiAction } from "../../../fixtures/gui-actions.js";
import { guiCallbacks } from "../scenarios/gui-callbacks.js";
import { guiScroll } from "../scenarios/gui-scroll.js";
import { check } from "../../../harness/page/checks.js";

export type GuiContract = Pick<
  typeof import("@ipp/gui-authoring-contract"),
  | "Entity"
  | "GuiButton"
  | "GuiCheckbox"
  | "GuiSlider"
  | "GuiTextInput"
  | "GuiVirtualList"
  | "GuiLayout"
  | "GuiTheme"
  | "GuiSkin"
  | "guiPaintPartIndex"
  | "components"
>;

function current(ref: {
  current: GuiControlHandle | null;
}): GuiControlHandle | null {
  return ref.current;
}

function guiClient(client: Client): GuiWorldClient {
  if (!("subscribeGuiEffects" in client))
    throw new Error("Ordinary GUI client missing");
  return client as GuiWorldClient;
}

export function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

export async function entity(client: Client, name: string) {
  const found = (await client.inspect()).entities.find(
    (entry) => entry.metadata.symbolicId === name,
  );
  check(found, `Missing entity: ${name}`);
  return found;
}

/**
 * Row inserts create entities, links and sibling order in one logical batch
 * that names the new entities by binding aliases; the effective order is the
 * declared order, as it was when links waited for a second batch.
 */
async function oneBatchRowInserts(
  create: () => Promise<WorldReference>,
  open: (world: WorldReference) => Promise<Client>,
  rootFor: (client: Client) => ReturnType<typeof createRoot>,
): Promise<string> {
  const counts: number[] = [];
  for (const count of [3, 40]) {
    const client = await open(await create());
    let batches = 0;
    const counted = new Proxy(client, {
      get(target, property) {
        if (property === "batch")
          return (commands: Command[]) => {
            batches++;
            return target.batch(commands);
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const root = rootFor(counted);
    const panel = `rows-${count}`;
    const rows = (names: readonly string[]) => (
      <Entity id={panel}>
        <Layout kind={1} width={96} height={64} />
        <Children>
          {names.map((name) => (
            <Entity key={name} id={name}>
              <Layout width={4} height={2} />
            </Entity>
          ))}
        </Children>
      </Entity>
    );
    const order = async () => {
      const parent = await entity(client, panel);
      return (await client.inspect()).entities
        .filter((item) => item.link.parent === parent.id)
        .sort((left, right) => (left.link.order < right.link.order ? -1 : 1))
        .map((item) => item.metadata.symbolicId);
    };
    const initial = Array.from(
      { length: count },
      (_, index) => `${panel}-${index}`,
    );
    await root.render(rows(initial));
    const mounted = batches;
    check(mounted === 1, `Inserting ${count} rows took ${mounted} batches`);
    check(
      JSON.stringify(await order()) === JSON.stringify(initial),
      `One-batch insert of ${count} rows changed sibling order`,
    );
    const inserted = [
      initial[0]!,
      `${panel}-inserted-a`,
      `${panel}-inserted-b`,
      ...initial.slice(1),
    ];
    await root.render(rows(inserted));
    check(
      batches - mounted === 1,
      "Inserting rows between acknowledged rows split",
    );
    check(
      JSON.stringify(await order()) === JSON.stringify(inserted),
      "Rows inserted before acknowledged siblings changed sibling order",
    );
    counts.push(count);
  }
  return `${counts.join(" and ")} rows and a middle insert each apply in one batch`;
}

export async function guiAuthoring(
  host: WorldPersistenceHostClient<Client>,
  contract: GuiContract,
  connect: () => Promise<WorldPersistenceHostClient<Client>>,
  probe: LifecycleTransportProbe,
): Promise<string[]> {
  const report: string[] = [];
  const worlds: WorldReference[] = [];
  const clients: Client[] = [];
  const connections: WorldPersistenceHostClient<Client>[] = [];
  const roots: ReturnType<typeof createRoot>[] = [];
  const errors: Error[] = [];
  const create = async () => {
    const world = (
      await host.createWorld({
        selectedSystems: [
          "ipp.animation",
          "ipp.gui",
          "ipp.gui-layout",
          "ipp.canvas",
          "ipp.asset-dependencies",
          "ipp.world-attachment",
          "ipp.lifecycle-publisher",
        ],
        // The World is the canvas; each scenario's layout root sizes itself
        // within this extent.
        canvas: { extent: [128, 64], unitsPerMetre: 1 },
      })
    ).reference;
    worlds.push(world);
    return world;
  };
  const open = async (world: WorldReference) => {
    const client = guiClient(await host.openWorld(world));
    clients.push(client);
    return client;
  };
  const rootFor = (client: Client) => {
    const root = createRoot(client, {
      ...(host.sessions.get(client.session) === client ? { host } : {}),
      onError: (error) => errors.push(error),
    });
    roots.push(root);
    return root;
  };
  try {
    const world = await create();
    const client = await open(world);
    const peer = await open(world);
    await guiCallbacks(client, peer, () => open(world), contract);
    await guiScroll(client, peer, contract);
    const checkbox = createRef<GuiControlHandle>();
    const slider = createRef<GuiControlHandle>();
    const text = createRef<GuiControlHandle>();
    const button = createRef<GuiControlHandle>();
    let batches = 0;
    const authored: Command[][] = [];
    let hold:
      | {
          arrived: ReturnType<typeof deferred<void>>;
          release: ReturnType<typeof deferred<void>>;
        }
      | undefined;
    const wrapped = new Proxy(client, {
      get(target, property) {
        if (property === "batch")
          return async (commands: Command[]) => {
            batches++;
            authored.push(commands);
            const result = await target.batch(commands);
            const held = hold;
            if (held) {
              hold = undefined;
              held.arrived.resolve();
              await held.release.promise;
            }
            return result;
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const root = rootFor(wrapped);
    const part = contract.guiPaintPartIndex({ part: "background" });
    const parts = contract.GuiTheme.encodeParts({
      nextSlot: 4,
      rows: new Map([[3, { part, color: [1, 0, 0, 1] }]]),
    });
    const tree = (reverse = false, initial = false, label = "button") => (
      <StrictMode>
        <Entity id="theme">
          <Theme parts={parts} />
        </Entity>
        <Entity id="canvas">
          <Layout kind={1} width={96} height={64} />
          <Style opacity={1} />
          <Children>
            {(reverse ? ["check", "button"] : ["button", "check"]).map(
              (name) => (
                <Entity key={name} id={name}>
                  <Layout width={30} height={24} />
                  <Behavior />
                  {name === "button" ? (
                    <>
                      <Skin theme="theme" />
                      <Button label={label} ref={button} />
                    </>
                  ) : (
                    <Checkbox checked={initial} ref={checkbox} />
                  )}
                </Entity>
              ),
            )}
          </Children>
          <Entity id="unparented">
            <Slider min={0} max={10} value={2} ref={slider} />
          </Entity>
        </Entity>
        <Entity id="text">
          <TextInput text="seed" ref={text} />
        </Entity>
      </StrictMode>
    );
    const held = { arrived: deferred<void>(), release: deferred<void>() };
    hold = held;
    const mounting = root.render(tree());
    await Promise.race([
      held.arrived.promise,
      mounting.then(() => {
        throw new Error("Mount skipped the held ACK");
      }),
    ]);
    check(
      current(checkbox) === null && current(button) === null,
      "Refs preceded declaration ACK",
    );
    held.release.resolve();
    await mounting;
    check(
      checkbox.current && slider.current && text.current && button.current,
      "ACK refs missing",
    );
    const checkHandle = checkbox.current;
    const original = checkHandle.target;
    check(
      (await checkHandle.read()).checked === false,
      "Checkbox declared value",
    );
    check(
      (await checkHandle.action({ kind: "toggle" })).ok,
      "Explicit semantic toggle",
    );
    check(
      (await checkHandle.read()).checked === true,
      "Toggled checkbox value",
    );
    // An unchanged declared value is not written again.
    await root.render(tree(true, false, "changed"));
    check(checkbox.current === checkHandle, "Reorder replaced the control ref");
    check(
      (await checkHandle.read()).checked === true,
      "Rerender rewrote the unchanged declared value",
    );
    check(
      !(await checkHandle.compareAndSet("checked", false, false)),
      "Stale compare-and-set applied",
    );
    check(
      await checkHandle.compareAndSet("checked", true, false),
      "Current compare-and-set failed",
    );
    check(
      (await checkHandle.read()).checked === false,
      "Compare-and-set did not write",
    );
    for (const [handle, action, field, value] of [
      [button.current, { kind: "press" }, undefined, undefined],
      [slider.current, { kind: "scalar", value: 7 }, "value", 7],
      [text.current, { kind: "text", value: "edited" }, "text", "edited"],
    ] as const) {
      check((await handle.action(action)).ok, "Control semantic action failed");
      if (field)
        check(
          (await handle.read())[field] === value,
          "Semantic action did not write its field",
        );
    }
    const snapshot = await client.inspect();
    const componentNames = new Map<number, string>(
      Object.entries(contract.components).map(([name, { id }]) => [id, name]),
    );
    check(
      snapshot.entities.every((item) =>
        item.components.every(
          (component) => componentNames.get(component.component) !== "GuiRoot",
        ),
      ),
      "Legacy GuiRoot component exists",
    );
    check(
      (await entity(client, "unparented")).link.parent === null,
      "JSX nesting implicitly parented",
    );
    const canvasEntity = await entity(client, "canvas");
    const checkEntity = await entity(client, "check");
    const buttonEntity = await entity(client, "button");
    check(
      checkEntity.id === original.entity &&
        checkEntity.link.parent === canvasEntity.id &&
        checkEntity.link.order < buttonEntity.link.order,
      "Keyed explicit sibling order failed",
    );
    check(
      // Semantic actions are guiAction commands; structure uses generic ones.
      authored
        .flat()
        .filter((command) => command.kind !== "guiAction")
        .every((command) => !command.kind.toLowerCase().includes("gui")),
      "Non-generic structural authoring",
    );
    const before = batches;
    await root.render(tree(true, false, "changed"));
    check(batches === before, "Unchanged tree submitted structural work");
    report.push(await oneBatchRowInserts(create, open, rootFor));
    report.push(
      "generic declarations, explicit links, keyed reorder, four exact control refs, declared values, compare-and-set and actions",
    );

    const foreign = await peer.batch([
      contract.Entity.create(1, { symbolicId: "foreign-child" }),
      {
        kind: "placeEntity",
        entity: contract.Entity.alias(1),
        placement: {
          parent: contract.Entity.handle(canvasEntity.id),
          before: null,
        },
      },
    ]);
    check(foreign.ok, "Foreign child setup");
    const independent = rootFor(peer);
    await independent.render(
      <Entity id="independent">
        <Checkbox />
      </Entity>,
    );
    // Unmount deletes nothing; removing the declarations deletes them.
    await root.render(null);
    await root.unmount();
    check(current(checkbox) === null, "Unmount retained ref");
    let fenced = false;
    try {
      await checkHandle.action({ kind: "toggle" });
    } catch {
      fenced = true;
    }
    check(fenced, "Unmounted ref dispatched");
    check(
      (await entity(peer, "foreign-child")).link.parent === null,
      "Declared parent deletion deleted a foreign child",
    );
    await entity(peer, "independent");
    report.push(
      "independent roots and nonowning parent deletion preserve foreign entities",
    );

    const producer = await peer.batch([
      contract.Entity.create(2, { symbolicId: "restored" }),
      contract.GuiCheckbox.insert(contract.Entity.alias(2), {
        checked: true,
        label: "producer",
      }),
    ]);
    check(producer.ok, "Producer control setup");
    const borrowed = rootFor(client);
    const observed = createRef<GuiControlHandle>();
    await borrowed.render(
      <Entity bindTo="restored">
        <Checkbox checked={false} label="declared" ref={observed} />
      </Entity>,
    );
    check(observed.current, "Bound control ref");
    const boundState = await observed.current.read();
    check(
      boundState.checked === false && boundState.label === "declared",
      "Declared values did not overwrite the adopted producer control",
    );
    check(
      (
        await peer.batch([
          contract.GuiCheckbox.setLabel(
            contract.Entity.handle(observed.current.target.entity),
            "latest-producer",
          ),
        ])
      ).ok,
      "Producer update",
    );
    await borrowed.unmount();
    const kept = (await entity(peer, "restored")).components.find(
      (component) => component.component === contract.GuiCheckbox.id,
    )?.fields;
    check(
      kept?.label === "latest-producer" && kept.checked === false,
      "Unmount removed or reverted the adopted producer control",
    );
    const saved = await host.saveWorld(peer.session);
    const loaded = await host.loadWorld(saved, {
      worldNames: (graph) =>
        new Map(graph.nodes.map((node) => [node.id, `restored-${node.id}`])),
    });
    worlds.push(...loaded.created.values());
    const restored = await open(loaded.root);
    const restoredRoot = rootFor(restored);
    await restoredRoot.render(
      <Entity bindTo="restored">
        <Checkbox ref={observed} />
      </Entity>,
    );
    check(observed.current, "Restored ref missing");
    const restoredState = await observed.current.read();
    check(
      restoredState.checked === false &&
        restoredState.label === "latest-producer",
      "Restore lost the saved control fields",
    );
    report.push(
      "adopted producer controls take declared values, keep the latest write on unmount and restore saved fields",
    );

    const themed = rootFor(client);
    const baseParts = contract.GuiTheme.encodeParts({
      nextSlot: 10,
      rows: new Map([[9, { part, opacity: 0.25 }]]),
    });
    check(
      (
        await peer.batch([
          contract.Entity.create(3, { symbolicId: "theme-producer" }),
          {
            kind: "insertComponent",
            entity: contract.Entity.alias(3),
            component: contract.GuiTheme.id,
            fields: [
              {
                offset: contract.GuiTheme.fields.parts.offset,
                value: { kind: "rows", value: baseParts },
              },
            ],
          },
        ])
      ).ok,
      "Producer theme rows",
    );
    await themed.render(
      <Entity bindTo="theme-producer">
        <Theme
          fields={contract.GuiTheme.patchPartsFields(9, { opacity: 0.75 })}
        />
      </Entity>,
    );
    const rowState = await entity(peer, "theme-producer");
    const rowTable = rowState.components.find(
      (component) => component.component === contract.GuiTheme.id,
    )?.fields.parts;
    check(
      rowTable &&
        typeof rowTable === "object" &&
        "rows" in rowTable &&
        rowTable.nextSlot === 10,
      "Row patch changed table identity",
    );
    check(
      rowTable.rows.get(9)?.opacity === 0.75,
      "Sparse row override was not applied",
    );
    await themed.render(
      <Entity bindTo="theme-producer">
        <Theme
          fields={contract.GuiTheme.patchPartsFields(9, { opacity: null })}
        />
      </Entity>,
    );
    const unset = (await entity(peer, "theme-producer")).components.find(
      (component) => component.component === contract.GuiTheme.id,
    )?.fields.parts;
    check(
      unset &&
        typeof unset === "object" &&
        "rows" in unset &&
        unset.rows.get(9)?.opacity === undefined,
      "Optional row unset retained a value",
    );
    // The root adopted the producer's theme, so unmount leaves the last
    // written rows in place.
    await themed.unmount();
    const keptRows = (await entity(peer, "theme-producer")).components.find(
      (component) => component.component === contract.GuiTheme.id,
    )?.fields.parts;
    check(
      keptRows &&
        typeof keptRows === "object" &&
        "rows" in keptRows &&
        keptRows.nextSlot === 10 &&
        keptRows.rows.get(9)?.opacity === undefined,
      "Unmount changed the adopted producer rows",
    );
    report.push(
      "generated row-addressed writes, optional unset and adopted rows kept on unmount",
    );

    const childWorld = await create();
    const childClient = await open(childWorld);
    const attachmentRoot = rootFor(client);
    const childRef = createRef<GuiControlHandle>();
    const attachment = createRef<AttachedWorldHandle>();
    const Label = createContext("default");
    function ChildControl() {
      return (
        <Entity id="portal-control">
          <Button label={useContext(Label)} ref={childRef} />
        </Entity>
      );
    }
    await attachmentRoot.render(
      <Label value="context">
        <Entity id="anchor" />
        <AttachedWorld
          anchor="anchor"
          child={{ borrow: childWorld }}
          attachment={{ mode: "spatial" }}
          ref={attachment}
        >
          <ChildControl />
        </AttachedWorld>
      </Label>,
    );
    check(
      childRef.current && (await childRef.current.read()).label === "context",
      "Portal lost GUI context",
    );
    // Removing the borrowed boundary deletes its child declarations without
    // destroying the child World; unmount alone would delete nothing.
    await attachmentRoot.render(null);
    await attachmentRoot.unmount();
    check(
      current(childRef) === null &&
        !(await childClient.inspect()).entities.length,
      "Borrowed portal declaration cleanup",
    );
    report.push(
      "GUI declarations in fixed-World AttachedWorld context and borrowed cleanup",
    );

    let fail = true;
    const failing = new Proxy(client, {
      get(target, property) {
        if (property === "batch")
          return (commands: Command[]) => {
            if (
              fail &&
              commands.some((command) => command.kind === "insertComponent")
            ) {
              fail = false;
              return target.batch([
                ...commands,
                {
                  kind: "delete",
                  entity: { kind: "handle", id: 0xffffffffffffffffn },
                },
              ]);
            }
            return target.batch(commands);
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const partial = rootFor(failing);
    const partialRef = createRef<GuiControlHandle>();
    await partial
      .render(
        <Entity id="partial">
          <Checkbox ref={partialRef} />
        </Entity>,
      )
      .then(
        () => {
          throw new Error("Expected partial rejection");
        },
        () => {},
      );
    check(partialRef.current === null, "Failed partial batch published ref");
    await entity(peer, "partial");
    // Removing the declaration deletes the applied prefix's entity.
    await partial.render(null);
    await partial.unmount();
    check(
      !(await peer.inspect()).entities.some(
        (item) => item.metadata.symbolicId === "partial",
      ),
      "Partial records leaked",
    );
    report.push(
      "real applied-prefix failure cleanup without guessed identities",
    );

    const suspendedRoot = rootFor(client);
    const suspended = createRef<GuiControlHandle>();
    let ready = false;
    const release = deferred<void>();
    function Pending(): ReactNode {
      if (!ready) throw release.promise;
      return (
        <Entity id="suspended">
          <Checkbox ref={suspended} />
        </Entity>
      );
    }
    await suspendedRoot.render(
      <Suspense fallback={<Fragment />}>
        <Pending />
      </Suspense>,
    );
    check(current(suspended) === null, "Suspense published speculative ref");
    ready = true;
    release.resolve();
    await suspendedRoot.render(
      <Suspense fallback={<Fragment />}>
        <Pending />
      </Suspense>,
    );
    check(suspended.current, "Suspense did not acknowledge control");
    const staleHandle = suspended.current;
    check(
      (
        await peer.batch([
          contract.GuiCheckbox.insert(
            contract.Entity.handle(staleHandle.target.entity),
            { checked: true },
          ),
        ])
      ).ok,
      "External replacement",
    );
    const terminal = await guiAction(client, staleHandle.target, {
      kind: "toggle",
    });
    check(
      !terminal.ok && terminal.error.reason === "StaleTarget",
      "Old incarnation retargeted producer replacement",
    );
    await suspendedRoot.unmount();
    report.push("Suspense and exact component replacement fences");
    await controlRefLifetimes(client, peer, contract);
    await sharedRefAcquisition(client, peer, contract);
    const scalingWorld = await create();
    const scaling = await controlRefScaling(
      await open(scalingWorld),
      await open(scalingWorld),
      contract,
      probe,
    );
    console.info("React indexed control tracking", scaling);
    const subsetWorld = await create();
    await controlRefSubsetFailure(
      await open(subsetWorld),
      await open(subsetWorld),
      probe,
    );
    report.push(
      "lifecycle refresh, identity loss, held acknowledgements and reentrant ref callbacks",
    );
    const closingClient = await open(world);
    const closedRef = createRef<GuiControlHandle>();
    const closingRoot = createRoot(closingClient, {
      onError: (error) => errors.push(error),
    });
    await closingRoot.render(
      <Entity id="session-fenced-control">
        <Checkbox ref={closedRef} />
      </Entity>,
    );
    const closedHandle = current(closedRef);
    check(closedHandle, "Closing session ref was not acknowledged");
    await closingClient.close();
    check(
      current(closedRef) === null,
      "Terminal session retained its control ref",
    );
    let closedAction = false;
    try {
      await closedHandle.action({ kind: "toggle" });
    } catch {
      closedAction = true;
    }
    check(
      closedAction,
      "A terminal control handle dispatched into a closed session",
    );
    await closingRoot.unmount().catch((error: unknown) => {
      check(
        closingClient.closure && error instanceof Error,
        "Unrelated closure cleanup failure",
      );
    });
    check(errors.length > 0, "Partial rejection was not reported");
    const openIsolated = async () => {
      const connection = await connect();
      connections.push(connection);
      const client = guiClient(await connection.openWorld(world));
      return new Proxy(client, {
        get(target, property) {
          if (property === "close")
            return async () => {
              try {
                await target.close();
              } finally {
                await connection.close();
              }
            };
          const value = Reflect.get(target, property, target);
          return typeof value === "function" ? value.bind(target) : value;
        },
      });
    };
    await controlRefReplacementBurst(openIsolated, peer, contract);
    await controlRefSubscriptionRaces(openIsolated, peer, contract);
    await controlRefCleanup(openIsolated, peer, contract);
    await controlRefGatedCleanup(openIsolated, peer, contract);
    await standaloneRefCleanupRetry(client, peer, contract);
    await reentrantRootUnmount(client, peer, contract);
    check(
      observed.current &&
        typeof (await observed.current.read()).checked === "boolean",
      "A replacement burst in another World fenced healthy controls",
    );
    return report;
  } catch (error) {
    console.error("Ordinary GUI authoring failure", error);
    throw error;
  } finally {
    for (const root of roots.toReversed()) await root.unmount();
    for (const client of clients.toReversed()) await client.close();
    for (const connection of connections.toReversed()) await connection.close();
    for (const world of worlds.toReversed()) await host.destroyWorld(world);
  }
}
