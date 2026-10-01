import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import {
  createContext,
  createRef,
  useContext,
  useLayoutEffect,
  useState,
} from "react";
import {
  canvasOutput,
  PresentationError,
  type Client,
  type HostClientBase,
  type PresentedCapture,
} from "@ipp/client";
import {
  createRoot,
  AttachedWorld,
  Camera,
  Entity,
  MeshInstance,
  Scalar,
  Transform,
  UnlitMaterial,
  type AttachedWorldHandle,
} from "@ipp/react";
import { CanvasWorldSession, CanvasCleanupError } from "@ipp/react/web";
import { presentationTesting } from "../../packages/ipp-client/src/testing.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import { capturedPixels } from "./canvas-fixture-helpers.js";
import { summarizeImage } from "./image-assertions.js";
import { observeRootCutoff } from "./react-cutoff-observer.js";
import { attachedInitializationCutoffs } from "./attached-cutoff.js";
import {
  REACT_ROOT,
  REACT_CHILD,
  LIFECYCLE,
  CONSTRAINTS,
  RENDER,
  CANVAS,
  selectSystems,
} from "../integration/system-selections.js";
export { nativePresentationTransport } from "../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../packages/ipp-client/src/worker.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const Placement = createContext(0);

function ChildMesh() {
  const offset = useContext(Placement);
  return (
    <Entity id="child-mesh">
      <Transform x={offset} />
      <MeshInstance source="ipp://mesh/cube?width=1&height=1&length=1" />
      <UnlitMaterial r={1} g={0} b={0} />
    </Entity>
  );
}

