import type { AnimationWorldClient, AssetResourceSnapshot } from "@ipp/client";
import {
  AnimationFixture,
  check,
  type AnimationContract,
  type AnimationRecord,
} from "../animation-fixtures.js";
import { pageCommands } from "../camera-fixtures.js";

/** Logical worlds and observations are independent of socket/process arrangement. */
export async function multipleWorldsShareAssets(
  connect: () => Promise<AnimationWorldClient>,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const left = await connect();
  const right = await connect();
  const a = new AnimationFixture(left, contract, record);
  const b = new AnimationFixture(right, contract, record);
  const shared = "ipp://mesh/cube?width=1&height=1&length=1";
  const privateSource = "ipp://mesh/cube?width=2&height=1&length=1";
  const rightEvents: AssetResourceSnapshot[] = [];
  const stop = right.onResourceChange((event) => rightEvents.push(event));
  let replacement: AnimationWorldClient | undefined;
  try {
    // Batch identities belong to each connection: an open batch on one never
    // absorbs or blocks another connection's batch.
    const open = left.openBatch();
    open.write(
      Array.from({ length: pageCommands(left) + 1 }, (_, index) => ({
        kind: "create" as const,
        alias: index,
        metadata: { symbolicId: `left-open-${index}`, classes: [] },
      })),
    );
    const peer = await right.batch([
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: "right-during-left-batch", classes: [] },
      },
    ]);
    check(peer.ok, "an open batch on another connection blocked this batch");
    check(
      !(await left.inspect()).entities.some((entity) =>
        entity.metadata.symbolicId?.startsWith("left-open-"),
      ),
      "a page of an unfinished batch applied",
    );
    const completed = await open.finish();
    check(
      completed.ok && completed.aliases.length === pageCommands(left) + 1,
      "the open batch lost pages",
    );
    await left.batch(
      completed.aliases.map(({ id }) => ({
        kind: "delete",
        entity: { kind: "handle", id },
      })),
    );
    await right.batch([
      { kind: "delete", entity: { kind: "handle", id: peer.aliases[0]!.id } },
    ]);
    await record("host.command-batch-isolation", {
      leftBatch: open.batchId,
      peer,
      completed: completed.tick,
    });
    const ea = await a.create("same-symbol", {
      Scalar: { value: 99 },
      MeshInstance: { source: shared },
    });
    const eb = await b.create("same-symbol", {
      Scalar: { value: 77 },
      MeshInstance: { source: shared },
    });
    await a.create("left-only", { MeshInstance: { source: privateSource } });
    const ready = async (client: AnimationWorldClient, source: string) => {
      for (let attempt = 0; attempt < 120; attempt++) {
        const observation = await client.inspect();
        const resource = observation.resources.find(
          (item) => item.source === source,
        );
        if (resource?.status === "loaded") return resource;
        await client.waitForFrame(observation.tick);
      }
      throw new Error(`Resource did not become ready: ${source}`);
    };
    const ra = await ready(left, shared);
    const rb = await ready(right, shared);
    check(
      ra.id === rb.id,
      "Worlds must retain the same Host resource identity",
    );
    check(
      left.session !== right.session,
      "Connections require independent fences",
    );
    check(
      !(await right.inspect()).resources.some(
        (resource) => resource.source === privateSource,
      ),
      "Inspection must exclude another world's resource subscriptions",
    );
    check(
      !rightEvents.some((event) => event.source === privateSource),
      "Resource events must be world scoped",
    );

    // Identical producer IDs identify different immutable content in each
    // world. At x=.4375 the left curve has changed by 6 and the right by 56.
    const sourceA = await a.upload(a.curve("Scalar", "value", 0, 0), 501n);
    const sourceB = await b.upload(b.curve("Scalar", "value", 0, 100), 501n);
    const pa = await a.controller([
      a.driver(ea, sourceA, 0, "Scalar", ["value"]),
    ]);
    const pb = await b.controller([
      b.driver(eb, sourceB, 0, "Scalar", ["value"]),
    ]);
    const sampledA = await a.seekPaused(pa, 0.4375);
    const sampledB = await b.seekPaused(pb, 0.4375);
    check(
      Math.abs(a.value(sampledA, ea, "Scalar", "value") - 105) < 1e-4,
      "Left producer content was replaced",
    );
    check(
      Math.abs(b.value(sampledB, eb, "Scalar", "value") - 133) < 1e-4,
      "Right producer content crossed worlds",
    );
    await left.close();
    await right.waitForFrame(sampledB.tick);

    replacement = await connect();
    const c = new AnimationFixture(replacement, contract, record);
    await c.upload(c.curve("Scalar", "value", 200, 200), 501n);
    const survived = await right.inspect();
    check(
      survived.entities.length === 1,
      "Disconnect must preserve the other world's entities",
    );
    check(
      Math.abs(b.value(survived, eb, "Scalar", "value") - 133) < 1e-4,
      "Replacement producer changed surviving playback",
    );
    // Stopping the surviving controller subtracts its contribution from the
    // authored value.
    await right.controlAnimationController(pb, { action: "stop" });
    const stopped = await right.inspect();
    check(
      b.value(stopped, eb, "Scalar", "value") === 77,
      "Playback lost the authored value",
    );
    const retained = await ready(right, shared);
    check(
      retained.id === rb.id,
      "Disconnect must preserve shared demand and identity",
    );
    check(
      !rightEvents.some(
        (event) => event.source === shared && event.status === "unloaded",
      ),
      "One world's disconnect unloaded another world's asset",
    );
    await record("host.multiple-worlds", {
      ra,
      rb,
      retained,
      survived,
      rightEvents,
    });
    return { sharedResource: retained.id, survivingSession: right.session };
  } finally {
    stop();
    await Promise.all([left.close(), right.close(), replacement?.close()]);
  }
}
