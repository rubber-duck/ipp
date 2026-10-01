import { createRef } from "react";
import type {
  Client,
  HostClientBase,
  AttachmentReceipt,
  WorldReference,
} from "@ipp/client";
import {
  AttachedWorld,
  Camera,
  Entity,
  Scalar,
  Surface,
  createRoot,
  type AttachedWorldHandle,
  type ReactCompositionHost,
} from "@ipp/react";
import { observeRootCutoff } from "./react-cutoff-observer.js";
import { findEntity, requireSuccess } from "../react/fixture-helpers.js";
import {
  REACT_ROOT,
  REACT_CHILD,
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

async function bounded<Value>(
  promise: Promise<Value>,
  message: string,
): Promise<Value> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), 3000);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

export async function attachedInitializationCutoffs(
  host: HostClientBase<Client>,
  mode: "unchanged" | "descriptor" | "later-parent" | "receipt-descriptor",
) {
  const parent = (
    await host.createWorld({ selectedSystems: selectSystems(REACT_ROOT) })
  ).reference;
  const client = await host.openWorld(parent);
  const created = gate();
  const releaseCreate = gate();
  const opened = gate();
  const releaseOpen = gate();
  const childAck = gate();
  const releaseChildAck = gate();
  const attachmentAck = gate();
  const releaseAttachmentAck = gate();
  const laterParentAck = gate();
  const releaseLaterParentAck = gate();
  const reference = createRef<AttachedWorldHandle>();
  let child: WorldReference | undefined;
  let receipt: AttachmentReceipt | undefined;
  let receiptCount = 0;
  const errors: Error[] = [];
  const adapter: ReactCompositionHost = {
    get sessions() {
      return host.sessions;
    },
    createWorld: async (options) => {
      const result = await host.createWorld(options);
      child = result.reference;
      created.resolve();
      await releaseCreate.promise;
      return result;
    },
    openWorld: async (world) => {
      const session = await host.openWorld(world);
      if (world.id !== parent.id) {
        const original = session.batch.bind(session);
        let first = true;
        session.batch = async (...args) => {
          const outcome = await requireSuccess(original(...args));
          if (first) {
            first = false;
            childAck.resolve();
            await releaseChildAck.promise;
          }
          return outcome;
        };
        opened.resolve();
        await releaseOpen.promise;
      } else {
        const original = session.batch.bind(session);
        session.batch = async (...args) => {
          const outcome = await requireSuccess(original(...args));
          const written = outcome.effects.find(
            (effect) => effect.kind === "written",
          );
          if (written?.kind === "written") {
            receipt = written.receipt;
            receiptCount++;
            attachmentAck.resolve();
            await releaseAttachmentAck.promise;
          }
          return outcome;
        };
      }
      return session;
    },
    destroyWorld: (world) => host.destroyWorld(world),
    listWorlds: () => host.listWorlds(),
    bindOutput: (world, entity, kind) => host.bindOutput(world, entity, kind),
    resolveOutput: (output) => host.resolveOutput(output),
  };
  const root = createRoot(client, {
    host: adapter,
    onError: (error) => errors.push(error),
  });
  const boundary = (value: number) => (
    <AttachedWorld
      anchor={
        mode === "descriptor" && value > 0
          ? "cutoff-next-anchor"
          : "cutoff-anchor"
      }
      child={{ create: { selectedSystems: selectSystems(REACT_CHILD) } }}
      attachment={
        mode === "receipt-descriptor" && value > 0
          ? { mode: "surface-camera", output: { entity: "cutoff-child" } }
          : { mode: "spatial" }
      }
      ref={reference}
    >
      <Entity id="cutoff-child">
        <Scalar value={7} />
        <Camera />
      </Entity>
    </AttachedWorld>
  );
  const scene = (value: number, attached: boolean) => (
    <>
      <Entity id="cutoff-anchor">
        <Surface width={1} height={1} />
      </Entity>
      <Entity id="cutoff-next-anchor" />
      <Entity id="cutoff-unrelated">
        <Scalar value={value} />
      </Entity>
      {attached ? boundary(value) : null}
    </>
  );
  const observers: ReturnType<typeof observeRootCutoff>[] = [];
  const renders: Promise<void>[] = [];
  try {
    await root.render(scene(0, false));
    const observeA = observeRootCutoff("cutoff-anchor");
    observers.push(observeA);
    let finishedA = false;
    const renderingA = root.render(scene(0, true)).then(() => {
      finishedA = true;
    });
    renders.push(renderingA);
    await bounded(
      observeA.sealed,
      "Cutoff A did not invoke the shared checkpoint module",
    );
    await bounded(created.promise, "Child create did not reach its real ACK");
    if (mode === "receipt-descriptor") {
      releaseCreate.resolve();
      await bounded(opened.promise, "Child open did not acknowledge");
      releaseOpen.resolve();
      await bounded(childAck.promise, "Child declarations did not acknowledge");
      releaseChildAck.resolve();
      await bounded(
        attachmentAck.promise,
        "First generation did not reach its held attachment ACK",
      );
    }
    const observeB = observeRootCutoff("cutoff-anchor");
    observers.push(observeB);
    let finishedB = false;
    const renderingB = root.render(scene(1, true)).then(() => {
      finishedB = true;
    });
    renders.push(renderingB);
    const cutoffB = await bounded(
      observeB.sealed,
      "Cutoff B did not seal after the parent update",
    );
    await bounded(cutoffB.acknowledged, "Parent update B did not acknowledge");
    let finishedLater = false;
    let later: Promise<void> | undefined;
    if (mode === "later-parent" || mode === "receipt-descriptor") {
      const original = client.batch.bind(client);
      client.batch = async (...args) => {
        const result = await requireSuccess(original(...args));
        laterParentAck.resolve();
        await releaseLaterParentAck.promise;
        return result;
      };
      later = root.render(scene(2, true)).then(() => {
        finishedLater = true;
      });
      renders.push(later);
      await bounded(
        laterParentAck.promise,
        "Later parent work did not reach its held ACK",
      );
    }
    releaseCreate.resolve();
    await bounded(opened.promise, "Child open did not acknowledge");
    releaseOpen.resolve();
    await bounded(
      childAck.promise,
      "Initial child declarations did not acknowledge",
    );
    await client.inspect();
    check(
      !finishedA && !finishedB,
      "Overlapping root cutoffs resolved before the unchanged boundary's child ACK",
    );
    if (mode !== "receipt-descriptor")
      check(
        reference.current === null && receiptCount === 0,
        "Boundary escaped the held child declaration ACK",
      );
    releaseChildAck.resolve();
    await bounded(
      attachmentAck.promise,
      "Attachment did not reach a real written receipt ACK",
    );
    await client.inspect();
    check(
      (mode === "descriptor" || !finishedA) &&
        !finishedB &&
        reference.current === null,
      "Root cutoff/ref escaped the held attachment receipt ACK",
    );
    releaseAttachmentAck.resolve();
    await bounded(
      Promise.all([renderingA, renderingB]),
      "Overlapping cutoffs did not finish their finite requirements",
    );
    if (later) {
      check(
        !finishedLater,
        "Later unrelated parent ACK did not remain held across the earlier cutoffs",
      );
    }
    const handle = reference.current as AttachedWorldHandle | null;
    check(
      handle &&
        child &&
        handle.world.id === child.id &&
        handle.world.incarnation === child.incarnation,
      "Cutoffs did not publish the exact acknowledged child handle",
    );
    const anchor =
      mode === "descriptor" ? "cutoff-next-anchor" : "cutoff-anchor";
    check(
      receipt &&
        Number(receiptCount) === (mode === "receipt-descriptor" ? 2 : 1) &&
        receipt.anchor === findEntity(await client.inspect(), anchor)?.id &&
        receipt.parent.id === parent.id &&
        receipt.parent.incarnation === parent.incarnation &&
        receipt.child?.id === child.id &&
        receipt.child.incarnation === child.incarnation,
      "Cutoffs did not retain one real attachment receipt for their exact Worlds and anchor",
    );
    if (mode === "receipt-descriptor")
      check(
        handle.output?.kind === "camera",
        "Later parent work erased the captured attachment descriptor update",
      );
    if (later) {
      releaseLaterParentAck.resolve();
      await bounded(
        later,
        "Later cutoff did not finish after releasing its own ACK",
      );
    }
    const acknowledged = { mode, child, receipt };
    const childId = child.id;
    // Unmount releases a boundary without cleanup; removing it destroys the
    // child World it created before its handle closes.
    await root.render(null);
    await handle.closed;
    check(
      !(await host.listWorlds()).some((world) => world.id === childId),
      "Cutoff initialization leaked its creator-owned child",
    );
    await root.unmount();
    check(
      errors.length === 0,
      `Unexpected cutoff errors: ${errors.map((error) => error.message).join("; ")}`,
    );
    return acknowledged;
  } finally {
    for (const observer of observers) observer.restore();
    for (const release of [
      releaseCreate,
      releaseOpen,
      releaseChildAck,
      releaseAttachmentAck,
      releaseLaterParentAck,
    ])
      release.resolve();
    await Promise.allSettled(renders);
    await root.unmount().catch(() => {});
    await client.close();
    if (
      child &&
      (await host.listWorlds()).some((world) => world.id === child!.id)
    )
      await host.destroyWorld(child);
    await host.destroyWorld(parent);
  }
}