export async function canvasController(host: HostClientBase<Client>) {
  const parent = await host.createWorld({
    selectedSystems: selectSystems(REACT_ROOT),
    symbolicId: "react-controller-parent",
  });
  const canvas = await host.createWorld({
    selectedSystems: selectSystems(CANVAS, CONSTRAINTS, LIFECYCLE),
    symbolicId: "react-controller-canvas",
    canvas: { extent: [96, 64], unitsPerMetre: 100 },
  });
  const client = await host.openWorld(parent.reference);
  const canvasClient = await host.openWorld(canvas.reference);
  const canvasScope = createRoot(canvasClient, { host });
  const failures: Error[] = [];
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    view: PresentedCapture["view"];
    sequence: bigint;
    publication: PresentedCapture["publication"];
  }[] = [];
  const session = new CanvasWorldSession({
    host,
    client,
    onError: (error) => failures.push(error),
  });
  const root = session.createRoot();
  let child: AttachedWorldHandle | null = null;
  const childRef = (value: AttachedWorldHandle | null) => {
    child = value;
  };
  const scene = (offset: number) => (
    <Placement value={offset}>
      <Entity id="camera">
        <Transform z={6} />
        <Camera projection={1} ortho_height={4} near={0.1} far={100} />
      </Entity>
      <Entity id="anchor">
        <Transform />
      </Entity>
      <AttachedWorld
        anchor="anchor"
        child={{
          create: {
            selectedSystems: selectSystems(REACT_CHILD, RENDER),
            symbolicId: "react-controller-child",
          },
        }}
        attachment={{ mode: "spatial" }}
        ref={childRef}
      >
        <ChildMesh />
      </AttachedWorld>
    </Placement>
  );
  async function capture(
    label: string,
    ready: (image: PresentedCapture) => boolean,
  ) {
    const deadline = performance.now() + 15_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      if (ready(frame)) {
        const pixels = capturedPixels(frame);
        images.push({
          label,
          width: pixels.width,
          height: pixels.height,
          pixels: [...new Uint8Array(pixels.pixels)],
          view: frame.view,
          sequence: frame.sequence,
          publication: frame.publication,
        });
        return frame;
      }
      check(performance.now() < deadline, `Capture never reached ${label}`);
      sequence = frame.sequence;
    }
  }
  let foreign: Awaited<ReturnType<typeof host.presentation.select>> | undefined;
  let createdChild: AttachedWorldHandle["world"] | undefined;
  try {
    await root.render(scene(-0.7));
    check(child, "Real React portal did not become ready");
    const ownedChild = (child as AttachedWorldHandle).world;
    createdChild = ownedChild;
    check(
      (await host.getRootOutputBinding(parent.reference)) === null,
      "Authoring implicitly selected a root",
    );
    const output = await root.bindOutput("camera", "camera");
    check(
      (await host.getRootOutputBinding(parent.reference)) === null,
      "Binding an output selected presentation",
    );
    const viewport = { width: 96, height: 64, devicePixelRatio: 1 };
    const originalSet = host.setRootOutput.bind(host);
    let rootRequests = 0;
    host.setRootOutput = async (...args) => {
      rootRequests++;
      if (rootRequests === 1)
        throw Object.assign(new Error("Root binding not submitted"), {
          code: "IPP_REQUEST_NOT_SENT",
        });
      return originalSet(...args);
    };
    const originalSelect = host.presentation.select.bind(host.presentation);
    let selectRequests = 0;
    host.presentation.select = async (...args) => {
      selectRequests++;
      if (selectRequests === 1) throw new PresentationError("capacity");
      return originalSelect(...args);
    };
    for (let attempt = 0; attempt < 2; attempt++) {
      const error = await session.selectOutput(output, viewport).then(
        () => undefined,
        (error: unknown) => error,
      );
      check(error instanceof Error, "Expected known rejected selection");
    }
    await session.selectOutput(output, viewport);
    check(
      rootRequests === 2 && selectRequests === 2,
      "Corrected identical selection did not reuse its acknowledged root",
    );
    host.setRootOutput = originalSet;
    host.presentation.select = originalSelect;
    const left = await capture(
      "context-left",
      (frame) => summarizeImage(capturedPixels(frame)).foregroundPixels > 40,
    );
    const leftSummary = summarizeImage(capturedPixels(left));
    await finiteBarriers(session);
    const internalBarriers = await internalStateBarriers(session);
    const attachmentCutoffs = [];
    for (const mode of [
      "unchanged",
      "descriptor",
      "later-parent",
      "receipt-descriptor",
    ] as const)
      attachmentCutoffs.push(await attachedInitializationCutoffs(host, mode));
    await independentRetirement(session);
    await root.render(scene(0.7));
    const right = await capture(
      "context-right",
      (frame) =>
        (summarizeImage(capturedPixels(frame)).centroidX ?? 0) >
        leftSummary.centroidX! + 8,
    );
    check(
      right.view.selection === left.view.selection,
      "Authoring update rebound presentation",
    );
    check(
      (await host.listWorlds()).some((world) => world.id === ownedChild.id),
      "Portal child disappeared during update",
    );
    const sibling = await host.openWorld(parent.reference);
    await sibling.close();
    check(
      (await session.frame()).view.selection === left.view.selection,
      "Sibling session close invalidated presentation",
    );
    const recoveredBindings = await recoverySelectionRetry(session);

    const canvasEntity = { kind: "alias", alias: 1 } as const;
    const shape = { kind: "alias", alias: 2 } as const;
    const outcome = successfulBatch(
      await canvasClient.batch([
        createEntity(1, "explicit-canvas"),
        createEntity(2, "blue-shape"),
        insertComponent(canvasClient, "CanvasBox", shape, {
          width: 96,
          height: 64,
        }),
        insertComponent(canvasClient, "CanvasStyle", shape, {
          red: 0,
          green: 0,
          blue: 1,
        }),
        {
          kind: "placeEntity",
          entity: shape,
          placement: { parent: canvasEntity, before: null },
        },
      ]),
    );
    check(
      aliasId(outcome, 1) > 0n,
      "Canvas producer had no acknowledged entity",
    );
    await canvasScope.render(<Entity bindTo="explicit-canvas" />);
    // The canvas World's output names no entity and needs no binding.
    const explicitOutput = canvasOutput(canvas.reference);
    await session.selectOutput(explicitOutput, {
      width: 96,
      height: 64,
      devicePixelRatio: 1,
    });
    check(
      session.client.worldReference?.id === parent.id,
      "Selection retargeted the authoring session",
    );
    const blue = await capture(
      "explicit-canvas",
      (frame) => new Uint8Array(frame.pixels)[2]! > 250,
    );
    const replacement = await host.setRootOutput(
      explicitOutput,
      blue.view.binding.viewport,
    );
    foreign = await host.presentation.select(
      await host.presentation.surface(),
      replacement,
    );
    check(
      replacement.generation.serial !== blue.view.binding.generation.serial,
      "Same-value replacement lacked a fresh generation",
    );
    const recoveryError = await session.recoverPresentation().then(
      () => undefined,
      (error: unknown) => error,
    );
    check(
      recoveryError instanceof Error,
      "Recovery reclaimed an externally replaced root",
    );
    const originalClear = host.clearRootOutput.bind(host);
    let rejected = false;
    host.clearRootOutput = async (binding) => {
      if (!rejected) {
        rejected = true;
        throw Object.assign(new Error("Known-unsent root clear"), {
          code: "IPP_REQUEST_NOT_SENT",
        });
      }
      await originalClear(binding);
    };
    const originalBatch = client.batch.bind(client);
    // Teardown unmounts the Canvas roots, which delete nothing: it submits no
    // deletes and releases the portal boundary without destroying its child.
    let teardownDeletes = 0;
    client.batch = async (...args) => {
      if (args[0].some((operation) => operation.kind === "delete"))
        teardownDeletes++;
      return originalBatch(...args);
    };
    const cleanup = await session.close().then(
      () => undefined,
      (error: unknown) => error,
    );
    host.clearRootOutput = originalClear;
    check(
      cleanup instanceof CanvasCleanupError,
      "Failed teardown lost its recoverable Canvas owner",
    );
    check(
      cleanup.recovery.journal.presentation!.bindings.length > 0,
      "Cleanup discarded its exact root receipt",
    );
    check(
      host.sessions.has(client.session),
      "Canvas closed its borrowed authoring session",
    );
    await cleanup.recovery.retry();
    client.batch = originalBatch;
    check(teardownDeletes === 0, "Canvas teardown deleted authored entities");
    check(
      (await host.listWorlds()).some((world) => world.id === ownedChild.id),
      "Canvas teardown destroyed the portal child it only released",
    );
    check(
      (await host.listWorlds()).some((world) => world.id === parent.id),
      "Canvas destroyed a borrowed World",
    );
    check(
      (await host.getRootOutputBinding(canvas.reference))?.generation.serial ===
        replacement.generation.serial,
      "Conditional cleanup cleared a replacement root",
    );
    const after = await host.presentation.capture(foreign, {
      afterSequence: blue.sequence,
    });
    check(
      new Uint8Array(after.pixels)[2]! > 250,
      "Conditional cleanup cleared the replacement surface",
    );
    check(
      new Uint8Array(blue.pixels)[2]! > 250,
      "Completed capture mutated after teardown",
    );
    check(
      failures.includes(cleanup) &&
        failures.every((error) => error === cleanup),
      "Unexpected controller errors",
    );
    const diagnostics = await renderDiagnostics(host)?.statistics();
    return {
      images,
      parent: parent.reference,
      child: ownedChild,
      rootGeneration: replacement.generation,
      device: diagnostics?.device,
      recovery: cleanup.recovery.journal,
      internalBarriers,
      attachmentCutoffs,
      recoveredBindings,
    };
  } finally {
    await session.close().catch(async () => {
      await session.cleanup.retry().catch(() => {});
    });
    if (foreign) await host.presentation.clear(foreign);
    await canvasScope.unmount();
    await client.close();
    await canvasClient.close();
    await host.destroyWorld(parent.reference);
    await host.destroyWorld(canvas.reference);
    // Teardown released the portal child React created; the scenario that
    // created it destroys it.
    if (
      createdChild &&
      (await host.listWorlds()).some((world) => world.id === createdChild!.id)
    )
      await host.destroyWorld(createdChild);
  }
}

