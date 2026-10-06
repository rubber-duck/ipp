import { createRef } from "react";
import type { GuiWorldClient } from "@ipp/client";
import { createRoot, Entity } from "@ipp/react";
import { Checkbox, type GuiControlHandle } from "@ipp/react/gui";
import { deferred, type GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";

async function bounded<Value>(promise: Promise<Value>): Promise<Value> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Control ref lifecycle did not settle")),
          10_000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

/** The GuiCheckbox fields of `entity`, read through one entity inspection. */
export async function checkboxFields(
  client: GuiWorldClient,
  contract: GuiContract,
  entity: bigint,
) {
  const page = await client.inspectPage({
    collection: "entities",
    target: entity,
    limit: 1,
  });
  return page.entities
    .find((item) => item.id === entity)
    ?.components.find(
      (component) => component.component === contract.GuiCheckbox.id,
    )?.fields;
}

export async function controlRefLifetimes(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  const name = "ref-adopted";
  const producer = await peer.batch([
    contract.Entity.create(1, { symbolicId: name }),
    contract.GuiCheckbox.insert(contract.Entity.alias(1), { checked: false }),
  ]);
  check(producer.ok, "Ref producer setup failed");
  const lifetimeErrors: Error[] = [];
  const root = createRoot(client, {
    onError: (error) => lifetimeErrors.push(error),
  });
  let live: GuiControlHandle | null = null;
  const current = () => live;
  const invalidated = deferred<void>();
  const replaced = deferred<GuiControlHandle>();
  let first: GuiControlHandle | undefined;
  const lifetimeRef = (value: GuiControlHandle | null) => {
    live = value;
    if (!value) invalidated.resolve();
    else if (first && value.target.incarnation !== first.target.incarnation)
      replaced.resolve(value);
  };
  const tree = (label = "initial") => (
    <Entity bindTo={name}>
      <Checkbox label={label} ref={lifetimeRef} />
    </Entity>
  );
  try {
    await root.render(tree());
    const initial = current();
    check(initial, "Initial ref missing");
    first = initial;
    // An external insertion replaces the component with a new incarnation.
    check(
      (
        await peer.batch([
          contract.GuiCheckbox.insert(
            contract.Entity.handle(initial.target.entity),
            { checked: true },
          ),
        ])
      ).ok,
      "External replacement failed",
    );
    await bounded(invalidated.promise);
    let rejected = false;
    try {
      await initial.read();
    } catch {
      rejected = true;
    }
    check(rejected, "Old handle silently retargeted");
    await root.render(tree());
    await root.render(tree("changed"));
    const next = await bounded(replaced.promise);
    check(
      current() === next &&
        next.target.entity === initial.target.entity &&
        next.target.incarnation !== initial.target.incarnation,
      "Replacement did not publish a fresh exact handle",
    );
    const fields = await next.read();
    check(
      fields.checked === true && fields.label === "changed",
      "Ref and declared label did not follow the new incarnation",
    );
    check(
      lifetimeErrors.length === 0,
      `Lifecycle lifetimeErrors: ${lifetimeErrors.map((error) => error.message)}`,
    );
  } finally {
    await root.unmount();
  }
  const retained = (await peer.inspect()).entities.find(
    (entry) => entry.metadata.symbolicId === name,
  );
  check(retained, "Cleanup removed its bound entity");
  check(
    (await checkboxFields(peer, contract, retained.id))?.checked === true,
    "Cleanup removed the replacement producer component",
  );

  // A replacement while the tracking start is held publishes the newest
  // incarnation, never the baseline's.
  const arrived = deferred<void>();
  const release = deferred<void>();
  const replacedEvent = deferred<void>();
  let held = true;
  let baseline: bigint | null = null;
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "watchLifecycle")
        return async (
          ...[targets, listener]: Parameters<GuiWorldClient["watchLifecycle"]>
        ) => {
          const watch = await target.watchLifecycle(targets, (event) => {
            listener(event);
            if (
              event.kind === "event" &&
              event.observation.kind === "component" &&
              event.observation.change === "replaced"
            )
              replacedEvent.resolve();
          });
          if (held) {
            held = false;
            const lifetime = watch.baselines.find(
              (item) => item.target.kind === "component",
            )?.lifetime;
            baseline =
              lifetime?.kind === "component" ? lifetime.incarnation : null;
            arrived.resolve();
            await release.promise;
          }
          return watch;
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const heldRoot = createRoot(wrapped);
  const fresh = deferred<GuiControlHandle>();
  const observed: GuiControlHandle[] = [];
  const heldRef = (value: GuiControlHandle | null) => {
    if (value) {
      observed.push(value);
      fresh.resolve(value);
    }
  };
  const mounting = heldRoot.render(
    <Entity id="held-tracking">
      <Checkbox ref={heldRef} />
    </Entity>,
  );
  try {
    await bounded(
      Promise.race([
        arrived.promise,
        mounting.then(() => {
          throw new Error("Tracking start was not held");
        }),
      ]),
    );
    const heldEntity = (await peer.inspect()).entities.find(
      (item) => item.metadata.symbolicId === "held-tracking",
    );
    check(heldEntity, "Held control missing");
    check(baseline !== null, "Held tracking reported no incarnation");
    check(
      (
        await peer.batch([
          contract.GuiCheckbox.insert(contract.Entity.handle(heldEntity.id), {
            checked: true,
          }),
        ])
      ).ok,
      "Held tracking replacement failed",
    );
    await bounded(replacedEvent.promise);
    release.resolve();
    await mounting;
    const next = await bounded(fresh.promise);
    check(
      observed.length === 1 && next.target.incarnation !== baseline,
      "A held baseline published an obsolete ref",
    );
    check(
      (await next.read()).checked === true,
      "Held tracking did not recover exact identity",
    );
  } finally {
    release.resolve();
    await mounting.catch(() => {});
    await heldRoot.unmount();
  }

  for (const cleanup of [false, true]) {
    const root = createRoot(client);
    let closing: Promise<void> | undefined;
    let released = 0;
    let published: GuiControlHandle | undefined;
    const ref = (value: GuiControlHandle | null) => {
      if (value) {
        published = value;
        closing = root.unmount();
        if (cleanup)
          return () => {
            released++;
          };
      } else released++;
      return undefined;
    };
    await root.render(
      <Entity id={`ref-unmount-${cleanup}`}>
        <Checkbox ref={ref} />
      </Entity>,
    );
    await bounded(
      closing ?? Promise.reject(new Error("Ref callback did not run")),
    );
    check(
      published && released === 1,
      "Reentrant unmount lost or duplicated ref disposal",
    );
    let rejected = false;
    try {
      await published.read();
    } catch {
      rejected = true;
    }
    check(rejected, "Callback-unmounted handle remained live");
  }

  const errors: Error[] = [];
  const reentrant = createRoot(client, {
    onError: (error) => errors.push(error),
  });
  const replacement = createRef<GuiControlHandle>();
  let updated: Promise<void> | undefined;
  let disposed = 0;
  const ref = (value: GuiControlHandle | null) => {
    if (!value) return;
    updated = reentrant.render(
      <Entity id="reentrant-ref">
        <Checkbox ref={replacement} label="new" />
      </Entity>,
    );
    return () => {
      disposed++;
    };
  };
  try {
    await reentrant.render(
      <Entity id="reentrant-ref">
        <Checkbox ref={ref} />
      </Entity>,
    );
    await bounded(
      updated ?? Promise.reject(new Error("Reentrant update missing")),
    );
    check(
      replacement.current && disposed === 1,
      "Reentrant ref swap lost new binding or old disposer",
    );
    await reentrant.render(
      <Entity id="reentrant-ref">
        <Checkbox
          ref={() => {
            throw new Error("intentional ref failure");
          }}
        />
      </Entity>,
    );
    check(
      errors.some((error) => error.message === "intentional ref failure"),
      "Ref throw was not reported",
    );
  } finally {
    await reentrant.unmount();
  }
}

/**
 * Two roots share one tracking of a control. A replacement event held before
 * delivery leaves the second root bound to the tracked incarnation; its
 * delivery republishes both refs, and only the last user's release removes
 * the tracking.
 */
export async function sharedRefAcquisition(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  const name = "shared-pending-acquisition";
  check(
    (
      await peer.batch([
        contract.Entity.create(1, { symbolicId: name }),
        contract.GuiCheckbox.insert(contract.Entity.alias(1), {
          checked: false,
        }),
      ])
    ).ok,
    "Shared producer setup failed",
  );
  const arrived = deferred<void>();
  let deliver: (() => void) | undefined;
  let holding = true;
  let registrations = 0;
  let removals = 0;
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "watchLifecycle")
        return async (
          ...[targets, listener]: Parameters<GuiWorldClient["watchLifecycle"]>
        ) => {
          registrations++;
          const watch = await target.watchLifecycle(targets, (event) => {
            if (
              holding &&
              event.kind === "event" &&
              event.observation.kind === "component" &&
              event.observation.change === "replaced"
            ) {
              check(
                !deliver,
                "Multiple events crossed the held acquisition cut",
              );
              deliver = () => {
                holding = false;
                deliver = undefined;
                listener(event);
              };
              arrived.resolve();
            } else listener(event);
          });
          return {
            ...watch,
            removeMembers: async (
              members: readonly import("@ipp/client").LifecycleMemberId[],
            ) => {
              const cuts = await watch.removeMembers(members);
              removals++;
              return cuts;
            },
          };
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const first = createRoot(wrapped);
  const second = createRoot(wrapped);
  const firstRef = createRef<GuiControlHandle>();
  let shared: GuiControlHandle | null = null;
  const current = () => shared;
  let old: GuiControlHandle | null = null;
  const republished = deferred<GuiControlHandle>();
  try {
    await first.render(
      <Entity bindTo={name}>
        <Checkbox ref={firstRef} />
      </Entity>,
    );
    old = firstRef.current;
    check(old, "Shared initial ref missing");
    const initial = old;
    check(
      (
        await peer.batch([
          contract.GuiCheckbox.insert(
            contract.Entity.handle(initial.target.entity),
            { checked: true },
          ),
        ])
      ).ok,
      "Shared producer replacement failed",
    );
    await bounded(arrived.promise);
    await second.render(
      <Entity bindTo={name}>
        <Checkbox
          ref={(value) => {
            shared = value;
            if (
              value &&
              value.target.incarnation !== initial.target.incarnation
            )
              republished.resolve(value);
          }}
        />
      </Entity>,
    );
    const acquired = current();
    check(
      acquired && acquired.target.incarnation === initial.target.incarnation,
      "The second root did not bind the shared tracked incarnation",
    );
    check(registrations === 1, "Shared target registered twice");
    check(deliver, "Shared actual event was not held");
    deliver();
    const next = await bounded(republished.promise);
    check(
      current() === next && next.target.entity === initial.target.entity,
      "The delivered replacement did not republish the shared ref",
    );
    let stale = false;
    try {
      await acquired.read();
    } catch {
      stale = true;
    }
    check(stale, "Shared handle retargeted");
    const removed = () => removals;
    await first.unmount();
    check(removed() === 0, "Removing one user removed another user's tracking");
    await second.unmount();
    check(removed() === 1, "Shared last-user removal was not exact");
  } finally {
    deliver?.();
    await first.unmount();
    await second.unmount();
  }
}

export async function controlRefReplacementBurst(
  open: () => Promise<GuiWorldClient>,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  const client = await open();
  const sibling = await open();
  const errors: Error[] = [];
  const root = createRoot(client, { onError: (error) => errors.push(error) });
  const healthy = createRoot(sibling);
  const ref = createRef<GuiControlHandle>();
  const unaffected = createRef<GuiControlHandle>();
  const name = "burst-producer";
  check(
    (
      await peer.batch([
        contract.Entity.create(1, { symbolicId: name }),
        contract.GuiCheckbox.insert(contract.Entity.alias(1), {
          checked: false,
        }),
      ])
    ).ok,
    "Burst producer setup failed",
  );
  const tree = (label = "before") => (
    <Entity bindTo={name}>
      <Checkbox ref={ref} label={label} />
    </Entity>
  );
  try {
    await root.render(tree());
    await healthy.render(
      <Entity id="healthy-burst-control">
        <Checkbox ref={unaffected} />
      </Entity>,
    );
    const initial = ref.current;
    check(initial && unaffected.current, "Burst fixture initial refs missing");
    // More than 128 replacements in one frame stay within the connection's byte budget:
    // every lifecycle record is delivered and tracking follows the last incarnation.
    check(
      (
        await peer.batch(
          Array.from({ length: 130 }, (_, index) =>
            contract.GuiCheckbox.insert(
              contract.Entity.handle(initial.target.entity),
              { checked: index % 2 === 1 },
            ),
          ),
        )
      ).ok,
      "Observed replacement burst failed",
    );
    await bounded(client.inspectPage());
    await root.flush();
    await root.render(tree("changed"));
    const current = ref.current;
    check(
      !client.closure &&
        errors.length === 0 &&
        current &&
        current.target.incarnation !== initial.target.incarnation,
      "Replacement burst lost tracking or closed the connection",
    );
    check(
      (await current.read()).checked === true,
      "Tracked ref did not follow the last replacement",
    );
    let stale = false;
    try {
      await initial.read();
    } catch {
      stale = true;
    }
    check(stale, "Replacement burst silently retargeted a retained handle");
    check(
      !peer.closure &&
        !sibling.closure &&
        unaffected.current &&
        typeof (await unaffected.current.read()).checked === "boolean",
      "Replacement burst disturbed a healthy physical connection",
    );
  } finally {
    await root.unmount();
    await healthy.unmount();
    await Promise.all([client.close(), sibling.close()]);
  }
}

export async function controlRefSubscriptionRaces(
  open: () => Promise<GuiWorldClient>,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  for (const interruption of ["unmount", "session-close"] as const) {
    const client = await open();
    const arrived = deferred<void>();
    const release = deferred<void>();
    let removed = 0;
    const wrapped = new Proxy(client, {
      get(target, property) {
        if (property === "watchLifecycle")
          return async (
            ...args: Parameters<GuiWorldClient["watchLifecycle"]>
          ) => {
            const watch = await target.watchLifecycle(...args);
            arrived.resolve();
            await release.promise;
            return {
              ...watch,
              removeMembers: async (
                members: readonly import("@ipp/client").LifecycleMemberId[],
              ) => {
                const cuts = await watch.removeMembers(members);
                removed++;
                return cuts;
              },
            };
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const root = createRoot(wrapped, { onError: () => {} });
    const ref = createRef<GuiControlHandle>();
    const name = "subscribe-" + interruption;
    const rendering = root
      .render(
        <Entity id={name}>
          <Checkbox ref={ref} />
        </Entity>,
      )
      .then(
        () => true,
        () => false,
      );
    let cleanup: Promise<void> | undefined;
    try {
      await bounded(arrived.promise);
      if (interruption === "unmount") cleanup = root.unmount();
      else await client.close();
      release.resolve();
      const rendered = await bounded(rendering);
      if (interruption !== "unmount")
        check(!rendered, "Late membership ACK revived closed tracking");
      check(ref.current === null, "Late membership ACK published a ref");
      cleanup ??= root.unmount();
      await bounded(cleanup).catch((error: unknown) => {
        check(
          client.closure && error instanceof Error,
          "Unexpected live-session cleanup failure",
        );
      });
      check(
        removed === (interruption === "unmount" ? 1 : 0),
        "Membership cleanup did not match exact endpoint lifetime",
      );
      // Unmount and a session end delete nothing: the authored entity stays
      // for other clients whatever interrupted the membership.
      const retained = (await peer.inspect()).entities.find(
        (entry) => entry.metadata.symbolicId === name,
      );
      check(
        retained,
        `Interrupted membership deleted its entity (${interruption})`,
      );
      check(
        (
          await peer.batch([
            contract.Entity.delete(contract.Entity.handle(retained.id)),
          ])
        ).ok,
        "Retained authored entity could not be deleted",
      );
    } finally {
      release.resolve();
      await rendering;
      await root.unmount().catch((error: unknown) => {
        check(
          client.closure && error instanceof Error,
          "Membership race cleanup failed",
        );
      });
      await client.close();
    }
  }

  for (const phase of ["before-send", "after-ack"] as const) {
    const client = await open();
    const name = "held-remove-" + phase;
    const arrived = deferred<void>();
    const release = deferred<void>();
    let removed = 0;
    const wrapped = new Proxy(client, {
      get(target, property) {
        if (property === "watchLifecycle")
          return async (
            ...args: Parameters<GuiWorldClient["watchLifecycle"]>
          ) => {
            const watch = await target.watchLifecycle(...args);
            return {
              ...watch,
              removeMembers: async (
                members: readonly import("@ipp/client").LifecycleMemberId[],
              ) => {
                const acknowledged =
                  phase === "after-ack"
                    ? await watch.removeMembers(members)
                    : undefined;
                arrived.resolve();
                await release.promise;
                const cuts =
                  acknowledged ?? (await watch.removeMembers(members));
                removed++;
                return cuts;
              },
            };
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const root = createRoot(wrapped, { onError: () => {} });
    const ref = createRef<GuiControlHandle>();
    try {
      check(
        (
          await peer.batch([
            contract.Entity.create(1, { symbolicId: name }),
            contract.GuiCheckbox.insert(contract.Entity.alias(1), {
              checked: false,
            }),
          ])
        ).ok,
        "Remove producer setup failed",
      );
      await root.render(
        <Entity bindTo={name}>
          <Checkbox ref={ref} />
        </Entity>,
      );
      const initial = ref.current;
      check(initial, "Remove fixture ref missing");
      const cleanup = root.unmount();
      await bounded(arrived.promise);
      check(ref.current === null, "Held removal retained local ref");
      check(
        (
          await peer.batch([
            contract.GuiCheckbox.insert(
              contract.Entity.handle(initial.target.entity),
              { checked: true },
            ),
          ])
        ).ok,
        "Replacement during removal failed",
      );
      let stale = false;
      try {
        await initial.read();
      } catch {
        stale = true;
      }
      check(
        stale && ref.current === null,
        "Handed-off prefix event revived disposed ref",
      );
      release.resolve();
      await bounded(cleanup);
      check(removed === 1, "Removal did not settle exactly once");
      check(
        (await checkboxFields(peer, contract, initial.target.entity))
          ?.checked === true,
        "Cleanup removed the adopted producer replacement",
      );
    } finally {
      release.resolve();
      await root.unmount();
      await client.close();
    }
  }
}
