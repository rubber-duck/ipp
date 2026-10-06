import type {
  Client,
  WorldCreateOptions,
  WorldLoadOptions,
  WorldGraphLoadResult,
  WorldReference,
} from "@ipp/client";
import type { CanvasHost, CanvasPresentationJournal } from "./presentation.js";

export type CanvasWorldSource =
  | { create: WorldCreateOptions; load?: never }
  | {
      load: { url: string; options?: Omit<WorldLoadOptions, "signal"> };
      create?: never;
    };

export interface CanvasCleanupJournal {
  readonly hostOwned: boolean;
  readonly session: bigint | undefined;
  readonly sessionOwned: boolean;
  readonly presentation: CanvasPresentationJournal | undefined;
  readonly failures: readonly unknown[];
}

export interface CanvasCleanupRecovery {
  readonly journal: CanvasCleanupJournal;
  retry(): Promise<void>;
  /** Explicitly relinquish cleanup, without claiming its completion. */
  abandon(): Promise<CanvasCleanupJournal>;
}

export class CanvasCleanupError extends AggregateError {
  constructor(
    errors: readonly unknown[],
    readonly recovery: CanvasCleanupRecovery,
  ) {
    super(
      errors,
      "Canvas cleanup is incomplete; its owner is retained for recovery",
    );
    this.name = "CanvasCleanupError";
  }
}

/**
 * The Canvas's sessions and owned Host. Closing unmounts its roots and closes
 * what it owns; like unmount, it destroys no World, so a World it created
 * stays until its Host closes.
 */
export class CanvasLifetime implements CanvasCleanupRecovery {
  client: Client | undefined;
  private sessionOwned = false;
  private failures: readonly unknown[] = [];
  private cleanup:
    | {
        close(retry: boolean): Promise<void>;
        abandon(): Promise<void>;
        journal(): CanvasPresentationJournal;
      }
    | undefined;
  private attempt: Promise<void> | undefined;
  private abandoned: Promise<CanvasCleanupJournal> | undefined;
  private completed = false;

  constructor(
    readonly host: CanvasHost,
    readonly hostOwned: boolean,
  ) {}

  get journal(): CanvasCleanupJournal {
    return Object.freeze({
      hostOwned: this.hostOwned,
      session: this.client?.session,
      sessionOwned: this.sessionOwned,
      presentation: this.cleanup?.journal(),
      failures: Object.freeze([...this.failures]),
    });
  }

  adopt(client: Client, owned: boolean): void {
    if (this.client) throw new Error("Canvas already has an authoring session");
    if (this.host.sessions.get(client.session) !== client)
      throw new Error("Canvas requires a live authoring session from its Host");
    this.client = client;
    this.sessionOwned = owned;
  }

  manage(cleanup: NonNullable<CanvasLifetime["cleanup"]>): void {
    this.cleanup = cleanup;
  }

  async open(source: CanvasWorldSource, signal?: AbortSignal): Promise<Client> {
    signal?.throwIfAborted();
    let world: WorldReference;
    if (source.create)
      world = (await this.host.createWorld(source.create)).reference;
    else {
      const host = this.host as CanvasHost & {
        loadWorld?: (
          bytes: Uint8Array,
          options: WorldLoadOptions,
        ) => Promise<WorldGraphLoadResult>;
      };
      if (!host.loadWorld)
        throw new Error(
          "Canvas graph loading requires generated snapshot support",
        );
      const response = await fetch(source.load.url, signal ? { signal } : {});
      if (!response.ok)
        throw new Error(`Unable to load World graph: HTTP ${response.status}`);
      const bytes = new Uint8Array(await response.arrayBuffer());
      signal?.throwIfAborted();
      const graph = await host.loadWorld(bytes, {
        ...source.load.options,
        ...(signal ? { signal } : {}),
      });
      world = graph.root;
    }
    signal?.throwIfAborted();
    const client = await this.host.openWorld(world);
    this.adopt(client, true);
    signal?.throwIfAborted();
    return client;
  }

  close(): Promise<void> {
    return this.perform(false);
  }

  retry(): Promise<void> {
    if (!this.failures.length && !this.attempt && !this.completed)
      return Promise.reject(new Error("Canvas cleanup has not failed"));
    return this.perform(true);
  }

  private perform(retry: boolean): Promise<void> {
    if (this.abandoned)
      return Promise.reject(
        new Error("Canvas cleanup was explicitly abandoned"),
      );
    if (this.attempt) return this.attempt;
    if (this.completed) return Promise.resolve();
    const work = (async () => {
      try {
        await this.cleanup?.close(retry);
        if (this.sessionOwned && this.client && !this.client.closure)
          await this.client.close();
        if (this.hostOwned) await this.host.close();
        this.completed = true;
        this.failures = [];
      } catch (error) {
        this.failures = [error];
        throw new CanvasCleanupError(this.failures, this);
      }
    })();
    this.attempt = work;
    void work
      .finally(() => {
        this.attempt = undefined;
      })
      .catch(() => {});
    return work;
  }

  abandon(): Promise<CanvasCleanupJournal> {
    if (this.abandoned) return this.abandoned;
    if (this.attempt)
      return Promise.reject(new Error("Canvas cleanup is still in progress"));
    if (!this.failures.length)
      return Promise.reject(new Error("Canvas cleanup has not failed"));
    const journal = this.journal;
    const work = (async () => {
      await this.cleanup?.abandon();
      if (this.hostOwned) await this.host.close();
      else if (this.sessionOwned && this.client && !this.client.closure)
        await this.client.close();
      return journal;
    })();
    this.abandoned = work;
    void work.catch(() => {
      this.abandoned = undefined;
    });
    return work;
  }
}
