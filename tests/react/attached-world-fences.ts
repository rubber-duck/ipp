import * as React from "react";
import {
  outputProducer,
  type Client,
  type Command,
  type HostClientBase,
} from "@ipp/client";
import {
  AttachedWorld,
  Camera,
  Entity,
  FlatSurface,
  createRoot,
  type AttachedWorldHandle,
  type ReactCompositionHost,
} from "@ipp/react";
import { requireSuccess } from "./fixture-helpers.js";
import {
  REACT_CHILD,
  REACT_ROOT,
  selectSystems,
} from "../integration/system-selections.js";

function gate() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export async function exerciseAttachedWorldFences(
  host: HostClientBase<Client>,
): Promise<string[]> {
  const report: string[] = [];
  for (const phase of ["writer-open", "anchor-retirement"] as const) {
    for (const replacement of ["supersede", "unmount"] as const) {
      const parent = (
        await host.createWorld({ selectedSystems: selectSystems(REACT_ROOT) })
      ).reference;
      const child = (
        await host.createWorld({ selectedSystems: selectSystems(REACT_CHILD) })
      ).reference;
      const client = await host.openWorld(parent);
      const created = await requireSuccess(
        client.batch(
          [0, 1, 2].map((alias) => ({
            kind: "create",
            alias,
            metadata: { symbolicId: null, classes: [] },
          })),
        ),
      );
      const anchors = created.aliases.map((alias) => alias.id);
      const arrived = gate();
      const release = gate();
      const writes: bigint[] = [];
      const errors: Error[] = [];
      const adapter: ReactCompositionHost = {
        get sessions() {
          return host.sessions;
        },
        createWorld: (options) => host.createWorld(options),
        destroyWorld: (world) => host.destroyWorld(world),
        listWorlds: () => host.listWorlds(),
        bindOutput: (world, entity, kind) =>
          host.bindOutput(world, entity, kind),
        resolveOutput: (output) => host.resolveOutput(output),
        openWorld: async (world) => {
          const opened = await host.openWorld(world);
          if (world.id !== parent.id) return opened;
          if (phase === "writer-open") {
            arrived.release();
            await release.promise;
          }
          return new Proxy(opened, {
            get(target, property) {
              if (property === "batch")
                return (commands: Command[]) => {
                  for (const command of commands)
                    if (
                      command.kind === "insertComponent" &&
                      command.component ===
                        target.components.WorldAttachment!.id &&
                      command.entity.kind === "handle"
                    )
                      writes.push(command.entity.id);
                  return target.batch(commands);
                };
              if (
                property === "attachmentRetirement" &&
                phase === "anchor-retirement"
              )
                return async (receipt: bigint) => {
                  const result = await target.attachmentRetirement(receipt);
                  arrived.release();
                  await release.promise;
                  return result;
                };
              const member: unknown = Reflect.get(target, property, target);
              return typeof member === "function"
                ? member.bind(target)
                : member;
            },
          });
        },
      };
      const root = createRoot(client, {
        host: adapter,
        onError: (error) => {
          errors.push(error);
        },
      });
      const handle = React.createRef<AttachedWorldHandle>();
      const scene = (ordinal: number) =>
        React.createElement(AttachedWorld, {
          anchor: anchors[ordinal]!,
          child: { borrow: child },
          attachment: { mode: "spatial" },
          ref: handle,
        });
      try {
        let pending = root.render(scene(0));
        if (phase === "anchor-retirement") {
          await pending;
          check(handle.current, "Initial attachment did not acknowledge");
          pending = root.render(scene(1));
        }
        await arrived.promise;
        if (phase === "anchor-retirement") await pending;
        const replacing =
          replacement === "unmount" ? root.unmount() : root.render(scene(2));
        check(
          handle.current === null,
          "Supersession did not immediately fence its ref",
        );
        release.release();
        await Promise.all([pending, replacing]);
        if (replacement === "supersede") {
          const deadline = performance.now() + 5000;
          while (!handle.current && performance.now() < deadline) {
            await root.flush();
            await new Promise<void>((resolve) => setTimeout(resolve, 16));
          }
          check(
            handle.current,
            `Latest attachment did not acknowledge: ${errors.map((error) => error.message).join("; ")}`,
          );
        }
        const expected = [
          ...(phase === "anchor-retirement" ? [anchors[0]!] : []),
          ...(replacement === "supersede" ? [anchors[2]!] : []),
        ];
        check(
          writes.length === expected.length &&
            writes.every((anchor, index) => anchor === expected[index]),
          `${phase}/${replacement} submitted stale unsent attachment: ${writes.join(",")}`,
        );
        check(
          errors.length === 0,
          `Fenced operation reported an error: ${errors.map((error) => error.message).join("; ")}`,
        );
        report.push(
          `${phase}/${replacement} fences unsent attachments across real delayed delivery`,
        );
      } finally {
        release.release();
        await root.unmount().catch(() => {});
        await client.close();
        await host.destroyWorld(parent);
        await host.destroyWorld(child);
      }
    }
  }
  report.push(...(await exerciseTypedSelectors(host)));
  return report;
}

