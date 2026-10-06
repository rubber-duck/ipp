import { createRef } from "react";
import type {
  Command,
  GuiWorldClient,
  LifecycleTargetWatch,
} from "@ipp/client";
import { createRoot, Entity } from "@ipp/react";
import { Checkbox, type GuiControlHandle } from "@ipp/react/gui";
import { deferred, type GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";
import { checkboxFields } from "./control-ref-lifetimes.js";

async function bounded<Value>(promise: Promise<Value>): Promise<Value> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Control cleanup did not settle")),
          10_000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

export async function controlRefCleanup(
  open: () => Promise<GuiWorldClient>,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  for (const partial of [false, true]) {
    const client = await open();
    const name = `remove-rejected-${partial}`;
    const ref = createRef<GuiControlHandle>();
    let rejectRemoval = true;
    let removals = 0;
    let rejected = 0;
    let failCommit = false;
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
                if (rejectRemoval) {
                  rejected++;
                  throw Object.assign(new Error("Removal was not submitted"), {
                    code: "IPP_REQUEST_NOT_SENT",
                  });
                }
                const cuts = await watch.removeMembers(members);
                removals++;
                return cuts;
              },
            };
          };
        if (property === "batch")
          return (commands: Command[]) => {
            if (failCommit) {
              failCommit = false;
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
    const root = createRoot(wrapped, { onError: () => {} });
    const tree = (label: string) => (
      <Entity id={name}>
        <Checkbox ref={ref} label={label} />
      </Entity>
    );
    try {
      await root.render(tree("initial"));
      const initial = ref.current;
      check(initial, "Cleanup initial ref missing");
      if (partial) {
        failCommit = true;
        const result = await root.render(tree("applied-prefix")).then(
          () => true,
          () => false,
        );
        check(!result, "Partial authoring failure did not reject");
        check(
          (await checkboxFields(peer, contract, initial.target.entity))
            ?.label === "applied-prefix",
          "Failed source page did not apply its known prefix",
        );
      }
      const cleared = await bounded(
        root.render(null).then(
          () => true,
          () => false,
        ),
      );
      check(!cleared && rejected > 0, "Removal rejection was hidden");
      check(ref.current === null, "Removal rejection retained a local ref");
      let stale = false;
      try {
        await initial.read();
      } catch {
        stale = true;
      }
      check(stale, "Rejected removal left the old handle usable");
      check(
        !(await peer.inspect()).entities.some(
          (entry) => entry.id === initial.target.entity,
        ),
        "Observer removal failure skipped acknowledged owned cleanup",
      );
      check(
        !client.closure,
        "Single rejected removal closed the authoring session",
      );
      rejectRemoval = false;
      await bounded(root.unmount());
      check(
        removals === 1,
        "Retained membership cleanup did not retry exactly once",
      );
      console.info("React rejected-removal cleanup", {
        partial,
        rejected,
        removals,
      });
    } finally {
      rejectRemoval = false;
      await bounded(root.unmount());
      await client.close();
    }
  }

  const client = await open();
  let watch: LifecycleTargetWatch | undefined;
  const fenced = deferred<void>();
  let live: GuiControlHandle | null = null;
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "watchLifecycle")
        return async (
          ...args: Parameters<GuiWorldClient["watchLifecycle"]>
        ) => {
          watch = await target.watchLifecycle(...args);
          return watch;
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(wrapped, { onError: () => {} });
  try {
    await root.render(
      <Entity id="tracking-ended-cleanup">
        <Checkbox
          ref={(value) => {
            live = value;
            if (!value) fenced.resolve();
          }}
        />
      </Entity>,
    );
    const initial = live as GuiControlHandle | null;
    check(initial && watch, "Ended tracking fixture was not ready");
    await watch.remove();
    await bounded(fenced.promise);
    check(
      live === null && !client.closure,
      "Tracking loss did not fence only the ref scope",
    );
    await bounded(root.render(null));
    check(
      !(await peer.inspect()).entities.some(
        (entry) => entry.id === initial.target.entity,
      ),
      "Ended observer prevented owned cleanup",
    );
    await root.render(<Entity id="after-tracking-ended" />);
    check(
      (await peer.inspect()).entities.some(
        (entry) => entry.metadata.symbolicId === "after-tracking-ended",
      ),
      "Observer availability became an ordinary Core mutation requirement",
    );
    console.info("React ended-tracking owned cleanup completed");
  } finally {
    await bounded(root.unmount());
    await client.close();
  }
}

