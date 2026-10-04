import * as React from "react";
import {
  type AttachmentReceipt,
  type BatchOutcome,
  type Client,
  type Command,
  type HostClientBase,
  type WorldCreateOptions,
  type WorldReference,
} from "@ipp/client";
import {
  AttachedWorld,
  Entity,
  createRoot,
  type AttachedWorldHandle,
  type ReactCompositionHost,
} from "@ipp/react";
import { attachmentEffects, requireSuccess } from "./fixture-helpers.js";
import {
  REACT_CHILD,
  REACT_ROOT,
  selectSystems,
} from "../integration/system-selections.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function gate() {
  let resolve!: () => void;
  const promise = new Promise<void>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

async function within<Value>(
  promise: Promise<Value>,
  message: string,
): Promise<Value> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), 1500);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

interface Recovery {
  readonly journal: {
    readonly child: WorldReference | undefined;
    readonly creatorOwned?: boolean;
    readonly preparationFailure?: unknown;
  };
  retry(): Promise<void>;
  abandon(): Promise<unknown>;
}

interface Probe {
  beforeCreate?: ((options: WorldCreateOptions) => void) | undefined;
  beforeOpen?: ((world: WorldReference) => void) | undefined;
  beforeChildClose?: (() => void) | undefined;
  beforeBatch?: ((commands: Command[]) => void) | undefined;
  batch?:
    | ((writer: Client, commands: Command[]) => ReturnType<Client["batch"]>)
    | undefined;
  retirement?:
    | ((writer: Client, receipt: bigint) => Promise<"pending" | "retired">)
    | undefined;
}

