import * as React from "react";
import type {
  Client,
  Command,
  HostClientBase,
  WorldReference,
} from "@ipp/client";
import {
  AttachedWorld,
  AttachedWorldCleanupError,
  Entity,
  createRoot,
  type ReactCompositionHost,
} from "@ipp/react";
import {
  REACT_CHILD,
  REACT_ROOT,
  selectSystems,
} from "../integration/system-selections.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export async function exerciseAttachedWorldJournals(
  host: HostClientBase<Client>,
): Promise<string[]> {
  const report: string[] = [];
  for (const unknown of [false, true]) {
    const parent = await host.createWorld({
      selectedSystems: selectSystems(REACT_ROOT),
    });
    const client = await host.openWorld(parent.reference);
    let child: WorldReference | undefined;
    let failure: Error | undefined;
    const adapter: ReactCompositionHost = {
      get sessions() {
        return host.sessions;
      },
      createWorld: async (options) => {
        const created = await host.createWorld(options);
        child = created.reference;
        return created;
      },
      openWorld: async (world) => {
        const writer = await host.openWorld(world);
        if (world.id !== parent.id) return writer;
        return new Proxy(writer, {
          get(target, property) {
            if (property === "batch")
              return (operations: Command[]) => {
                if (
                  !operations.some(
                    (operation) => operation.kind === "insertComponent",
                  )
                )
                  return target.batch(operations);
                // A batch applies whole: it either answers with its outcome,
                // or the reply is lost and the outcome stays unknown.
                return target
                  .batch([
                    ...operations,
                    {
                      kind: "delete",
                      entity: { kind: "handle", id: 0xffffffffffffffffn },
                    },
                  ])
                  .then((outcome) => {
                    if (unknown)
                      throw new Error(
                        "deliberately lose a real batch reply at delivery",
                      );
                    return outcome;
                  });
              };
            const member: unknown = Reflect.get(target, property, target);
            return typeof member === "function" ? member.bind(target) : member;
          },
        });
      },
      destroyWorld: (world) => host.destroyWorld(world),
      listWorlds: () => host.listWorlds(),
      bindOutput: (world, entity, kind) => host.bindOutput(world, entity, kind),
      resolveOutput: (output) => host.resolveOutput(output),
    };
    let retained: AttachedWorldCleanupError | undefined;
    const root = createRoot(client, {
      host: adapter,
      onError: (error) => {
        if (error instanceof AttachedWorldCleanupError) retained = error;
      },
    });
    try {
      await root.render(
        React.createElement(
          React.Fragment,
          null,
          React.createElement(Entity, { id: "anchor" }),
          React.createElement(AttachedWorld, {
            anchor: "anchor",
            child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
            attachment: { mode: "spatial" },
            onError: (error) => {
              failure ??= error;
            },
          }),
        ),
      );
      check(child && failure, "Real command writer failure was not delivered");
      check(
        unknown === /lose a real batch reply/.test(failure.message),
        "The delivered failure does not match the batch fault",
      );
      // Removing the boundary cleans up what it created; unmount would
      // release it without cleanup. Cleanup either destroys the child or
      // reports its failure through the root's error callback.
      await root.render(null);
      const deadline = Date.now() + 10_000;
      while (
        !retained &&
        Date.now() < deadline &&
        (await host.listWorlds()).some((world) => world.id === child!.id)
      )
        await new Promise((resolve) => setTimeout(resolve, 20));
      const incomplete = retained !== undefined;
      check(
        incomplete === unknown,
        "Cleanup did not distinguish known prefix from unknown effects",
      );
      if (unknown)
        check(
          retained?.journal.child?.id === child.id &&
            retained.journal.parent.id === parent.id &&
            retained.journal.creatorOwned &&
            retained.journal.unknownAttachmentOutcome === failure,
          "Cleanup failure lost its externally recoverable ownership journal",
        );
      const remaining = await host.listWorlds();
      check(
        remaining.some((world) => world.id === child!.id) === unknown,
        "Unknown effects destroyed a child early, or known effects leaked it",
      );
      report.push(
        unknown
          ? "lost real batch reply retains the owned child and incomplete journal"
          : "failed batch outcome retains its real applied receipt for complete cleanup",
      );
    } finally {
      await root.unmount().catch(() => {});
      await client.close();
      await host.destroyWorld(parent.reference);
      if (child) await host.destroyWorld(child).catch(() => {});
    }
  }
  return report;
}