async function finiteBarriers(session: CanvasWorldSession) {
  const producer = session.createRoot();
  let value = 0;
  const scene = () => (
    <Entity id="finite-producer">
      <Scalar value={value++} />
    </Entity>
  );
  await producer.render(scene());
  const client = session.client;
  const original = client.batch.bind(client);
  try {
    for (const barrier of [
      () => session.flush(),
      () => producer.flush(),
      () => session.frame(),
      () => session.capture(),
    ]) {
      let release!: () => void;
      let arrived!: () => void;
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      const submitted = new Promise<void>((resolve) => {
        arrived = resolve;
      });
      let first = true;
      client.batch = async (...args) => {
        const outcome = await original(...args);
        if (first) {
          first = false;
          arrived();
          await held;
        }
        return outcome;
      };
      const cutoff = barrier();
      let completedLater = false;
      const later = producer.render(scene()).then(() => {
        completedLater = true;
      });
      const trailing: Promise<void>[] = [];
      const producerTimer = setInterval(() => {
        trailing.push(producer.render(scene()));
      }, 1);
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          Promise.all([cutoff, submitted]),
          new Promise<never>((_, reject) => {
            timer = setTimeout(
              () =>
                reject(
                  new Error(
                    "Finite Canvas ACK barrier waited for later producer work",
                  ),
                ),
              3000,
            );
          }),
        ]);
        check(
          !completedLater,
          "The later ACK was not held across the finite barrier",
        );
      } finally {
        clearInterval(producerTimer);
        clearTimeout(timer);
        release();
        await Promise.all([later, ...trailing]);
      }
    }
  } finally {
    client.batch = original;
    await producer.unmount();
  }
}

