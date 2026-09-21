import { clientAssetSource, type AssetWorldClient } from "@ipp/client";
import { settledAsset } from "../integration/asset-fixtures.js";

export type SourceResult = {
  source: string;
  status: "loaded" | "failed";
  error?: string;
};

/** Fixture ingress uses the same named provider and lifecycle API as applications. */
export class AssetSourceFixture {
  constructor(private readonly client: AssetWorldClient) {}

  async register(
    kind: number,
    name: bigint,
    bytes: ArrayBuffer,
  ): Promise<SourceResult> {
    const source = clientAssetSource(this.client.session, kind, name);
    try {
      await this.client.registerAsset(source, bytes);
    } catch (error) {
      return { source: source.source, status: "failed", error: String(error) };
    }
    const result = await settledAsset(this.client, source);
    if (result.status !== "loaded" && result.status !== "failed")
      throw new Error("Unsettled asset");
    return {
      source: result.source,
      status: result.status,
      ...(result.error ? { error: result.error } : {}),
    };
  }
}
