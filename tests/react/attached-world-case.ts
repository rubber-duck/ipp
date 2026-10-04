import * as React from "react";
import type {
  CameraOutputReference,
  Client,
  LifecycleTargetWatch,
  LifecycleWatchClosure,
  Command,
  HostClientBase,
  OutputReference,
  WorldReference,
} from "@ipp/client";
import {
  AttachedWorld,
  Camera,
  Entity,
  Scalar,
  FlatSurface,
  createRoot,
  type AttachedWorldHandle,
  type ReactCompositionHost,
} from "@ipp/react";
import {
  attachmentEffects,
  findEntity,
  observeScalarInInspection,
  requireSuccess,
} from "./fixture-helpers.js";
import { exerciseAttachedWorldFences } from "./attached-world-fences.js";
import {
  REACT_CHILD,
  REACT_ROOT,
  selectSystems,
} from "../integration/system-selections.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const encoded = (value: unknown) => JSON.stringify(value);

/** The Camera output an attachment presents, if it presents one. */
function cameraOutput(
  output: OutputReference | undefined,
): CameraOutputReference | undefined {
  return output?.kind === "camera" ? output : undefined;
}

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

function hostMethods(host: HostClientBase<Client>): ReactCompositionHost {
  return {
    get sessions() {
      return host.sessions;
    },
    createWorld: (options) => host.createWorld(options),
    openWorld: (world) => host.openWorld(world),
    destroyWorld: (world) => host.destroyWorld(world),
    listWorlds: () => host.listWorlds(),
    bindOutput: (world, entity, kind) => host.bindOutput(world, entity, kind),
    resolveOutput: (output) => host.resolveOutput(output),
  };
}

function writeAttachment(
  client: Client,
  anchor: bigint,
  child: WorldReference,
): Command {
  const component = client.components.WorldAttachment!;
  return {
    kind: "insertComponent",
    entity: { kind: "handle", id: anchor },
    component: component.id,
    fields: [
      {
        offset: component.fields.child!.offset,
        value: { kind: "world", value: child },
      },
    ],
  };
}

