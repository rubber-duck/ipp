import type { AnimationWorldClient, AssetResourceSnapshot } from "@ipp/client";
import {
  AnimationFixture,
  check,
  type AnimationContract,
  type AnimationRecord,
} from "../animation-fixtures.js";

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
    const stream = await left.beginBatch();
    const held = await left.batchChunk(stream, []);
    const peer = await right.inspect();
    await right.waitForFrame(peer.tick);
    await right.batchChunk(stream, []).then(
      () => {
        throw new Error("foreign World accepted batch identity");
      },
      () => {},
    );
    const stillHeld = await left.batchChunk(stream, []);
    check(
      held.tick === stillHeld.tick,
      "peer World progression evaluated the gated World",
    );
    await left.endBatch(stream);
    await record("host.command-batch-isolation", { held, peer, stillHeld });
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

    // Identical producer IDs identify different immutable content in each world.
    const sourceA = await a.upload(a.curve("Scalar", "value", 0, 0), 501n);
    const sourceB = await b.upload(b.curve("Scalar", "value", 100, 100), 501n);
    const pa = await a.controller([
      a.driver(ea, sourceA, 0, "Scalar", ["value"]),
    ]);
    const pb = await b.controller([
      b.driver(eb, sourceB, 0, "Scalar", ["value"]),
    ]);
    const sampledA = await a.seekPaused(pa, 0.4375);
    const sampledB = await b.seekPaused(pb, 0.4375);
    check(
      a.value(sampledA, ea, "Scalar", "value") === 6,
      "Left producer content was replaced",
    );
    check(
      b.value(sampledB, eb, "Scalar", "value") === 106,
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
      b.value(survived, eb, "Scalar", "value") === 106,
      "Replacement producer changed surviving playback",
    );
    check(
      b.value(survived, eb, "Scalar", "value", "base") === 77,
      "Playback changed authored base",
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