async function exerciseTypedSelectors(
  host: HostClientBase<Client>,
): Promise<string[]> {
  const parent = (
    await host.createWorld({ selectedSystems: selectSystems(REACT_ROOT) })
  ).reference;
  const child = (
    await host.createWorld({ selectedSystems: selectSystems(REACT_CHILD) })
  ).reference;
  const client = await host.openWorld(parent);
  const observer = await host.openWorld(child);
  const parentCreated = await requireSuccess(
    client.batch([
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: "handle-anchor", classes: [] },
      },
    ]),
  );
  const anchor = parentCreated.aliases[0]!.id;
  const anchorName = `${anchor}n`;
  await requireSuccess(
    client.batch([
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: anchorName, classes: [] },
      },
    ]),
  );
  const childCreated = await requireSuccess(
    observer.batch([
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: "handle-camera", classes: [] },
      },
    ]),
  );
  const camera = childCreated.aliases[0]!.id;
  const cameraName = `${camera}n`;
  const namedCreated = await requireSuccess(
    observer.batch([
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: cameraName, classes: [] },
      },
    ]),
  );
  const namedCamera = namedCreated.aliases[0]!.id;
  const writes: bigint[] = [];
  const adapter: ReactCompositionHost = {
    get sessions() {
      return host.sessions;
    },
    createWorld: (options) => host.createWorld(options),
    destroyWorld: (world) => host.destroyWorld(world),
    listWorlds: () => host.listWorlds(),
    bindOutput: (world, entity, kind) => host.bindOutput(world, entity, kind),
    resolveOutput: (output) => host.resolveOutput(output),
    openWorld: async (world) => {
      const session = await host.openWorld(world);
      if (world.id !== parent.id) return session;
      return new Proxy(session, {
        get(target, property) {
          if (property === "batch")
            return (commands: Command[]) => {
              for (const command of commands)
                if (
                  command.kind === "insertComponent" &&
                  command.component === target.components.WorldAttachment!.id &&
                  command.entity.kind === "handle"
                )
                  writes.push(command.entity.id);
              return target.batch(commands);
            };
          const member: unknown = Reflect.get(target, property, target);
          return typeof member === "function" ? member.bind(target) : member;
        },
      });
    },
  };
  const root = createRoot(client, { host: adapter });
  const handle = React.createRef<AttachedWorldHandle>();
  const scene = (target: string | bigint, output: string | bigint) =>
    React.createElement(
      React.Fragment,
      null,
      ...["handle-anchor", anchorName].map((bindTo) =>
        React.createElement(
          Entity,
          { key: bindTo, bindTo },
          React.createElement(FlatSurface, { width: 1, height: 1 }),
        ),
      ),
      React.createElement(
        AttachedWorld,
        {
          anchor: target,
          child: { borrow: child },
          attachment: { mode: "surface-camera", output: { entity: output } },
          ref: handle,
        },
        ...["handle-camera", cameraName].map((bindTo) =>
          React.createElement(
            Entity,
            { key: bindTo, bindTo },
            React.createElement(Camera),
          ),
        ),
      ),
    );
  try {
    await root.render(scene(anchorName, cameraName));
    const original = handle.current!;
    check(
      original.output &&
        outputProducer(original.output)?.entity === namedCamera &&
        writes[0] !== anchor,
      "Symbolic selector did not identify its declared entity",
    );
    await root.render(scene(anchor, cameraName));
    const deadline = performance.now() + 5000;
    while (!handle.current && performance.now() < deadline) {
      await root.flush();
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    }
    check(
      handle.current && writes.length === 2 && writes[1] === anchor,
      "Bigint anchor collided with the same-looking string selector",
    );
    let fenced = false;
    try {
      void original.world;
    } catch {
      fenced = true;
    }
    check(fenced, "Superseded selector left its old handle live");
    await root.render(scene(anchor, camera));
    check(
      handle.current?.output &&
        outputProducer(handle.current.output)?.entity === camera &&
        writes.slice().length === 3,
      "Bigint output collided with the same-looking string selector",
    );
    return [
      "type-preserving anchor identity selects distinct real entities",
      "type-preserving output identity resolves distinct real Camera outputs",
    ];
  } finally {
    await root.unmount();
    await client.close();
    await observer.close();
    await host.destroyWorld(parent);
    await host.destroyWorld(child);
  }
}