export async function exerciseAttachedWorlds(
  host: HostClientBase<Client>,
): Promise<string[]> {
  const report: string[] = [];
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  const roots: ReturnType<typeof createRoot>[] = [];
  const failures: Error[] = [];
  const releaseGates: (() => void)[] = [];
  const create = async (systems: readonly string[] = REACT_ROOT) => {
    const world = (
      await host.createWorld({ selectedSystems: selectSystems(systems) })
    ).reference;
    worlds.push(world);
    return world;
  };
  const open = async (world: WorldReference) => {
    const client = await host.openWorld(world);
    sessions.push(client);
    return client;
  };
  const rootFor = (
    client: Client,
    composition: ReactCompositionHost = host,
  ) => {
    const root = createRoot(client, {
      host: composition,
      onError: (error) => {
        failures.push(error);
      },
    });
    roots.push(root);
    return root;
  };
  const exists = async (world: WorldReference) =>
    (await host.listWorlds()).some((item) => item.id === world.id);
  try {
    const parentWorld = await create();
    const parent = await open(parentWorld);
    const borrowed = await create();
    const observer = await open(borrowed);
    const root = rootFor(parent);
    const value = React.createContext(0);
    const handle = React.createRef<AttachedWorldHandle>();
    let ready = 0;
    function Child() {
      return React.createElement(
        Entity,
        { id: "context-child" },
        React.createElement(Scalar, { value: React.useContext(value) }),
      );
    }
    const scene = (
      amount: number,
      onReady = () => {
        ready++;
      },
    ) =>
      React.createElement(
        React.StrictMode,
        null,
        React.createElement(
          value,
          { value: amount },
          React.createElement(Entity, { id: "anchor" }),
          React.createElement(
            AttachedWorld,
            {
              anchor: "anchor",
              child: { borrow: borrowed },
              attachment: { mode: "spatial" },
              ref: handle,
              onReady,
            },
            React.createElement(Child),
          ),
        ),
      );
    await root.render(scene(12));
    check(
      handle.current?.world.id === borrowed.id && ready === 1,
      "StrictMode did not acknowledge exactly one borrowed boundary",
    );
    check(
      observeScalarInInspection(
        await observer.inspect(),
        observer.components.Scalar!.id,
        "context-child",
      ).value === 12,
      "Portal lost parent React context",
    );
    check(
      !findEntity(await parent.inspect(), "context-child"),
      "Child declarations escaped their fixed World",
    );
    report.push(
      "StrictMode portal preserves context and fixed-World ownership",
    );
    const first = handle.current;
    await root.render(
      scene(27, () => {
        ready += 100;
      }),
    );
    check(
      handle.current === first && ready === 1,
      "Callback-only boundary props rewrote the attachment",
    );
    check(
      observeScalarInInspection(
        await observer.inspect(),
        observer.components.Scalar!.id,
        "context-child",
      ).value === 27,
      "Context update failed across portal",
    );
    report.push(
      "callback-only boundary changes retain the acknowledged handle",
    );
    await root.render(null);
    check(handle.current === null, "Unmount did not fence refs immediately");
    await first.closed;
    check(await exists(borrowed), "Borrowed World was destroyed");
    check(
      !findEntity(await observer.inspect(), "context-child"),
      "Borrowed declarations leaked",
    );
    report.push(
      "borrowed teardown releases declarations without destroying its World",
    );

    const ownedRef = React.createRef<AttachedWorldHandle>();
    await root.render(
      React.createElement(
        React.Fragment,
        null,
        React.createElement(
          Entity,
          { id: "camera-anchor" },
          React.createElement(FlatSurface, { width: 1, height: 1 }),
        ),
        React.createElement(
          AttachedWorld,
          {
            anchor: "camera-anchor",
            child: {
              create: {
                selectedSystems: selectSystems(REACT_CHILD),
                symbolicId: "react-created-camera",
              },
            },
            attachment: {
              mode: "surface-camera",
              output: { entity: "child-camera" },
            },
            ref: ownedRef,
          },
          React.createElement(
            Entity,
            { id: "child-camera" },
            React.createElement(Camera),
          ),
        ),
      ),
    );
    const owned = ownedRef.current;
    check(
      owned?.output?.kind === "camera" &&
        owned.output.world.id === owned.world.id,
      `Output was not resolved after child acknowledgement: ${failures.map((error) => error.message).join("; ")}`,
    );
    const ownedWorld = owned.world;
    worlds.push(ownedWorld);
    await root.render(null);
    await owned.closed;
    check(
      !(await exists(ownedWorld)),
      "Creator-owned child survived acknowledged teardown",
    );
    report.push(
      "creator-owned Camera output resolves before attachment and destroys after retirement",
    );

    // A child commit that leaves the output declaration alone reuses the
    // bound output, which a lifecycle watch on the output component keeps
    // current: another producer's replacement of the Auto-declared Camera
    // rebinds once without a declaration change, and replacing the
    // declaration binds again.
    let outputBindings = 0;
    let outputWatches = 0;
    const counts = () => ({ binds: outputBindings, watches: outputWatches });
    const counting = hostMethods(host);
    counting.bindOutput = (world, entity, kind) => {
      outputBindings++;
      return host.bindOutput(world, entity, kind);
    };
    counting.openWorld = async (world) => {
      const client = await host.openWorld(world);
      return new Proxy(client, {
        get(target, property) {
          if (property === "watchLifecycle")
            return (...args: Parameters<Client["watchLifecycle"]>) => {
              outputWatches++;
              return target.watchLifecycle(...args);
            };
          const member: unknown = Reflect.get(target, property, target);
          return typeof member === "function" ? member.bind(target) : member;
        },
      });
    };
    const countedRoot = rootFor(parent, counting);
    const countedRef = React.createRef<AttachedWorldHandle>();
    const counted = (amount: number, camera = "camera") =>
      React.createElement(
        React.Fragment,
        null,
        React.createElement(
          Entity,
          { id: "counted-anchor" },
          React.createElement(FlatSurface, { width: 1, height: 1 }),
        ),
        React.createElement(
          AttachedWorld,
          {
            anchor: "counted-anchor",
            child: {
              create: {
                selectedSystems: selectSystems(REACT_CHILD),
                symbolicId: "react-counted-camera",
              },
            },
            attachment: {
              mode: "surface-camera",
              output: { entity: "counted-camera" },
            },
            ref: countedRef,
          },
          React.createElement(
            Entity,
            { id: "counted-camera" },
            React.createElement(Camera, { key: camera }),
          ),
          React.createElement(
            Entity,
            { id: "counted-value" },
            React.createElement(Scalar, { value: amount }),
          ),
        ),
      );
    const expectCounts = (binds: number, watches: number, label: string) =>
      check(
        outputBindings === binds && outputWatches === watches,
        `${label}: ${JSON.stringify(counts())}`,
      );
    await countedRoot.render(counted(1));
    const countedHandle = countedRef.current;
    check(
      countedHandle?.output?.kind === "camera",
      `Counted output was not bound: ${failures.map((error) => error.message).join("; ")}`,
    );
    expectCounts(1, 1, "Mount did not bind and watch its output once");
    const countedOutput = cameraOutput(countedHandle.output);
    worlds.push(countedHandle.world);
    for (const amount of [2, 3]) await countedRoot.render(counted(amount));
    expectCounts(1, 1, "Child content commits rebound an unchanged output");
    check(
      countedRef.current === countedHandle &&
        countedHandle.output === countedOutput,
      "Child content commits replaced the attached handle",
    );
    const producer = await open(countedHandle.world);
    await requireSuccess(
      producer.batch([
        {
          kind: "insertComponent",
          entity: { kind: "handle", id: countedOutput!.entity },
          component: producer.components.Camera!.id,
          fields: [],
        },
      ]),
    );
    const replacement = await host.bindOutput(
      countedHandle.world,
      countedOutput!.entity,
      "camera",
    );
    check(
      cameraOutput(replacement)!.incarnation !== countedOutput!.incarnation,
      "The foreign Camera insertion did not replace the output incarnation",
    );
    for (
      let attempt = 0;
      attempt < 200 &&
      cameraOutput(countedRef.current?.output)?.incarnation !==
        cameraOutput(replacement)?.incarnation;
      attempt++
    )
      await new Promise((resolve) => setTimeout(resolve, 16));
    const rebound = cameraOutput(countedRef.current?.output);
    check(
      rebound?.incarnation === cameraOutput(replacement)?.incarnation &&
        rebound?.entity === countedOutput!.entity,
      `A foreign Auto output replacement was not rebound: ${JSON.stringify(counts())}`,
    );
    expectCounts(2, 1, "A foreign replacement did not rebind exactly once");
    await countedRoot.render(counted(4));
    expectCounts(2, 1, "A child commit after the rebind bound again");
    await countedRoot.render(counted(4, "replaced-camera"));
    check(
      countedRef.current?.output?.kind === "camera",
      "A replaced output declaration lost its output",
    );
    expectCounts(3, 1, "A replaced output declaration was not bound again");
    const countedClosed = countedRef.current!.closed;
    await countedRoot.render(null);
    await countedClosed;
    report.push(
      "child commits reuse a watched output binding and rebind after a foreign or declared replacement",
    );

    // Output tracking that ends, by closure or a refused watch request, is
    // reported once and leaves only its own attachment binding on every
    // child commit; a sibling attachment keeps its reused binding.
    const trackingModes = ["closed", "refused", "kept"] as const;
    type TrackingMode = (typeof trackingModes)[number];
    const modeOfWorld = new Map<bigint, TrackingMode>();
    const bindsByMode = new Map<TrackingMode, number>(
      trackingModes.map((mode) => [mode, 0]),
    );
    const closeWatch = deferred<LifecycleWatchClosure>();
    const tracking = hostMethods(host);
    tracking.createWorld = async (options) => {
      const created = await host.createWorld(options);
      const mode = trackingModes.find(
        (candidate) => options?.symbolicId === `react-tracking-${candidate}`,
      );
      if (mode) modeOfWorld.set(created.reference.id, mode);
      return created;
    };
    tracking.bindOutput = (world, entity, kind) => {
      const mode = modeOfWorld.get(world.id);
      if (mode) bindsByMode.set(mode, bindsByMode.get(mode)! + 1);
      return host.bindOutput(world, entity, kind);
    };
    tracking.openWorld = async (world) => {
      const client = await host.openWorld(world);
      const mode = modeOfWorld.get(world.id);
      if (mode !== "closed" && mode !== "refused") return client;
      return new Proxy(client, {
        get(target, property) {
          if (property === "watchLifecycle")
            return async (
              ...args: Parameters<Client["watchLifecycle"]>
            ): Promise<LifecycleTargetWatch> => {
              if (mode === "refused")
                throw new Error("Output watch refused by the test host");
              const watch = await target.watchLifecycle(...args);
              // The watch is frozen: present a copy whose closure the test
              // can also end, with methods bound to the real watch.
              const wrapped: Record<string, unknown> = {};
              for (const key of Object.keys(watch)) {
                const member: unknown = Reflect.get(watch, key, watch);
                wrapped[key] =
                  typeof member === "function" ? member.bind(watch) : member;
              }
              for (const key of ["removeMembers", "remove"] as const)
                wrapped[key] = watch[key].bind(watch);
              wrapped.closed = Promise.race([watch.closed, closeWatch.promise]);
              return wrapped as unknown as LifecycleTargetWatch;
            };
          const member: unknown = Reflect.get(target, property, target);
          return typeof member === "function" ? member.bind(target) : member;
        },
      });
    };
    const trackingRoot = rootFor(parent, tracking);
    const trackingReported = () =>
      failures.filter((error) =>
        error.message.startsWith("Attached output tracking"),
      );
    const priorReports = trackingReported().length;
    const trackingScene = (amount: number) =>
      React.createElement(
        React.Fragment,
        null,
        trackingModes.map((mode) =>
          React.createElement(
            React.Fragment,
            { key: mode },
            React.createElement(
              Entity,
              { id: `tracking-anchor-${mode}` },
              React.createElement(FlatSurface, { width: 1, height: 1 }),
            ),
            React.createElement(
              AttachedWorld,
              {
                anchor: `tracking-anchor-${mode}`,
                child: {
                  create: {
                    selectedSystems: selectSystems(REACT_CHILD),
                    symbolicId: `react-tracking-${mode}`,
                  },
                },
                attachment: {
                  mode: "surface-camera",
                  output: { entity: "tracked-camera" },
                },
              },
              React.createElement(
                Entity,
                { id: "tracked-camera" },
                React.createElement(Camera),
              ),
              React.createElement(
                Entity,
                { id: "tracked-value" },
                React.createElement(Scalar, { value: amount }),
              ),
            ),
          ),
        ),
      );
    const binds = () => Object.fromEntries(bindsByMode);
    const reports = () =>
      trackingReported()
        .slice(priorReports)
        .map((error) => String((error.cause as Error | undefined)?.message));
    await trackingRoot.render(trackingScene(1));
    check(
      bindsByMode.get("closed") === 1 &&
        bindsByMode.get("kept") === 1 &&
        encoded(reports()) ===
          encoded(["Output watch refused by the test host"]),
      `Tracking mount did not reuse its watched outputs or report the refused watch once: ${encoded(binds())}; ${reports()}`,
    );
    closeWatch.resolve({
      kind: "closed",
      reason: new Error("Output watch closed by the test host"),
    });
    for (
      let attempt = 0;
      attempt < 200 && bindsByMode.get("closed") !== 2;
      attempt++
    )
      await new Promise((resolve) => setTimeout(resolve, 16));
    check(
      bindsByMode.get("closed") === 2 && bindsByMode.get("kept") === 1,
      `An ended output watch did not rebind its possibly stale output once: ${encoded(binds())}`,
    );
    const ended = { ...binds() };
    for (const amount of [2, 3])
      await trackingRoot.render(trackingScene(amount));
    const causes = reports();
    check(
      bindsByMode.get("closed") === ended.closed! + 2 &&
        bindsByMode.get("refused") === ended.refused! + 2 &&
        bindsByMode.get("kept") === 1 &&
        causes.length === 2 &&
        causes.some((cause) => cause.includes("closed by the test host")),
      `Ended output tracking did not fall back per attachment and report once: ${encoded(ended)} -> ${encoded(binds())}; ${causes}`,
    );
    await trackingRoot.render(null);
    report.push(
      "ended output tracking is reported once and binds only its own attachment on every child commit",
    );

    const foreignWorld = await create();
    const peer = await open(parentWorld);
    await requireSuccess(
      parent.batch([
        {
          kind: "create",
          alias: 0,
          metadata: { symbolicId: "bound-anchor", classes: [] },
        },
      ]),
    );
    const bound = findEntity(await parent.inspect(), "bound-anchor")!.id;
    const replacedRef = React.createRef<AttachedWorldHandle>();
    await root.render(
      React.createElement(AttachedWorld, {
        anchor: bound,
        child: { borrow: borrowed },
        attachment: { mode: "spatial" },
        ref: replacedRef,
      }),
    );
    const replaced = replacedRef.current!;
    const external = await requireSuccess(
      peer.batch([writeAttachment(peer, bound, foreignWorld)]),
    );
    await root.render(null);
    await replaced.closed;
    check(
      (await peer.attachmentRetirement(
        attachmentEffects(external)[0]!.receipt.id,
      )) === "pending",
      "Conditional detach cleared a foreign replacement",
    );
    await requireSuccess(
      peer.batch([
        {
          kind: "detachWorldAttachment",
          receipt: attachmentEffects(external)[0]!.receipt.id,
        },
      ]),
    );
    await peer.releaseAttachmentReceipt(
      attachmentEffects(external)[0]!.receipt.id,
    );
    report.push(
      "external replacement survives exact superseded conditional detach",
    );

    const retiringAdapter = hostMethods(host);
    const retiring = deferred<void>();
    const observeRetirement = deferred<void>();
    releaseGates.push(() => observeRetirement.resolve());
    retiringAdapter.openWorld = async (world) => {
      const client = await host.openWorld(world);
      if (world.id !== parentWorld.id) return client;
      return new Proxy(client, {
        get(target, property) {
          if (property === "attachmentRetirement")
            return async (receipt: bigint) => {
              const result = await target.attachmentRetirement(receipt);
              retiring.resolve();
              await observeRetirement.promise;
              return result;
            };
          const member: unknown = Reflect.get(target, property, target);
          return typeof member === "function" ? member.bind(target) : member;
        },
      });
    };
    const retiringRoot = rootFor(parent, retiringAdapter);
    const retiringRef = React.createRef<AttachedWorldHandle>();
    await retiringRoot.render(
      React.createElement(AttachedWorld, {
        anchor: bound,
        child: { borrow: borrowed },
        attachment: { mode: "spatial" },
        ref: retiringRef,
      }),
    );
    const retiringHandle = retiringRef.current!;
    await retiringRoot.render(null);
    await retiring.promise;
    let retired = false;
    void retiringHandle.closed.then(() => {
      retired = true;
    });
    const sibling = rootFor(parent);
    await sibling.render(
      React.createElement(Entity, { id: "independent-sibling" }),
    );
    check(
      !retired && findEntity(await parent.inspect(), "independent-sibling"),
      "Retirement monopolized the finite root commit queue",
    );
    observeRetirement.resolve();
    await retiringHandle.closed;
    await sibling.unmount();
    report.push(
      "delayed real retirement observation does not block independent declarations",
    );

    for (const phase of ["create", "open"] as const) {
      const arrived = deferred<WorldReference>();
      const release = deferred<void>();
      releaseGates.push(() => release.resolve());
      const adapter = hostMethods(host);
      let created: WorldReference | undefined;
      adapter.createWorld = async (options) => {
        const result = await host.createWorld(options);
        created = result.reference;
        worlds.push(result.reference);
        if (phase === "create") {
          arrived.resolve(result.reference);
          await release.promise;
        }
        return result;
      };
      adapter.openWorld = async (world) => {
        const client = await host.openWorld(world);
        if (phase === "open" && world.id === created?.id) {
          arrived.resolve(world);
          await release.promise;
        }
        return client;
      };
      const pendingRoot = rootFor(parent, adapter);
      let lateReady = false;
      const rendering = pendingRoot.render(
        React.createElement(AttachedWorld, {
          anchor: bound,
          child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
          attachment: { mode: "spatial" },
          onReady: () => {
            lateReady = true;
          },
        }),
      );
      const createdWorld = await arrived.promise;
      const cleanup = pendingRoot.unmount();
      release.resolve();
      await Promise.all([rendering, cleanup]);
      // Unmount destroys no World, not even one the boundary created.
      check(
        !lateReady && (await exists(createdWorld)),
        `Unmount during ${phase} destroyed its created World or emitted a stale callback`,
      );
      report.push(
        `unmount during acknowledged ${phase} releases without destroying the created World`,
      );
    }

    const failureAdapter = hostMethods(host);
    let failedWorld: WorldReference | undefined;
    failureAdapter.createWorld = async (options) => {
      const created = await host.createWorld(options);
      failedWorld = created.reference;
      worlds.push(created.reference);
      return created;
    };
    failureAdapter.openWorld = async (world) => {
      if (world.id === failedWorld?.id)
        throw new Error("deliberate failure before child open submission");
      return host.openWorld(world);
    };
    const failedRoot = rootFor(parent, failureAdapter);
    await failedRoot.render(
      React.createElement(AttachedWorld, {
        anchor: bound,
        child: { create: { selectedSystems: selectSystems(REACT_CHILD) } },
        attachment: { mode: "spatial" },
        onError: () => {},
      }),
    );
    await failedRoot.unmount();
    check(
      failedWorld && !(await exists(failedWorld)),
      "Open failure lost created World identity",
    );
    report.push(
      "open failure destroys only its already acknowledged creator identity",
    );

    await exerciseReactBoundaries(parent, borrowed, observer, rootFor, report);

    const doomedWorld = await create();
    const doomedClient = await open(doomedWorld);
    const doomedRoot = rootFor(doomedClient);
    const orphanRef = React.createRef<AttachedWorldHandle>();
    await doomedRoot.render(
      React.createElement(
        React.Fragment,
        null,
        React.createElement(Entity, { id: "doomed-anchor" }),
        React.createElement(AttachedWorld, {
          anchor: "doomed-anchor",
          child: { borrow: borrowed },
          attachment: { mode: "spatial" },
          ref: orphanRef,
        }),
      ),
    );
    const orphan = orphanRef.current!;
    await host.destroyWorld(doomedWorld);
    await orphan.closed;
    check(
      orphanRef.current === null && (await exists(borrowed)),
      "Parent destruction damaged the borrowed child",
    );
    report.push(
      "exact parent absence permits receipt cleanup without borrowed child destruction",
    );
    report.push(...(await exerciseAttachedWorldFences(host)));
    return report;
  } finally {
    for (const release of releaseGates) release();
    await Promise.allSettled(roots.map((root) => root.unmount()));
    await Promise.allSettled(sessions.map((client) => client.close()));
    await Promise.allSettled(worlds.map((world) => host.destroyWorld(world)));
  }
}

