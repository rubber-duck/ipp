/** Semantic ownership scenario shared by real generated native and worker Hosts. */
import type { WorldReference } from "@ipp/client";
import {
  hostProfiling,
  type ProfileCapture,
} from "../../../packages/ipp-client/src/testing.js";

interface ProfileScenarioClient {
  inspect(): Promise<unknown>;
  close(): Promise<void>;
}

interface ProfileScenarioHost {
  createWorld(options: {
    selectedSystems: readonly string[];
    temporary: boolean;
  }): Promise<{ reference: WorldReference }>;
  openWorld(world: WorldReference): Promise<ProfileScenarioClient>;
  destroyWorld(world: WorldReference): Promise<void>;
}

function require(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`Host profiling scenario: ${message}`);
}

export async function exerciseHostProfileOwnership(
  host: ProfileScenarioHost,
  selectedSystems: readonly string[],
  options: { growMemory?: () => Promise<void> | void } = {},
) {
  require(selectedSystems.length >=
    2, "fixture must select two or more Systems");
  const profiler = hostProfiling(host);
  require((await profiler.status()).available, "instrumentation unavailable");
  const worlds: WorldReference[] = [];
  const clients: ProfileScenarioClient[] = [];
  let captureId: string | undefined;
  const create = async (systems: readonly string[]) => {
    const world = (
      await host.createWorld({ selectedSystems: systems, temporary: true })
    ).reference;
    worlds.push(world);
    const client = await host.openWorld(world);
    clients.push(client);
    return { world, client };
  };
  try {
    const first = await create(selectedSystems);
    const second = await create(selectedSystems);
    const different = await create(selectedSystems.slice(0, -1));
    captureId = await profiler.start({ counters: true });
    await first.client.inspect();
    await second.client.inspect();
    await different.client.inspect();
    await options.growMemory?.();
    await host.destroyWorld(first.world);
    worlds.splice(worlds.indexOf(first.world), 1);
    const replacement = await create(selectedSystems);
    await replacement.client.inspect();
    await second.client.inspect();
    const artifact = await profiler.stop();
    const records = (reference: WorldReference) =>
      artifact.stages.filter(
        (stage) =>
          stage.kind === "system" &&
          stage.identity.worldId === reference.id.toString() &&
          stage.identity.incarnation === reference.incarnation.toString(),
      );
    const one = records(first.world);
    const two = records(second.world);
    const other = records(different.world);
    const recreated = records(replacement.world);
    require(one.length &&
      two.length &&
      other.length &&
      recreated.length, "all issued World lifetimes need semantic stage observations");
    require(one[0]!.identity.compositionId ===
      two[0]!.identity
        .compositionId, "same selected composition changed identity");
    require(one[0]!.identity.compositionId !==
      other[0]!.identity.compositionId, "distinct selected composition merged");
    require(first.world.incarnation !==
      replacement.world.incarnation, "recreated World reused incarnation");
    require(one.every(
      (record) => record.identity.hostId === artifact.hostId,
    ), "retired identity lost Host ownership");
    require(artifact.categories.reduce(
      (sum, category) => sum + BigInt(category.allocationCalls),
      0n,
    ) ===
      BigInt(
        artifact.allocations.calls,
      ), "exclusive allocation calls disagree");
    require(artifact.categories.reduce(
      (sum, category) => sum + BigInt(category.requestedBytes),
      0n,
    ) ===
      BigInt(
        artifact.allocations.requestedBytes,
      ), "exclusive requested bytes disagree");
    return {
      artifact,
      worlds: [first, second, different, replacement].map(({ world }) => ({
        id: world.id.toString(),
        incarnation: world.incarnation.toString(),
      })),
      memoryGrowthRequested: options.growMemory !== undefined,
    } satisfies {
      artifact: ProfileCapture;
      worlds: { id: string; incarnation: string }[];
      memoryGrowthRequested: boolean;
    };
  } finally {
    if (captureId !== undefined) await profiler.release(captureId);
    await Promise.allSettled(clients.map((client) => client.close()));
    for (const world of worlds) await host.destroyWorld(world);
  }
}
