/** Session-scoped asset preparation and ready-gated selections. */
import type { AssetResourceSnapshot, ClientAssetSource } from "@ipp/client";
import type { ReactWorldClient } from "./contract.js";
import type { AssetDescription } from "./assets.js";

export interface ReactAssetState {
  readonly id: string;
  readonly status: AssetResourceSnapshot["status"];
  /** Last successfully loaded selection, absent before the first success. */
  readonly current?: Readonly<ClientAssetSource>;
  readonly pendingSource: string;
  readonly error?: string | undefined;
}

type Entry = {
  description: AssetDescription;
  state: ReactAssetState;
  registrationFailed?: boolean;
};

export class ReactAssetRegistry {
  private readonly scope = globalThis.crypto.randomUUID();
  private readonly session: bigint;
  private readonly entries = new Map<string, Entry>();
  private readonly owned = new Map<string, ClientAssetSource>();
  private desired = new Map<string, AssetDescription>();
  private readonly listeners = new Set<(state: ReactAssetState) => void>();
  private readonly unsubscribe: (() => void) | undefined;
  private closed = false;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly changed: () => void,
    private readonly report: (error: unknown) => unknown,
  ) {
    this.session = client.session;
    this.unsubscribe = client.onResourceChange?.((resource) =>
      this.observe(resource),
    );
  }

  setDesired(assets: readonly AssetDescription[]): void {
    this.desired = new Map(assets.map((asset) => [asset.id, asset]));
  }

  get(id: string): ReactAssetState | undefined {
    return this.entries.get(id)?.state;
  }

  subscribe(listener: (state: ReactAssetState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private publish(entry: Entry): void {
    for (const listener of this.listeners) {
      try {
        listener(entry.state);
      } catch (error) {
        this.report(error);
      }
    }
  }

  private matches(
    a: AssetDescription | undefined,
    b: AssetDescription,
  ): boolean {
    return (
      a?.kind === b.kind &&
      a.variant === b.variant &&
      a.signature === b.signature
    );
  }

  private observe(resource: AssetResourceSnapshot): void {
    if (this.closed || this.client.session !== this.session) return;
    for (const entry of this.entries.values()) {
      if (
        entry.state.pendingSource !== resource.source ||
        resource.kind !== entry.description.kind ||
        resource.variant !== entry.description.variant ||
        !this.matches(this.desired.get(entry.description.id), entry.description)
      )
        continue;
      entry.state = Object.freeze({
        ...entry.state,
        status: resource.status,
        ...(resource.status === "loaded"
          ? {
              current: Object.freeze({
                kind: resource.kind,
                source: resource.source,
                variant: resource.variant,
              }),
              error: undefined,
            }
          : {}),
        ...(resource.status === "failed" ? { error: resource.error } : {}),
      });
      this.publish(entry);
      if (resource.status === "loaded") this.changed();
      if (resource.status === "failed")
        this.report(
          new Error(`Asset ${entry.description.id} failed: ${resource.error}`),
        );
    }
  }

  async prepare(assets: readonly AssetDescription[]): Promise<void> {
    if (
      assets.length &&
      (!this.client.registerAsset ||
        !this.client.releaseAsset ||
        !this.client.onResourceChange)
    )
      throw new Error("Asset declarations require named resource support");
    const retained = new Set(assets.map((asset) => asset.id));
    for (const id of this.entries.keys())
      if (!retained.has(id)) this.entries.delete(id);
    for (const description of assets) {
      const previous = this.entries.get(description.id);
      if (
        this.matches(previous?.description, description) &&
        !previous?.registrationFailed
      )
        continue;
      const source = `client://${this.session}/${this.scope}/${encodeURIComponent(description.id)}#${globalThis.crypto.randomUUID()}`;
      const resource = {
        kind: description.kind,
        source,
        variant: description.variant,
      };
      const entry: Entry = {
        description,
        state: Object.freeze({
          id: description.id,
          status: "start",
          pendingSource: source,
          ...(previous?.state.current
            ? { current: previous.state.current }
            : {}),
        }),
      };
      this.entries.set(description.id, entry);
      this.owned.set(source, resource);
      this.publish(entry);
      try {
        await this.client.registerAsset!(
          resource,
          description.bytes.slice().buffer,
        );
      } catch (error) {
        entry.registrationFailed = true;
        entry.state = Object.freeze({
          ...entry.state,
          status: "failed",
          error: String(error),
        });
        this.publish(entry);
        throw error;
      }
    }
  }

  /** Call after consumer mutations acknowledge, so old sources remain recoverable. */
  async releaseUnused(): Promise<void> {
    const used = new Set<string>();
    for (const { state } of this.entries.values()) {
      used.add(state.pendingSource);
      if (state.current) used.add(state.current.source);
    }
    for (const [source, resource] of this.owned)
      if (!used.has(source)) {
        await this.client.releaseAsset!(resource);
        this.owned.delete(source);
      }
  }

  close(): void {
    this.closed = true;
    this.unsubscribe?.();
    this.listeners.clear();
  }

  async dispose(): Promise<void> {
    this.close();
    this.entries.clear();
    if (this.client.session === this.session) await this.releaseUnused();
  }
}
