import * as React from "react";
import type { Client, HostClientBase } from "@ipp/client";
import {
  AttachedWorld,
  AttachedWorldCleanupError,
  Entity,
  createRoot,
  type AttachedWorldHandle,
  type AttachedWorldCleanupRecovery,
} from "@ipp/react";
import {
  REACT_CHILD,
  REACT_ROOT,
  selectSystems,
} from "../integration/system-selections.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export async function exerciseAttachedWorldClosures(
  connect: () => Promise<HostClientBase<Client>>,
  observer?: HostClientBase<Client>,
): Promise<string[]> {
  const report: string[] = [];
  for (const temporary of [false, true]) {
    const host = await connect();
    const parent = await host.createWorld({
      selectedSystems: selectSystems(REACT_ROOT),
    });
    const client = await host.openWorld(parent.reference);
    let recovery: AttachedWorldCleanupRecovery | undefined;
    const root = createRoot(client, {
      host,
      onError: (error) => {
        if (error instanceof AttachedWorldCleanupError)
          recovery = error.recovery;
      },
    });
    const reference = React.createRef<AttachedWorldHandle>();
    let ready = 0;
    let child: AttachedWorldHandle | undefined;
    let childWorld: AttachedWorldHandle["world"] | undefined;
    try {
      await root.render(
        React.createElement(
          React.Fragment,
          null,
          React.createElement(Entity, { id: "anchor" }),
          React.createElement(AttachedWorld, {
            anchor: "anchor",
            child: {
              create: {
                selectedSystems: selectSystems(REACT_CHILD),
                temporary,
              },
            },
            attachment: { mode: "spatial" },
            ref: reference,
            onReady: () => {
              ready++;
            },
          }),
        ),
      );
      child = reference.current!;
      childWorld = child.world;
      // A session end deletes nothing: the boundary is released without
      // cleanup, so there is nothing incomplete to recover.
      let incomplete = false;
      void child.closed.catch(() => {
        incomplete = true;
      });
      await host.close();
      await child.closed.catch(() => {});
      await root.unmount().catch(() => {});
      check(
        !incomplete && reference.current === null && ready === 1,
        "Terminal connection closure did not fence the boundary or reported cleanup it does not perform",
      );
      let fenced = false;
      try {
        void child.world;
      } catch {
        fenced = true;
      }
      check(fenced, "A retained handle survived its terminal session fence");
      check(
        recovery === undefined,
        "Terminal connection closure reported a cleanup failure",
      );
      if (observer) {
        const worlds = await observer.listWorlds();
        check(
          worlds.some((world) => world.id === childWorld!.id) === !temporary,
          "Boundary invented a different creator disconnect policy",
        );
        check(
          worlds.some((world) => world.id === parent.id),
          "Connection closure was confused with parent World destruction",
        );
      }
      report.push(
        observer
          ? `terminal connection closure releases the boundary and preserves temporary=${temporary} Host policy`
          : `terminal worker closure fences callbacks without asserting World destruction, temporary=${temporary}`,
      );
    } finally {
      await host.close();
      await root.unmount().catch(() => {});
      if (observer && childWorld)
        await observer.destroyWorld(childWorld).catch(() => {});
      await observer?.destroyWorld(parent.reference);
    }
  }
  const host = await connect();
  const parent = await host.createWorld({
    selectedSystems: selectSystems(REACT_ROOT),
  });
  const client = await host.openWorld(parent.reference);
  const root = createRoot(client, { host, onError: () => {} });
  const reference = React.createRef<AttachedWorldHandle>();
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
          ref: reference,
        }),
      ),
    );
    const child = reference.current!;
    const world = child.world;
    await client.close();
    await child.closed;
    // The authoring session's end releases the boundary; like unmount, it
    // destroys no World, not even the child the boundary created.
    const worlds = await host.listWorlds();
    check(
      worlds.some((item) => item.id === world.id) &&
        worlds.some((item) => item.id === parent.id),
      "Authoring-session close destroyed the parent or the created child World",
    );
    await host.destroyWorld(world);
    report.push(
      "authoring-session close releases the boundary and destroys no World",
    );
  } finally {
    await root.unmount().catch(() => {});
    await host.destroyWorld(parent.reference);
    await host.close();
  }
  return report;
}