async function internalStateBarriers(session: CanvasWorldSession) {
  const producer = session.createRoot();
  let update!: (value: number) => void;
  let committed = 0;
  let onCommitted: (() => void) | undefined;
  function Producer() {
    const [value, setValue] = useState(0);
    update = setValue;
    useLayoutEffect(() => {
      committed = value;
      onCommitted?.();
    }, [value]);
    return (
      <Entity id="internal-producer">
        <Scalar value={value} />
      </Entity>
    );
  }
  await producer.render(<Producer />);
  const client = session.client;
  const original = client.batch.bind(client);
  let value = 0;
  const report: string[] = [];
  async function bounded<T>(work: Promise<T>, message: string): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        work,
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error(message)), 3000);
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  }
  try {
    for (const [name, barrier] of Object.entries({
      root: () => producer.flush(),
      canvas: () => session.flush(),
      frame: () => session.frame(),
      capture: () => session.capture(),
    })) {
      const releases: (() => void)[] = [];
      const arrivals: (() => void)[] = [];
      const held = Array.from(
        { length: 2 },
        () => new Promise<void>((resolve) => releases.push(resolve)),
      );
      const submitted = Array.from(
        { length: 2 },
        () => new Promise<void>((resolve) => arrivals.push(resolve)),
      );
      let dispatched = 0;
      client.batch = async (...args) => {
        const index = dispatched++;
        const outcome = await original(...args);
        successfulBatch(outcome);
        if (index < 2) {
          arrivals[index]!();
          await held[index];
        }
        return outcome;
      };
      update(++value);
      await bounded(
        submitted[0]!,
        "Internal update A never reached its real ACK",
      );
      const observer = observeRootCutoff("internal-producer");
      const cutoff = barrier();
      void cutoff.catch(() => {});
      let timer: ReturnType<typeof setInterval> | undefined;
      try {
        await bounded(
          observer.sealed,
          "Barrier did not invoke the shared checkpoint module",
        );
        await bounded(
          new Promise<void>((resolve) => {
            onCommitted = () => {
              if (committed === value) resolve();
            };
            update(++value);
          }),
          "Internal update B did not commit while A was held",
        );
        onCommitted = undefined;
        timer = setInterval(() => update(++value), 1);
        releases[0]!();
        await bounded(
          Promise.all([cutoff, submitted[1]]),
          "Root ACK cutoff waited for later internal setState B",
        );
        check(dispatched >= 2, "Later internal ACK was not held");
        report.push(name);
      } finally {
        observer.restore();
        onCommitted = undefined;
        clearInterval(timer);
        for (const release of releases) release();
        await cutoff.catch(() => {});
        await producer.flush();
      }
    }
  } finally {
    client.batch = original;
    await producer.unmount();
  }
  return report;
}

