/** Real Surface fixture with Host-owned profiling controls; rendering entry points are reused. */
import { hostProfiling } from "../../packages/ipp-client/src/profiling.js";
import { surfaceFixture, clearWorkload } from "./surface-fixture.js";
export * from "./surface-fixture.js";

let captureId: string | undefined;
export async function profileStart(
  gpu: "off" | "frame" | "passes" | "all",
  glCalls = false,
) {
  const id = await hostProfiling(surfaceFixture.host).start({
    counters: false,
    gpu,
    glCalls,
  });
  captureId = id;
  return id;
}

export function profileStop() {
  return hostProfiling(surfaceFixture.host).stop();
}

export async function profileRelease() {
  if (captureId !== undefined)
    await hostProfiling(surfaceFixture.host).release(captureId);
  captureId = undefined;
}

export async function profileWorldIds() {
  return (await surfaceFixture.host.listWorlds()).map((world) =>
    String(world.id),
  );
}

/** Destroy only this fixture's attached Canvas Worlds after removing their Surfaces. */
export async function profileDestroyWorkloadWorlds() {
  const root = surfaceFixture.client.worldReference!.id;
  const descriptors = await surfaceFixture.host.listWorlds();
  const references = await Promise.all(
    descriptors
      .filter((world) => world.id !== root)
      .map((world) => surfaceFixture.host.resolveWorld(world.id)),
  );
  await clearWorkload();
  for (const reference of references)
    await surfaceFixture.host.destroyWorld(reference);
  return references.map((reference) => String(reference.id));
}