async function fixture(host: HostClientBase<Client>) {
  const parent = (
    await host.createWorld({ selectedSystems: selectSystems(REACT_ROOT) })
  ).reference;
  const child = (
    await host.createWorld({ selectedSystems: selectSystems(REACT_CHILD) })
  ).reference;
  const client = await host.openWorld(parent);
  const observer = await host.openWorld(child);
  const anchor = (
    await requireSuccess(
      client.batch([
        {
          kind: "create",
          alias: 0,
          metadata: { symbolicId: null, classes: [] },
        },
      ]),
    )
  ).aliases[0]!.id;
  const surface = client.components.FlatSurface!;
  await requireSuccess(
    client.batch([
      {
        kind: "insertComponent",
        entity: { kind: "handle", id: anchor },
        component: surface.id,
        fields: [
          {
            offset: surface.fields.width!.offset,
            value: { kind: "f32", value: 1 },
          },
          {
            offset: surface.fields.height!.offset,
            value: { kind: "f32", value: 1 },
          },
        ],
      },
    ]),
  );
  const camera = (
    await requireSuccess(
      observer.batch([
        {
          kind: "create",
          alias: 0,
          metadata: { symbolicId: null, classes: [] },
        },
        {
          kind: "insertComponent",
          entity: { kind: "alias", alias: 0 },
          component: observer.components.Camera!.id,
          fields: [],
        },
      ]),
    )
  ).aliases[0]!.id;
  const output = await host.bindOutput(child, camera, "camera");
  if (output.kind !== "camera")
    throw new Error("Camera binding was not a Camera");
  const probe: Probe = {};
  let writer: Client | undefined;
  const receipts: AttachmentReceipt[] = [];
  const writes: Command[] = [];
  const writeAcks: BatchOutcome[] = [];
  const errors: Error[] = [];
  const created: WorldReference[] = [];
  const adapter: ReactCompositionHost = {
    get sessions() {
      return host.sessions;
    },
    createWorld: async (options) => {
      probe.beforeCreate?.(options ?? {});
      const result = await host.createWorld(options);
      created.push(result.reference);
      return result;
    },
    destroyWorld: (world) => host.destroyWorld(world),
    listWorlds: () => host.listWorlds(),
    bindOutput: (world, entity, kind) => host.bindOutput(world, entity, kind),
    resolveOutput: (value) => host.resolveOutput(value),
    openWorld: async (world) => {
      probe.beforeOpen?.(world);
      const opened = await host.openWorld(world);
      if (world.id !== parent.id)
        return new Proxy(opened, {
          get(target, property) {
            if (property === "close")
              return async () => {
                probe.beforeChildClose?.();
                await target.close();
              };
            const member: unknown = Reflect.get(target, property, target);
            return typeof member === "function" ? member.bind(target) : member;
          },
        });
      writer = opened;
      return new Proxy(opened, {
        get(target, property) {
          if (property === "batch")
            return async (commands: Command[]) => {
              probe.beforeBatch?.(commands);
              writes.push(
                ...commands.filter(
                  (command) => command.kind === "insertComponent",
                ),
              );
              const outcome = await (probe.batch?.(target, commands) ??
                target.batch(commands));
              if (
                commands.some((command) => command.kind === "insertComponent")
              )
                writeAcks.push(outcome);
              receipts.push(
                ...attachmentEffects(outcome).map((effect) => effect.receipt),
              );
              return outcome;
            };
          if (property === "attachmentRetirement")
            return (receipt: bigint) =>
              probe.retirement?.(target, receipt) ??
              target.attachmentRetirement(receipt);
          const member: unknown = Reflect.get(target, property, target);
          return typeof member === "function" ? member.bind(target) : member;
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
  const reference = React.createRef<AttachedWorldHandle>();
  const scene = (mode: "spatial" | "surface-camera" = "spatial") =>
    React.createElement(AttachedWorld, {
      anchor,
      child: { borrow: child },
      attachment: mode === "spatial" ? { mode } : { mode, output },
      ref: reference,
      onError: (error) => {
        errors.push(error);
      },
    });
  const cleanupFailure = () =>
    errors.findLast((error) => error.name === "AttachedWorldCleanupError") as
      | (Error & { recovery?: Recovery })
      | undefined;
  const waitReady = async () => {
    const deadline = performance.now() + 3000;
    while (!reference.current && performance.now() < deadline) {
      await root.flush();
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    }
    check(
      reference.current,
      `Expected ready boundary: ${errors.map((error) => error.message).join("; ")}`,
    );
    return reference.current;
  };
  return {
    parent,
    child,
    client,
    root,
    reference,
    scene,
    probe,
    writes,
    writeAcks,
    receipts,
    errors,
    created,
    adapter,
    anchor,
    camera,
    cleanupFailure,
    waitReady,
    /**
     * Remove the boundary and await its cleanup, returning the cleanup
     * failure if any. Unmount would release the boundary without cleanup.
     */
    async remove(): Promise<unknown> {
      const handle = reference.current;
      await root.render(null);
      if (!handle) return undefined;
      return handle.closed.then(
        () => undefined,
        (error: unknown) => error ?? new Error("Boundary cleanup failed"),
      );
    },
    writer: () => writer!,
    async finish() {
      probe.beforeCreate = undefined;
      probe.beforeOpen = undefined;
      probe.beforeChildClose = undefined;
      probe.beforeBatch = undefined;
      probe.batch = undefined;
      probe.retirement = undefined;
      for (const receipt of receipts)
        await writer
          ?.batch([{ kind: "detachWorldAttachment", receipt: receipt.id }])
          .catch(() => {});
      await within(
        root.unmount().catch(() => {}),
        "fixture unmount did not settle",
      ).catch(() => {});
      await cleanupFailure()
        ?.recovery?.abandon()
        .catch(() => {});
      await writer?.close().catch(() => {});
      await client.close().catch(() => {});
      await observer.close().catch(() => {});
      for (const world of [parent, child, ...created])
        await host.destroyWorld(world).catch(() => {});
    },
  };
}

export async function exerciseAttachedWorldRecovery(
  host: HostClientBase<Client>,
): Promise<string[]> {
  const report: string[] = [];
  const failures: string[] = [];
  const run = async (
    name: string,
    action: (context: Awaited<ReturnType<typeof fixture>>) => Promise<void>,
  ) => {
    const context = await fixture(host);
    try {
      await action(context);
      report.push(name);
    } catch (error) {
      failures.push(
        `${name}: ${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      await context.finish();
    }
  };
  for (const action of ["reconcile", "unmount", "retry", "abandon"] as const)
    await run(
      `unsent receipt writer open permits ${action}`,
      async (context) => {
        let attempts = 0;
        const unavailable = Object.assign(
          new Error("writer open not submitted"),
          {
            code: "IPP_REQUEST_NOT_SENT",
          },
        );
        context.probe.beforeOpen = (world) => {
          if (world.id !== context.parent.id) return;
          attempts++;
          throw unavailable;
        };
        await context.root.render(context.scene());
        check(
          attempts > 0 &&
            context.errors.includes(unavailable) &&
            context.receipts.length === 0 &&
            !context.reference.current,
          "Expected failed real writer acquisition with no acknowledged writer/receipt",
        );
        context.probe.beforeOpen = (world) => {
          if (world.id === context.parent.id) attempts++;
        };
        if (action === "reconcile") {
          const before = attempts;
          await context.root.render(context.scene("surface-camera"));
          const handle = await context.waitReady();
          check(
            attempts > before &&
              handle &&
              handle.output?.kind === "camera" &&
              context.writeAcks.at(-1)?.ok &&
              context.receipts.length > 0,
            "Rejected open promise prevented a fresh acknowledged receipt writer",
          );
          const writer = context.writer();
          await within(context.root.unmount(), "Fresh writer did not close");
          check(
            !host.sessions.has(writer.session),
            "Fresh receipt writer leaked",
          );
        } else {
          if (action !== "unmount")
            context.probe.beforeChildClose = () => {
              throw new Error("recoverable child session close failure");
            };
          let failure: unknown;
          await within(
            context.root.unmount().catch((error: unknown) => {
              failure = error;
            }),
            "Writer acquisition failure blocked unmount",
          );
          if (action === "unmount")
            check(
              !failure,
              "Unmount tried to close a writer it never acquired",
            );
          else {
            const recovery = context.cleanupFailure()?.recovery;
            check(
              failure && recovery,
              "Expected reachable child cleanup recovery",
            );
            context.probe.beforeChildClose = undefined;
            await within(
              recovery[action](),
              "Recovery retried an unowned rejected writer",
            );
          }
        }
        check(
          (await host.listWorlds()).some(
            (world) => world.id === context.child.id,
          ),
          "Writer acquisition failure destroyed a borrowed child",
        );
      },
    );
  for (const action of ["reconcile", "unmount", "retry", "abandon"] as const)
    await run(`unsent child create permits ${action}`, async (context) => {
      const name = `react-unsent-create-${action}-${context.parent.id}`;
      let attempts = 0;
      const unavailable = Object.assign(
        new Error("child create not submitted"),
        { code: "IPP_REQUEST_NOT_SENT" },
      );
      context.probe.beforeCreate = (options) => {
        if (options.symbolicId !== name) return;
        attempts++;
        if (attempts === 1) throw unavailable;
      };
      const scene = () =>
        React.createElement(AttachedWorld, {
          anchor: context.anchor,
          child: {
            create: {
              selectedSystems: selectSystems(REACT_CHILD),
              symbolicId: name,
            },
          },
          attachment: { mode: "spatial" },
          ref: context.reference,
          onError: (error) => {
            context.errors.push(error);
          },
        });
      const named = async () =>
        (await host.listWorlds()).filter((world) => world.symbolicId === name);
      await context.root.render(scene());
      await context.root.flush();
      check(
        attempts === 1 &&
          context.errors.includes(unavailable) &&
          context.created.length === 0 &&
          context.receipts.length === 0 &&
          !context.reference.current &&
          (await named()).length === 0,
        "Expected a known-unsent child create with no World, receipt or handle",
      );
      check(
        !context.cleanupFailure(),
        "Known-unsent child create retained unresolved cleanup ownership",
      );
      if (action === "unmount") {
        await within(
          context.root.unmount(),
          "Known-unsent child create blocked unmount",
        );
        check(
          !context.cleanupFailure() && (await named()).length === 0,
          "Unmount after an unsent create reported or leaked ownership",
        );
        return;
      }
      // Redeclaring the boundary creates a fresh child under the same
      // symbolic name: the unsent create left no retiring creator behind.
      await context.root.render(null);
      await context.root.render(scene());
      const world = (await context.waitReady()).world;
      check(
        Number(attempts) === 2 &&
          Number(context.created.length) === 1 &&
          world.id === context.created[0]!.id &&
          (await named()).length === 1,
        "Rejected child create prevented a fresh creator-owned child",
      );
      if (action === "reconcile") {
        await within(context.remove(), "Recovered child did not close");
        check(
          (await named()).length === 0 && !context.cleanupFailure(),
          "Recovered creator-owned child leaked after its removal",
        );
        return;
      }
      // A later, genuinely owned cleanup failure stays recoverable: the
      // earlier rejected create leaves nothing that blocks retry.
      context.probe.beforeChildClose = () => {
        throw new Error("recoverable child session close failure");
      };
      let failure: unknown;
      await within(
        context.remove().then((error) => {
          failure = error;
        }),
        "Owned child cleanup failure blocked its removal",
      );
      const recovery = context.cleanupFailure()?.recovery;
      check(
        failure &&
          recovery &&
          recovery.journal.child?.id === world.id &&
          recovery.journal.creatorOwned === true &&
          recovery.journal.preparationFailure === undefined,
        "Expected exact creator-owned recovery without an unresolved create",
      );
      context.probe.beforeChildClose = undefined;
      await within(recovery[action](), `Recovery ${action} did not settle`);
      check(
        ((await named()).length === 0) === (action === "retry"),
        action === "retry"
          ? "Retry did not destroy the exact creator-owned child"
          : "Abandonment destroyed the child instead of relinquishing it",
      );
    });
  await run(
    "unknown child create outcome retains explicit recovery",
    async (context) => {
      const name = `react-unknown-create-${context.parent.id}`;
      const lost = new Error("child create outcome unknown");
      context.probe.beforeCreate = (options) => {
        if (options.symbolicId === name) throw lost;
      };
      await context.root.render(
        React.createElement(AttachedWorld, {
          anchor: context.anchor,
          child: {
            create: {
              selectedSystems: selectSystems(REACT_CHILD),
              symbolicId: name,
            },
          },
          attachment: { mode: "spatial" },
          ref: context.reference,
          onError: (error) => {
            context.errors.push(error);
          },
        }),
      );
      const deadline = performance.now() + 3000;
      while (!context.cleanupFailure() && performance.now() < deadline)
        await new Promise<void>((resolve) => setTimeout(resolve, 16));
      const recovery = context.cleanupFailure()?.recovery;
      check(
        context.errors.includes(lost) &&
          recovery &&
          recovery.journal.preparationFailure === lost &&
          recovery.journal.child === undefined &&
          recovery.journal.creatorOwned === false,
        "An unknown create outcome was discarded instead of journaled",
      );
      context.probe.beforeCreate = undefined;
      await context.root.render(null);
      check(
        context.created.length === 0,
        "An unresolved create outcome permitted a colliding creator",
      );
      let retried = false;
      await within(
        recovery.retry().then(
          () => {
            retried = true;
          },
          () => {},
        ),
        "Unknown-outcome retry did not settle",
      );
      check(!retried, "Retry claimed an unknown create outcome was resolved");
      await within(recovery.abandon(), "Explicit abandonment did not settle");
    },
  );
  await run(
    "applied failed outcome replaces current selection",
    async (context) => {
      await context.root.render(context.scene());
      context.probe.batch = async (writer, commands) => {
        if (!commands.some((command) => command.kind === "insertComponent"))
          return writer.batch(commands);
        context.probe.batch = undefined;
        const invalid: Command = {
          kind: "delete",
          entity: { kind: "handle", id: 0xffffffffffffffffn },
        };
        return writer.batch([...commands, invalid]);
      };
      await context.root.render(context.scene("surface-camera"));
      check(context.errors.length > 0, "Expected real failed attachment batch");
      check(
        context.receipts.length >= 2,
        "Failure did not acknowledge a real replacement receipt",
      );
      await context.root.render(context.scene());
      const handle = context.reference.current;
      const latest = context.writeAcks.at(-1);
      const effect =
        latest &&
        attachmentEffects(latest).find((item) => item.kind === "written");
      check(
        context.writes.length === 3 &&
          handle &&
          handle.output === undefined &&
          latest?.ok &&
          effect &&
          effect.receipt.id !== context.receipts[1]!.id &&
          effect.receipt.child?.id === handle.world.id,
        "A/B-failed/A published stale A without rewriting the acknowledged B selection",
      );
      const entity = (await context.client.inspect()).entities.find(
        (item) => item.id === context.anchor,
      );
      const attachment = entity?.components.find(
        (item) =>
          item.component === context.client.components.WorldAttachment!.id,
      );
      check(
        attachment?.fields.mode === 0 &&
          (await context.writer().attachmentRetirement(effect.receipt.id)) ===
            "pending",
        "Successful third ACK did not leave the spatial attachment current",
      );
    },
  );
  for (const kind of ["detach", "query"] as const)
    await run(
      `${kind} failure retains recoverable receipt authority`,
      async (context) => {
        await context.root.render(
          React.createElement(AttachedWorld, {
            anchor: context.anchor,
            child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
            attachment: { mode: "spatial" },
            ref: context.reference,
          }),
        );
        const world = context.reference.current!.world;
        const session = context.writer().session;
        if (kind === "detach")
          context.probe.beforeBatch = (commands) => {
            if (
              commands.some(
                (command) => command.kind === "detachWorldAttachment",
              )
            )
              throw Object.assign(new Error("definite non-submission"), {
                code: "IPP_REQUEST_NOT_SENT",
              });
          };
        else
          context.probe.retirement = async () => {
            throw new Error("recoverable retirement query failure");
          };
        let rejected = false;
        await within(
          context.remove().then((error) => {
            rejected = error !== undefined;
          }),
          "cleanup waited forever after known detach non-submission",
        );
        check(
          rejected && host.sessions.has(session),
          "Incomplete cleanup revoked its receipt session",
        );
        check(
          (await host.listWorlds()).some((item) => item.id === world.id),
          "Incomplete cleanup destroyed its child",
        );
        const recovery = context.cleanupFailure()?.recovery;
        check(recovery, "Cleanup failure has no recoverable owner");
        context.probe.beforeBatch = undefined;
        context.probe.retirement = undefined;
        const retrying = recovery.retry();
        check(
          recovery.retry() === retrying,
          "Concurrent recovery retries did not coalesce",
        );
        await retrying;
        check(
          !(await host.listWorlds()).some((item) => item.id === world.id),
          "Recovery did not complete actual cleanup",
        );
        // The mounted root keeps its receipt writer; unmount closes it.
        await context.root.unmount();
        check(
          !host.sessions.has(session),
          "Unmount did not close the receipt writer",
        );
      },
    );
  for (const kind of ["cleanup", "null", "swap"] as const)
    await run(
      `reentrant ref ${kind} disposes exactly once`,
      async (context) => {
        let first = 0;
        let second = 0;
        let ready = 0;
        const pending: Promise<void>[] = [];
        const nextRef = (value: AttachedWorldHandle | null) => {
          if (value)
            return () => {
              second++;
            };
          return undefined;
        };
        const render = (ref: React.Ref<AttachedWorldHandle>) =>
          React.createElement(AttachedWorld, {
            anchor: context.anchor,
            child: { borrow: context.child },
            attachment: { mode: "spatial" },
            ref,
            onReady: () => {
              ready++;
            },
          });
        const ref = (value: AttachedWorldHandle | null) => {
          if (!value) {
            first++;
            return;
          }
          pending.push(
            kind === "swap"
              ? context.root.render(render(nextRef))
              : context.root.unmount(),
          );
          if (kind !== "null")
            return () => {
              first++;
            };
          return undefined;
        };
        await context.root.render(render(ref));
        await Promise.all(pending);
        check(
          first === 1,
          "Superseded callback-return cleanup/null fallback was lost",
        );
        if (kind === "swap") {
          check(second === 0, "Nested live ref disposed early");
          await context.root.unmount();
          check(Number(second) === 1, "Nested ref disposer was overwritten");
        } else check(ready === 0, "Reentrant unmount delivered onReady");
      },
    );
  for (const replacement of ["unmount", "supersede"] as const)
    await run(
      `replacement ref disposal ${replacement} fences capture`,
      async (context) => {
        let cleanup: Promise<void> | undefined;
        const finalName = `react-capture-final-${context.parent.id}`;
        const scene = (name: string, ref: React.Ref<AttachedWorldHandle>) =>
          React.createElement(AttachedWorld, {
            anchor: context.anchor,
            child: {
              create: {
                selectedSystems: selectSystems(REACT_CHILD),
                symbolicId: name,
              },
            },
            attachment: { mode: "spatial" },
            ref,
          });
        const ref = (value: AttachedWorldHandle | null) => {
          if (value)
            return () => {
              cleanup =
                replacement === "unmount"
                  ? context.root.unmount()
                  : context.root.render(scene(finalName, context.reference));
            };
          return undefined;
        };
        await context.root.render(
          scene(`react-capture-old-${context.parent.id}`, ref),
        );
        const session = context.writer().session;
        await context.root.render(
          scene(`react-capture-stale-${context.parent.id}`, context.reference),
        );
        check(
          cleanup,
          "Replacement did not synchronously invoke the old disposer",
        );
        await within(cleanup, "Reentrant cleanup did not settle");
        if (replacement === "supersede") {
          const handle = await context.waitReady();
          check(
            (await host.listWorlds()).find(
              (world) => world.id === handle.world.id,
            )?.symbolicId === finalName && context.created.length === 2,
            "Outer capture overwrote the reentrant replacement or created a stale child",
          );
          await context.root.unmount();
        }
        check(
          !host.sessions.has(session) &&
            (replacement !== "unmount" || context.created.length === 1),
          "Closed scope retained a newly inserted uncancelled record or receipt writer",
        );
      },
    );
  for (const failure of [false, true])
    await run(
      `older same-name creator ${failure ? "failure" : "retirement"} blocks reuse`,
      async (context) => {
        const arrived = gate();
        const release = gate();
        const name = `react-sparse-name-${context.parent.id}`;
        const scene = (symbolicId: string) =>
          React.createElement(AttachedWorld, {
            anchor: context.anchor,
            child: {
              create: {
                selectedSystems: selectSystems(REACT_CHILD),
                symbolicId,
              },
            },
            attachment: { mode: "spatial" },
            ref: context.reference,
            onError: (error) => {
              context.errors.push(error);
            },
          });
        await context.root.render(scene(name));
        const original = context.reference.current!.world;
        const originalReceipt = context.receipts[0]!.id;
        context.probe.retirement = async (writer, receipt) => {
          const result = await writer.attachmentRetirement(receipt);
          if (receipt === originalReceipt) {
            arrived.resolve();
            await release.promise;
            if (failure)
              throw new Error("older creator retirement unavailable");
          }
          return result;
        };
        try {
          const different = context.root.render(scene(`${name}-different`));
          await within(
            arrived.promise,
            "Original creator did not reach retirement",
          );
          await within(different, "Unrelated name waited on old retirement");
          await context.waitReady();
          await within(
            context.root.render(scene(name)),
            "Sparse name barrier blocked root queue",
          );
          await new Promise<void>((resolve) => setTimeout(resolve, 100));
          check(
            context.created.length === 2 &&
              context.errors.length === 0 &&
              !context.reference.current,
            "Older same-name creator was bypassed by the immediate differently-named predecessor",
          );
          release.resolve();
          if (failure) {
            const deadline = performance.now() + 3000;
            while (!context.cleanupFailure() && performance.now() < deadline)
              await new Promise<void>((resolve) => setTimeout(resolve, 16));
            const originalFailure = context.errors.find(
              (error) =>
                error.name === "AttachedWorldCleanupError" &&
                "recovery" in error &&
                (error.recovery as Recovery).journal.child?.id === original.id,
            );
            check(
              originalFailure && context.created.length === 2,
              "Original failure lost its recovery journal or attempted a colliding create",
            );
            context.probe.retirement = undefined;
            await context.root.render(null);
            for (const error of context.errors) {
              if (
                error.name === "AttachedWorldCleanupError" &&
                "recovery" in error
              ) {
                const recovery = error.recovery as Recovery;
                await recovery.retry().catch(() => recovery.abandon());
              }
            }
            await context.root.render(scene(name));
            const recovered = await context.waitReady();
            check(
              recovered.world.id !== original.id &&
                Number(context.created.length) === 3,
              "Recovered creator left a permanent name barrier",
            );
          } else {
            const handle = await context.waitReady();
            check(
              handle.world.id !== original.id &&
                Number(context.created.length) === 3,
              "Same-name replacement did not create after exact destruction",
            );
          }
          check(
            !(await host.listWorlds()).some(
              (world) => world.id === original.id,
            ),
            "Original creator survived successful cleanup/recovery",
          );
        } finally {
          release.resolve();
        }
      },
    );
  await run(
    "cross-container name wait cancellation leaves unrelated creators free",
    async (context) => {
      const arrived = gate();
      const release = gate();
      const name = `react-cross-container-${context.parent.id}`;
      const originalRef = React.createRef<AttachedWorldHandle>();
      const scene = (original: boolean, nestedName?: string) =>
        React.createElement(
          React.Fragment,
          null,
          React.createElement(Entity, { id: "holder-anchor" }),
          original
            ? React.createElement(AttachedWorld, {
                key: "original",
                anchor: context.anchor,
                child: {
                  create: {
                    selectedSystems: selectSystems(REACT_CHILD),
                    symbolicId: name,
                  },
                },
                attachment: { mode: "spatial" },
                ref: originalRef,
              })
            : null,
          React.createElement(
            AttachedWorld,
            {
              key: "holder",
              anchor: "holder-anchor",
              child: { borrow: context.child },
              attachment: { mode: "spatial" },
            },
            nestedName
              ? React.createElement(AttachedWorld, {
                  anchor: context.camera,
                  child: {
                    create: {
                      selectedSystems: selectSystems(REACT_CHILD),
                      symbolicId: nestedName,
                    },
                  },
                  attachment: { mode: "spatial" },
                  ref: context.reference,
                })
              : null,
          ),
        );
      await context.root.render(scene(true));
      const original = originalRef.current!.world;
      const receipt = context.receipts.find(
        (item) => item.child?.id === original.id,
      )!;
      context.probe.retirement = async (writer, token) => {
        const result = await writer.attachmentRetirement(token);
        if (token === receipt.id) {
          arrived.resolve();
          await release.promise;
        }
        return result;
      };
      try {
        await context.root.render(scene(false, name));
        await within(
          arrived.promise,
          "Cross-container predecessor never retired",
        );
        check(
          context.created.length === 1 && !context.reference.current,
          "Child container ignored coordinator name barrier",
        );
        await within(
          context.root.render(scene(false, `${name}-unrelated`)),
          "Cancelled unsent child retained an unrelated name wait",
        );
        await context.waitReady();
        check(
          Number(context.created.length) === 2 && context.errors.length === 0,
          "Unrelated creator could not replace a cancelled waiting child",
        );
        await context.root.render(scene(false, name));
        check(
          !context.reference.current,
          "Cross-container name reused before destruction",
        );
        release.resolve();
        const handle = await context.waitReady();
        check(
          handle.world.id !== original.id &&
            Number(context.created.length) === 3 &&
            !(await host.listWorlds()).some(
              (world) => world.id === original.id,
            ),
          "Cross-container creator did not follow exact destruction",
        );
      } finally {
        release.resolve();
      }
    },
  );
  await run(
    "name-reusing creator replacement waits off the shared queue",
    async (context) => {
      const arrived = gate();
      const release = gate();
      const name = `react-replacement-${context.parent.id}`;
      const scene = (temporary: boolean) =>
        React.createElement(AttachedWorld, {
          anchor: context.anchor,
          child: {
            create: {
              selectedSystems: selectSystems(REACT_CHILD),
              symbolicId: name,
              temporary,
            },
          },
          attachment: { mode: "spatial" },
          ref: context.reference,
          onError: (error) => {
            context.errors.push(error);
          },
        });
      await context.root.render(scene(false));
      const previous = context.reference.current!.world;
      context.probe.retirement = async (writer, receipt) => {
        const result = await writer.attachmentRetirement(receipt);
        arrived.resolve();
        await release.promise;
        return result;
      };
      try {
        const replacing = context.root.render(scene(true));
        await arrived.promise;
        await within(
          replacing,
          "Replacement retirement monopolized root flush",
        );
        check(
          context.created.length === 1 && context.errors.length === 0,
          "Name-reusing create ran before predecessor destruction",
        );
        const sibling = createRoot(context.client);
        try {
          await sibling.render(
            React.createElement(Entity, { id: "independent-replacement-work" }),
          );
        } finally {
          await sibling.unmount();
        }
        release.resolve();
        const current = await context.waitReady();
        check(
          current.world.id !== previous.id &&
            !(await host.listWorlds()).some((item) => item.id === previous.id),
          "Creator replacement lost exact predecessor destruction",
        );
      } finally {
        release.resolve();
      }
    },
  );
  await run(
    "explicit abandonment retains Worlds and foreign replacement",
    async (context) => {
      await context.root.render(
        React.createElement(AttachedWorld, {
          anchor: context.anchor,
          child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
          attachment: { mode: "spatial" },
          ref: context.reference,
        }),
      );
      const owned = context.reference.current!.world;
      context.probe.beforeBatch = (commands) => {
        if (
          commands.some((command) => command.kind === "detachWorldAttachment")
        )
          throw Object.assign(new Error("deliberate non-submission"), {
            code: "IPP_REQUEST_NOT_SENT",
          });
      };
      await within(context.remove(), "Failed detach did not release caller");
      const recovery = context.cleanupFailure()?.recovery;
      check(recovery, "Abandonment has no explicit authority owner");
      const component = context.client.components.WorldAttachment!;
      const external = await requireSuccess(
        context.client.batch([
          {
            kind: "insertComponent",
            entity: { kind: "handle", id: context.anchor },
            component: component.id,
            fields: [
              {
                offset: component.fields.child!.offset,
                value: { kind: "world", value: context.child },
              },
            ],
          },
        ]),
      );
      await recovery.abandon();
      // The mounted root keeps its receipt writer; unmount closes it.
      await context.root.unmount();
      check(
        !host.sessions.has(context.writer().session),
        "Explicit abandonment leaked the receipt session",
      );
      check(
        (await host.listWorlds()).some((world) => world.id === owned.id),
        "Abandonment destroyed the owned child without retirement",
      );
      check(
        (await context.client.attachmentRetirement(
          attachmentEffects(external)[0]!.receipt.id,
        )) === "pending",
        "Abandonment cleared a foreign replacement",
      );
    },
  );
  await run(
    "failed predecessor prevents a name-reusing create",
    async (context) => {
      const name = `react-failed-replacement-${context.parent.id}`;
      const scene = (temporary: boolean) =>
        React.createElement(AttachedWorld, {
          anchor: context.anchor,
          child: {
            create: {
              selectedSystems: selectSystems(REACT_CHILD),
              symbolicId: name,
              temporary,
            },
          },
          attachment: { mode: "spatial" },
          ref: context.reference,
          onError: (error) => {
            context.errors.push(error);
          },
        });
      await context.root.render(scene(false));
      const previous = context.reference.current!.world;
      context.probe.retirement = async () => {
        throw new Error("predecessor observation unavailable");
      };
      await context.root.render(scene(true));
      const deadline = performance.now() + 3000;
      while (!context.cleanupFailure() && performance.now() < deadline)
        await new Promise<void>((resolve) => setTimeout(resolve, 16));
      await context.root.unmount().catch(() => {});
      check(
        context.created.length === 1 &&
          (await host.listWorlds()).some((world) => world.id === previous.id),
        "Failed predecessor permitted unsafe successor creation",
      );
      context.probe.retirement = undefined;
      for (const error of context.errors) {
        if (
          error.name !== "AttachedWorldCleanupError" ||
          !("recovery" in error)
        )
          continue;
        const recovery = error.recovery as Recovery;
        await recovery.retry().catch(() => recovery.abandon());
      }
      check(
        !(await host.listWorlds()).some((world) => world.id === previous.id),
        "Predecessor journal could not recover after failure",
      );
    },
  );
  await run(
    "failed outer detach does not wait for nested retirement",
    async (context) => {
      const opened = context.adapter.openWorld;
      const arrived = gate();
      const release = gate();
      context.adapter.openWorld = async (world) => {
        const client = await opened(world);
        if (world.id === context.parent.id) return client;
        return new Proxy(client, {
          get(target, property) {
            if (property === "attachmentRetirement")
              return async (receipt: bigint) => {
                const result = await target.attachmentRetirement(receipt);
                arrived.resolve();
                await release.promise;
                return result;
              };
            const member: unknown = Reflect.get(target, property, target);
            return typeof member === "function" ? member.bind(target) : member;
          },
        });
      };
      const nested = React.createRef<AttachedWorldHandle>();
      await context.root.render(
        React.createElement(
          AttachedWorld,
          {
            anchor: context.anchor,
            child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
            attachment: { mode: "spatial" },
            ref: context.reference,
          },
          React.createElement(Entity, { id: "nested-anchor" }),
          React.createElement(AttachedWorld, {
            anchor: "nested-anchor",
            child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
            attachment: { mode: "spatial" },
            ref: nested,
          }),
        ),
      );
      check(
        context.reference.current && nested.current,
        "Nested boundaries did not acknowledge",
      );
      const worlds = [context.reference.current.world, nested.current.world];
      context.probe.beforeBatch = (commands) => {
        if (
          commands.some((command) => command.kind === "detachWorldAttachment")
        )
          throw Object.assign(new Error("outer detach not submitted"), {
            code: "IPP_REQUEST_NOT_SENT",
          });
      };
      try {
        await within(
          context.remove(),
          "Outer known failure waited for nested retirement",
        );
        await within(
          arrived.promise,
          "Nested cleanup did not progress independently",
        );
        const recovery = context.cleanupFailure()?.recovery;
        check(recovery, "Outer failure lost recovery ownership");
        const sibling = createRoot(context.client);
        try {
          await sibling.render(
            React.createElement(Entity, { id: "independent-nested-cleanup" }),
          );
        } finally {
          await sibling.unmount();
        }
        context.probe.beforeBatch = undefined;
        release.resolve();
        await recovery.retry();
        check(
          !(await host.listWorlds()).some((world) =>
            worlds.some((owned) => owned.id === world.id),
          ),
          "Nested recovery leaked creator-owned Worlds",
        );
      } finally {
        release.resolve();
      }
    },
  );
  if (failures.length) throw new Error(failures.join("\n"));
  return report;
}
