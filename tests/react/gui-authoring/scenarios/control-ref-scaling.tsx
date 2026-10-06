import { createRef } from "react";
import type { GuiWorldClient } from "@ipp/client";
import { lifecycleDiagnostics } from "@ipp/client/diagnostics";
import { createRoot, Entity } from "@ipp/react";
import { Checkbox, type GuiControlHandle } from "@ipp/react/gui";
import { deferred, type GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";
import type { LifecycleTransportProbe } from "../../../runtime/drivers/browser-lifecycle-targets.js";

export async function controlRefScaling(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  contract: GuiContract,
  probe: LifecycleTransportProbe,
) {
  const refs = Array.from({ length: 140 }, () => createRef<GuiControlHandle>());
  let registrations = 0;
  let events = 0;
  let publications = 0;
  let removals = 0;
  let addedPages = 0;
  let removedPages = 0;
  const removedCount = () => removals;
  let output: bigint | undefined;
  let changed = deferred<void>();
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "watchLifecycle")
        return async (
          ...[targets, listener]: Parameters<GuiWorldClient["watchLifecycle"]>
        ) => {
          registrations++;
          const watch = await target.watchLifecycle(targets, (event) => {
            events++;
            listener(event);
          });
          output ??= watch.baselines[0]?.member.output;
          addedPages += watch.cuts.length;
          return {
            ...watch,
            removeMembers: async (
              members: readonly import("@ipp/client").LifecycleMemberId[],
            ) => {
              const cuts = await watch.removeMembers(members);
              removals++;
              removedPages += cuts.length;
              return cuts;
            },
          };
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(wrapped);
  // Stable callback refs: a new callback on every render would be detached
  // and attached again by React itself, which is not a ref publication.
  const callbacks = refs.map((ref) => (value: GuiControlHandle | null) => {
    ref.current = value;
    if (value) {
      publications++;
      changed.resolve();
    }
  });
  const tree = (count: number) =>
    callbacks.slice(0, count).map((callback, index) => (
      <Entity key={index} id={`indexed-control-${index}`}>
        <Checkbox ref={callback} />
      </Entity>
    ));
  const observations: string[] = [];
  try {
    for (const count of [70, 140]) {
      console.info("Indexed refs mount", {
        count,
        registrations,
        publications,
      });
      await root.render(tree(count));
      console.info("Indexed refs mounted", {
        count,
        registrations,
        publications,
      });
      check(
        refs.slice(0, count).every((ref) => ref.current),
        "Large mount omitted acknowledged refs",
      );
      check(
        registrations === count / 70 && publications === count,
        "Large mount repeated healthy tracking or ref publications",
      );
      check(output !== undefined, "Diagnostic watch endpoint missing");
      const before = await lifecycleDiagnostics(client).statistics(output);
      const beforeWire = probe.records(client.session);
      const initialEvents = events;
      check(
        (
          await peer.batch([
            contract.Entity.create(1, { symbolicId: `unrelated-${count}` }),
          ])
        ).ok,
        "Unrelated producer creation failed",
      );
      const after = await lifecycleDiagnostics(client).statistics(output);
      const afterWire = probe.records(client.session);
      check(
        afterWire.messages === beforeWire.messages &&
          afterWire.bytes === beforeWire.bytes,
        "Unrelated transitions produced lifecycle wire delivery",
      );
      check(
        !before.work.saturated &&
          !after.work.saturated &&
          !before.traffic.saturated &&
          !after.traffic.saturated,
        "Saturated diagnostic counters cannot prove exact work",
      );
      check(
        before.output === after.output &&
          before.world.id === after.world.id &&
          before.world.incarnation === after.world.incarnation,
        "Diagnostic lifetime changed",
      );
      check(
        after.work.lookups - before.work.lookups === 1n &&
          after.work.recipientVisits === before.work.recipientVisits &&
          after.traffic.queuedEvents === before.traffic.queuedEvents &&
          after.traffic.queuedBytes === before.traffic.queuedBytes,
        "Unrelated creation performed recipient work or charged output",
      );
      await root.flush();
      await root.render(tree(count));
      check(
        events === initialEvents &&
          publications === count &&
          registrations === count / 70,
        "Unrelated producer or unchanged render touched control refs",
      );
      observations.push(
        `${count} refs: unrelated lookup=1 candidates=0 events=0 bytes=0 publications=0`,
      );
      console.info("Indexed unrelated work", observations.at(-1));
    }
    const first = refs[0]!.current!;
    changed = deferred<void>();
    const previousPublications = publications;
    const previousEvents = events;
    check(
      (
        await peer.batch([
          contract.GuiCheckbox.insert(
            contract.Entity.handle(first.target.entity),
            { checked: true },
          ),
        ])
      ).ok,
      "Indexed replacement failed",
    );
    await changed.promise;
    await root.flush();
    check(
      events === previousEvents + 1 &&
        publications === previousPublications + 1,
      "One target replacement rescanned unrelated controls",
    );
    check(
      refs[0]!.current?.target.incarnation !== first.target.incarnation,
      "Auto replacement did not refresh exact handle",
    );
    await first.read().then(
      () => {
        throw new Error("Retired indexed handle remained usable");
      },
      () => {},
    );
    console.info("Indexed replacement", {
      registrations,
      publications,
      events,
    });
    const sibling = createRoot(wrapped);
    const shared = createRef<GuiControlHandle>();
    try {
      await sibling.render(
        <Entity bindTo="indexed-control-0">
          <Checkbox ref={shared} />
        </Entity>,
      );
      check(
        shared.current && registrations === 2,
        "Shared target allocated duplicate tracking",
      );
      await sibling.unmount();
      check(
        removedCount() === 0 && refs[0]!.current,
        "Shared target removed before its last user",
      );
    } finally {
      await sibling.unmount();
    }
    const removedHandle = refs[139]!.current!;
    const retainedHandle = refs[69]!.current!;
    const beforeSubset = publications;
    await root.render(tree(100));
    check(
      refs.slice(100).every((ref) => ref.current === null) &&
        refs[69]!.current === retainedHandle &&
        removedCount() === 1,
      "Subset removal changed retained refs or missed its last users",
    );
    await removedHandle.read().then(
      () => {
        throw new Error("Removed subset retained a usable handle");
      },
      () => {},
    );
    await root.render(tree(140));
    check(
      refs.every((ref) => ref.current) &&
        publications === beforeSubset + 40 &&
        refs[69]!.current === retainedHandle &&
        refs[139]!.current !== removedHandle,
      "Re-added subset reacquired unrelated targets or reused an old handle",
    );
    await root.unmount();
    console.info("Indexed refs removed", {
      registrations,
      publications,
      removals,
    });
    check(
      removedCount() === 4 && refs.every((ref) => ref.current === null),
      "Last-user cleanup leaked indexed tracking",
    );
    const requests = probe.requests(client.session);
    check(
      requests.adds === addedPages && requests.removes === removedPages,
      "Wire request counts disagree with acknowledged membership pages",
    );
    check(
      requests.adds === 3 && requests.removes === 4,
      "140 refs and subset re-add required per-ref wire roundtrips",
    );
    observations.push(
      `140 refs: groups=${registrations} addRequests=${requests.adds} removeRequests=${requests.removes}`,
    );
    return observations;
  } finally {
    await root.unmount();
  }
}