export async function controlRefGatedCleanup(
  open: () => Promise<GuiWorldClient>,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  for (const closeSession of [false, true]) {
    const client = await open();
    const name = `gated-ref-cleanup-${closeSession}`;
    const entered = deferred<void>();
    const finish = deferred<void>();
    const removing = deferred<void>();
    const ref = createRef<GuiControlHandle>();
    let gateNext = false;
    let removalAcks = 0;
    const wrapped = new Proxy(client, {
      get(target, property) {
        if (property === "batch")
          return async (commands: Command[]) => {
            if (!gateNext) return target.batch(commands);
            gateNext = false;
            // Withhold the applied outcome from React; batches never hold the World.
            const result = await target.batch(commands);
            entered.resolve();
            await finish.promise;
            return result;
          };
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
                removing.resolve();
                const cuts = await watch.removeMembers(members);
                removalAcks++;
                return cuts;
              },
            };
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const root = createRoot(wrapped, { onError: () => {} });
    const tree = (label: string) => (
      <Entity id={name}>
        <Checkbox ref={ref} label={label} />
      </Entity>
    );
    let updating: Promise<boolean> | undefined;
    let clearing: Promise<boolean> | undefined;
    try {
      await root.render(tree("initial"));
      const initial = ref.current;
      check(initial, "Gated cleanup initial ref missing");
      gateNext = true;
      updating = root.render(tree("updated")).then(
        () => true,
        () => false,
      );
      await bounded(entered.promise);
      clearing = root.render(null).then(
        () => true,
        () => false,
      );
      await bounded(removing.promise);
      check(ref.current === null, "A World gate delayed local ref fencing");
      let stale = false;
      try {
        await initial.read();
      } catch {
        stale = true;
      }
      check(stale, "Gated old handle remained usable");
      if (closeSession) await bounded(client.close());
      finish.resolve();
      await bounded(updating);
      const cleared = await bounded(clearing);
      check(
        cleared !== closeSession,
        "Gated cleanup outcome lost session authority",
      );
      if (!closeSession)
        check(removalAcks === 1, "Gated membership removal did not settle");
      const retained = (await peer.inspect()).entities.some(
        (entry) => entry.id === initial.target.entity,
      );
      if (closeSession) {
        // Nothing is deleted when a session ends: the clearing render could
        // not delete after the close, so the entity stays for other clients.
        check(retained, "Session end deleted an authored entity");
        check(
          (
            await peer.batch([
              contract.Entity.delete(
                contract.Entity.handle(initial.target.entity),
              ),
            ])
          ).ok,
          "Retained authored entity could not be deleted",
        );
      } else
        check(!retained, "Gated cleanup leaked its acknowledged owned entity");
      await bounded(root.unmount()).catch((error: unknown) => {
        check(
          closeSession && error instanceof Error,
          "Live gated cleanup failed",
        );
      });
      check(
        (
          await peer.batch([
            contract.Entity.create(1, { symbolicId: name + "-peer" }),
          ])
        ).ok,
        "Session loss blocked the healthy peer",
      );
      console.info("React gated removal cleanup", {
        closeSession,
        removalAcks,
        cleared,
      });
    } finally {
      finish.resolve();
      await updating;
      await clearing;
      await bounded(root.unmount()).catch((error: unknown) => {
        check(
          client.closure && error instanceof Error,
          "Gated cleanup did not recover",
        );
      });
      await client.close();
    }
  }
}

export async function standaloneRefCleanupRetry(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  const name = "standalone-cleanup-retry";
  check(
    (
      await peer.batch([
        contract.Entity.create(1, { symbolicId: name }),
        contract.GuiCheckbox.insert(contract.Entity.alias(1), {
          checked: false,
        }),
        contract.Entity.create(2, { symbolicId: name + "-healthy" }),
        contract.GuiCheckbox.insert(contract.Entity.alias(2), {
          checked: false,
        }),
      ])
    ).ok,
    "Standalone cleanup producers failed",
  );
  const producer = (await peer.inspect()).entities.find(
    (entry) => entry.metadata.symbolicId === name,
  );
  check(producer, "Standalone producer missing");
  const ref = createRef<GuiControlHandle>();
  const healthyRef = createRef<GuiControlHandle>();
  const removalEntered = deferred<void>();
  const removalRelease = deferred<void>();
  let rejectRemoval = true;
  let ownedWatch: LifecycleTargetWatch | undefined;
  let registrations = 0;
  let removals = 0;
  let batches = 0;
  const counts = () => ({ registrations, removals, batches });
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "watchLifecycle")
        return async (
          ...args: Parameters<GuiWorldClient["watchLifecycle"]>
        ) => {
          registrations++;
          const watch = await target.watchLifecycle(...args);
          if (!args[0].some((entry) => entry.target.entity === producer.id))
            return watch;
          ownedWatch = watch;
          return {
            ...watch,
            removeMembers: async (
              members: readonly import("@ipp/client").LifecycleMemberId[],
            ) => {
              if (rejectRemoval)
                throw Object.assign(new Error("Standalone removal not sent"), {
                  code: "IPP_REQUEST_NOT_SENT",
                });
              removalEntered.resolve();
              await removalRelease.promise;
              const cuts = await watch.removeMembers(members);
              removals++;
              return cuts;
            },
          };
        };
      if (property === "batch")
        return (...args: Parameters<GuiWorldClient["batch"]>) => {
          batches++;
          return target.batch(...args);
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(wrapped, { onError: () => {} });
  const healthy = createRoot(wrapped);
  try {
    await root.render(
      <Entity bindTo={name}>
        <Checkbox ref={ref} label="temporary override" />
      </Entity>,
    );
    await healthy.render(
      <Entity bindTo={name + "-healthy"}>
        <Checkbox ref={healthyRef} />
      </Entity>,
    );
    const initial = ref.current;
    const sibling = healthyRef.current;
    check(initial && sibling, "Standalone cleanup refs were not ready");
    const first = root.unmount();
    check(root.unmount() === first, "Inflight first unmount did not coalesce");
    const originalError = await bounded(
      first.then(
        () => null,
        (error: unknown) => error,
      ),
    );
    check(
      originalError instanceof Error,
      "First standalone cleanup failure was hidden",
    );
    check(
      ref.current === null && !client.closure && !peer.closure,
      "Failed root cleanup affected shared sessions or kept its ref",
    );
    // The component was adopted, so the written label stays.
    check(
      (await checkboxFields(peer, contract, producer.id))?.label ===
        "temporary override",
      "Failed unmount reverted or removed the adopted control",
    );
    let stale = false;
    try {
      await initial.read();
    } catch {
      stale = true;
    }
    check(stale, "First failed unmount did not fence retained handles");
    await root.render(<Entity id="forbidden-after-unmount" />).then(
      () => {
        throw new Error("Failed unmount reopened authoring");
      },
      () => {},
    );
    check(
      (await sibling.action({ kind: "toggle" })).ok,
      "Failed cleanup fenced the healthy root",
    );
    const beforeRetry = counts();
    rejectRemoval = false;
    const retry = root.unmount();
    const retried = retry.then(
      () => true,
      () => false,
    );
    check(
      retry !== first,
      "Standalone unmount permanently cached failed cleanup",
    );
    await bounded(removalEntered.promise);
    check(
      root.unmount() === retry,
      "Inflight cleanup retries did not coalesce",
    );
    check(
      ref.current === null && healthyRef.current === sibling,
      "Cleanup retry republished or fenced a foreign ref",
    );
    removalRelease.resolve();
    check(
      await bounded(retried),
      "Standalone cleanup retry failed after removal recovered",
    );
    check(
      (await first.then(
        () => null,
        (error: unknown) => error,
      )) === originalError,
      "Cleanup retry replaced the first failure result",
    );
    check(
      root.unmount() === retry,
      "Successful cleanup was needlessly repeated",
    );
    const afterRetry = counts();
    check(
      afterRetry.registrations === beforeRetry.registrations &&
        afterRetry.batches === beforeRetry.batches &&
        afterRetry.removals === beforeRetry.removals + 1,
      "Retry reauthored instead of releasing only retained cleanup",
    );
    check(
      (await checkboxFields(peer, contract, producer.id))?.label ===
        "temporary override",
      "Cleanup retry destroyed or replaced the adopted producer control",
    );
    check(
      !client.closure &&
        !peer.closure &&
        typeof (await sibling.read()).checked === "boolean",
      "Cleanup retry closed shared sessions",
    );
    console.info("React standalone unmount cleanup retry", {
      beforeRetry,
      afterRetry,
    });
  } finally {
    rejectRemoval = false;
    removalRelease.resolve();
    await root.unmount().catch(() => {});
    await ownedWatch?.remove();
    await healthy.unmount();
  }
}

export async function reentrantRootUnmount(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  for (const kind of ["null", "disposer"] as const) {
    const name = `reentrant-unmount-${kind}`;
    check(
      (
        await peer.batch([
          contract.Entity.create(1, { symbolicId: name }),
          contract.GuiCheckbox.insert(contract.Entity.alias(1), {
            checked: false,
          }),
        ])
      ).ok,
      "Reentrant producer setup failed",
    );
    let callbacks = 0;
    let deletions = 0;
    const wrapped = new Proxy(client, {
      get(target, property) {
        if (property === "batch")
          return (...args: Parameters<GuiWorldClient["batch"]>) => {
            deletions += args[0].filter(
              (operation) => operation.kind === "delete",
            ).length;
            return target.batch(...args);
          };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
    const root = createRoot(wrapped, { onError: () => {} });
    let nested: Promise<void> | undefined;
    let attemptedRender: Promise<boolean> | undefined;
    const onRemoval = () => {
      callbacks++;
      if (callbacks !== 1) return;
      nested = root.unmount();
      void nested.catch(() => {});
      attemptedRender = root.render(<Entity id={name + "-forbidden"} />).then(
        () => true,
        () => false,
      );
    };
    try {
      await root.render(
        <>
          <Entity bindTo={name}>
            <Checkbox
              ref={(value) => {
                if (value && kind === "disposer") return onRemoval;
                if (!value && kind === "null") onRemoval();
                return undefined;
              }}
            />
          </Entity>
          <Entity id={name + "-declared"} />
        </>,
      );
      const closing = root.unmount();
      void closing.catch(() => {});
      check(callbacks === 1, "Unmount deferred local ref cleanup");
      check(
        nested === closing && root.unmount() === closing,
        "Reentrant teardown did not share the published cleanup attempt",
      );
      check(
        attemptedRender && !(await attemptedRender),
        "Reentrant callback authored after unmount",
      );
      await bounded(closing);
      // Unmount deletes nothing, even when reentered from a ref callback.
      check(deletions === 0, "Reentrant unmount sent deletes");
      const entities = (await peer.inspect()).entities;
      const declared = entities.find(
        (entity) => entity.metadata.symbolicId === name + "-declared",
      );
      check(
        entities.some((entity) => entity.metadata.symbolicId === name) &&
          declared &&
          !entities.some(
            (entity) => entity.metadata.symbolicId === name + "-forbidden",
          ),
        "Reentrant unmount deleted an entity or admitted new authoring",
      );
      check(
        (
          await peer.batch([
            contract.Entity.delete(contract.Entity.handle(declared.id)),
          ])
        ).ok,
        "Retained declared entity could not be deleted",
      );
      console.info("React reentrant unmount", {
        kind,
        callbacks,
        deletions,
      });
    } finally {
      await root.unmount().catch(() => {});
      await nested?.catch(() => {});
      await attemptedRender;
    }
  }
}
