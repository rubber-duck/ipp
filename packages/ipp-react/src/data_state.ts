/** Producer ownership records, independent of World commands and row storage. */
import type { DatasetProducer } from "@ipp/client";
import type { ReactWorldClient } from "./contract.js";
import type { DataSourceDescription, ReactDataSourceState } from "./data.js";

interface Entry {
  description: DataSourceDescription;
  state: ReactDataSourceState;
  producer?: DatasetProducer;
}

export class ReactDataSourceRegistry {
  private readonly session: bigint;
  private readonly entries = new Map<string, Entry>();
  private desired = new Map<string, DataSourceDescription>();
  private readonly listeners = new Set<(state: ReactDataSourceState) => void>();
  private closed = false;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly report: (error: unknown) => unknown,
  ) {
    this.session = client.session;
  }

  setDesired(sources: readonly DataSourceDescription[]): void {
    this.desired = new Map(
      sources.map((source) => [source.props.name, source]),
    );
  }

  get(name: string): ReactDataSourceState | undefined {
    return this.entries.get(name)?.state;
  }

  subscribe(listener: (state: ReactDataSourceState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private publish(entry: Entry): void {
    if (this.closed) return;
    for (const listener of this.listeners) {
      try {
        listener(entry.state);
      } catch (error) {
        this.report(error);
      }
    }
  }

  private matches(
    left: DataSourceDescription | undefined,
    right: DataSourceDescription,
  ): boolean {
    return (
      left?.signature === right.signature &&
      left.props.datasets === right.props.datasets
    );
  }

  private fence(entry: Entry): void {
    if (
      this.entries.get(entry.description.props.name) !== entry ||
      this.closed ||
      this.client.session !== this.session ||
      this.client.closure ||
      !this.matches(
        this.desired.get(entry.description.props.name),
        entry.description,
      )
    )
      throw new Error("Data source declaration or session is no longer active");
  }

  private async release(name: string, entry: Entry): Promise<void> {
    const props = entry.description.props;
    if (props.ownership === "producer" && entry.producer)
      await props.datasets.release(entry.producer);
    this.entries.delete(name);
  }

  /** Prepare declarations before authoring bindings; no samples are sent here. */
  async prepare(sources: readonly DataSourceDescription[]): Promise<void> {
    for (const description of sources) {
      if (this.closed) return;
      const props = description.props;
      const previous = this.entries.get(props.name);
      if (
        previous &&
        this.matches(previous.description, description) &&
        previous.state.status !== "failed"
      )
        continue;
      if (previous) await this.release(props.name, previous);
      if (this.closed) return;
      const entry: Entry = {
        description,
        state: Object.freeze({
          name: props.name,
          ownership: props.ownership,
          status: props.ownership === "borrowed" ? "borrowed" : "preparing",
        }),
      };
      this.entries.set(props.name, entry);
      if (this.matches(this.desired.get(props.name), description))
        this.publish(entry);
      if (props.ownership === "borrowed") continue;
      try {
        const producer = await props.datasets.create(
          props.name,
          props.kind,
          props.schema,
        );
        entry.producer = producer;
        // Keep the acknowledged producer for subsequent cleanup even when
        // a newer commit or unmount superseded this preparation.
        const handle = Object.freeze({
          name: props.name,
          producer,
          update: (deltas: Parameters<typeof props.datasets.update>[1]) => {
            this.fence(entry);
            return props.datasets.update(producer, deltas);
          },
        });
        entry.state = Object.freeze({
          name: props.name,
          ownership: "producer",
          status: "ready",
          handle,
        });
        if (this.matches(this.desired.get(props.name), description))
          this.publish(entry);
      } catch (error) {
        entry.state = Object.freeze({
          name: props.name,
          ownership: "producer",
          status: "failed",
          error: error instanceof Error ? error : new Error(String(error)),
        });
        if (this.matches(this.desired.get(props.name), description))
          this.publish(entry);
        throw error;
      }
    }
  }

  /** Release only after binding removal acknowledges. Borrowed sources are untouched. */
  async releaseUnused(
    sources: readonly DataSourceDescription[],
  ): Promise<void> {
    const retained = new Set(sources.map((source) => source.props.name));
    for (const [name, entry] of this.entries)
      if (!retained.has(name)) await this.release(name, entry);
  }

  close(): void {
    this.closed = true;
    this.listeners.clear();
  }

  async dispose(remove = false): Promise<void> {
    this.close();
    if (remove && this.client.session === this.session && !this.client.closure)
      for (const [name, entry] of this.entries) await this.release(name, entry);
  }
}