async function exerciseReactBoundaries(
  parent: Client,
  borrowed: WorldReference,
  observer: Client,
  rootFor: (client: Client) => ReturnType<typeof createRoot>,
  report: string[],
): Promise<void> {
  const root = rootFor(parent);
  const release = deferred<void>();
  let suspended = true;
  function Suspended() {
    if (suspended) throw release.promise;
    return React.createElement(Entity, { id: "suspense-child" });
  }
  const handle = React.createRef<AttachedWorldHandle>();
  await root.render(
    React.createElement(
      React.Fragment,
      null,
      React.createElement(Entity, { id: "suspense-anchor" }),
      React.createElement(
        React.Suspense,
        { fallback: React.createElement(Entity, { id: "suspense-fallback" }) },
        React.createElement(
          AttachedWorld,
          {
            anchor: "suspense-anchor",
            child: { borrow: borrowed },
            attachment: { mode: "spatial" },
            ref: handle,
          },
          React.createElement(Suspended),
        ),
      ),
    ),
  );
  check(
    handle.current === null &&
      findEntity(await parent.inspect(), "suspense-fallback"),
    "Suspense fallback did not preserve its parent World",
  );
  suspended = false;
  release.resolve();
  const deadline = performance.now() + 5000;
  while (!handle.current && performance.now() < deadline) {
    await root.flush();
    await new Promise<void>((resolve) => setTimeout(resolve, 16));
  }
  const revealed = handle.current as AttachedWorldHandle | null;
  check(
    revealed && findEntity(await observer.inspect(), "suspense-child"),
    "Suspense reveal failed across the World portal",
  );
  await root.render(null);
  await revealed.closed;
  report.push(
    "Suspense fallback and reveal retain React ancestry across fixed containers",
  );

  class Boundary extends React.Component<
    { children?: React.ReactNode },
    { failed: boolean }
  > {
    override state = { failed: false };
    static getDerivedStateFromError() {
      return { failed: true };
    }
    override render() {
      return this.state.failed
        ? React.createElement(Entity, { id: "error-fallback" })
        : this.props.children;
    }
  }
  function Failure(): React.ReactNode {
    throw new Error("child portal failure");
  }
  await root.render(
    React.createElement(
      React.Fragment,
      null,
      React.createElement(Entity, { id: "error-anchor" }),
      React.createElement(
        Boundary,
        null,
        React.createElement(
          AttachedWorld,
          {
            anchor: "error-anchor",
            child: { borrow: borrowed },
            attachment: { mode: "spatial" },
          },
          React.createElement(Failure),
        ),
      ),
    ),
  );
  check(
    findEntity(await parent.inspect(), "error-fallback"),
    "Child portal escaped its React error boundary",
  );
  check(
    !findEntity(await observer.inspect(), "error-fallback"),
    "Error fallback was retargeted into child World",
  );
  await root.unmount();
  report.push("child errors recover in the existing parent error boundary");
}