async function recoverySelectionRetry(session: CanvasWorldSession) {
  const host = session.host;
  check(
    renderDiagnostics(host),
    "Context recovery requires renderer diagnostics",
  );
  const testing = presentationTesting(host);
  const report = [];
  for (const retry of ["recover", "select"] as const) {
    const previous = session.view!;
    const pending = host.presentation.capture(previous, {
      afterSequence: 0xffff_ffff_ffff_ffffn,
    });
    const lost = pending.then(
      () => {
        throw new Error("Lost context completed a future capture");
      },
      () => {},
    );
    testing.loseContext();
    await lost;
    const dimensions = {
      width: previous.binding.viewport.width + 8,
      height: 64,
      devicePixelRatio: 1,
    };
    const output = previous.binding.output;
    const unavailable = await session.selectOutput(output, dimensions).then(
      () => undefined,
      (error: unknown) => error,
    );
    check(
      unavailable instanceof Error,
      "Lost-context resize unexpectedly selected",
    );
    testing.restoreContext();
    let restored = false;
    for (let attempt = 0; attempt < 200; attempt++) {
      try {
        restored =
          (await host.presentation.surface()).context !==
          previous.surface.context;
      } catch {}
      if (restored) break;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    check(restored, "Canvas context did not restore");
    const original = host.presentation.select.bind(host.presentation);
    const capacity = new PresentationError("capacity");
    host.presentation.select = async () => {
      throw capacity;
    };
    try {
      const rejected = await session.recoverPresentation().then(
        () => undefined,
        (error: unknown) => error,
      );
      check(
        rejected === capacity,
        "Expected known selection rejection after resized root ACK",
      );
    } finally {
      host.presentation.select = original;
    }
    const acknowledged = await host.getRootOutputBinding(output.world);
    check(
      acknowledged?.viewport.width === dimensions.width,
      "Resized root was not acknowledged before rejection",
    );
    check(
      session.cleanup.journal.presentation?.bindings.some(
        (binding) =>
          binding.generation.serial === acknowledged.generation.serial,
      ),
      "Recovery lost the acknowledged root journal",
    );
    if (retry === "recover") await session.recoverPresentation();
    else await session.selectOutput(output, dimensions);
    check(
      session.view?.binding.generation.serial ===
        acknowledged.generation.serial,
      "Recovery replayed an acknowledged root mutation",
    );
    const frame = await session.capture();
    check(
      frame.view.surface.context !== previous.surface.context &&
        frame.view.binding.viewport.width === dimensions.width,
      "Recovery did not draw its acknowledged resized root",
    );
    check(
      session.cleanup.journal.presentation?.bindings.length === 1,
      "Recovery retained obsolete root receipts",
    );
    report.push({
      retry,
      binding: acknowledged,
      view: frame.view,
      publication: frame.publication,
      sequence: frame.sequence,
    });
  }
  return report;
}

async function independentRetirement(session: CanvasWorldSession) {
  const host = session.host;
  const original = host.openWorld.bind(host);
  let release!: () => void;
  let arrived!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const retiring = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  host.openWorld = async (world) => {
    const client = await original(world);
    if (world.id !== session.client.worldReference!.id) return client;
    return new Proxy(client, {
      get(target, property) {
        if (property === "attachmentRetirement")
          return async (receipt: bigint) => {
            const result = await target.attachmentRetirement(receipt);
            arrived();
            await held;
            return result;
          };
        const member: unknown = Reflect.get(target, property, target);
        return typeof member === "function" ? member.bind(target) : member;
      },
    });
  };
  const boundary = session.createRoot();
  const sibling = session.createRoot();
  const reference = createRef<AttachedWorldHandle>();
  let removing: Promise<void> | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await boundary.render(
      <>
        <Entity id="retiring-anchor" />
        <AttachedWorld
          anchor="retiring-anchor"
          child={{
            create: {
              selectedSystems: selectSystems(REACT_CHILD, RENDER),
              symbolicId: "canvas-retirement-child",
            },
          }}
          attachment={{ mode: "spatial" }}
          ref={reference}
        />
      </>,
    );
    const handle = reference.current;
    check(handle !== null, "The retiring boundary published no handle");
    let retired = false;
    // Unmount releases a boundary without detaching it; removing the
    // boundary detaches it and waits for retirement before its handle closes.
    removing = Promise.all([
      boundary.render(<Entity id="retiring-anchor" />),
      handle.closed,
    ]).then(() => {
      retired = true;
    });
    await Promise.race([
      retiring,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Retirement observation did not arrive")),
          5000,
        );
      }),
    ]);
    clearTimeout(timer);
    const progress = (async () => {
      await sibling.render(
        <Entity id="retirement-independent">
          <Scalar value={7} />
        </Entity>,
      );
      await session.flush();
      await session.frame();
    })();
    await Promise.race([
      progress,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Retirement monopolized the Canvas queue")),
          3000,
        );
      }),
    ]);
    check(
      !retired,
      "Retirement was not held while sibling authoring/frame advanced",
    );
  } finally {
    clearTimeout(timer);
    release();
    host.openWorld = original;
    await removing;
    // Unmount deletes nothing; remove the probes' declarations first.
    await boundary.render(null);
    await sibling.render(null);
    await boundary.unmount();
    await sibling.unmount();
  }
}
