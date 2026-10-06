import type {
  AssetWorldClient,
  ClientAssetSource,
  AssetResourceSnapshot,
} from "@ipp/client";

/** Observe the actual provider/decoder lifecycle after source delivery acknowledges. */
export async function settledAsset(
  client: AssetWorldClient,
  source: ClientAssetSource,
): Promise<AssetResourceSnapshot> {
  const deadline = performance.now() + 10_000;
  for (;;) {
    const state = await client.inspect();
    const resource = state.resources.find(
      (resource) =>
        resource.kind === source.kind &&
        resource.source === source.source &&
        resource.variant === (source.variant ?? 0),
    );
    if (resource?.status === "loaded" || resource?.status === "failed")
      return resource;
    if (performance.now() >= deadline)
      throw new Error(`Asset did not settle: ${source.source}`);
    await client.waitForFrame(state.tick);
  }
}
