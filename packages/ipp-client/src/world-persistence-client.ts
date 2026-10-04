import { readBulkReference } from "./bulk-reads.js";
import type { Client } from "./client.js";
import { HostClientBase } from "./host-client.js";
import type { WorldCapacityHintsPatch } from "./host-protocol.js";
import type { WorldReference } from "./types.js";
import { readWorldReference } from "./references.js";

export interface WorldTransferOptions {
  signal?: AbortSignal;
  /** Bounds the completed owned result. Defaults to 64 MiB. */
  maxBytes?: number;
}

export interface WorldLoadOptions extends WorldTransferOptions {
  symbolicId?: string;
  capacityHints?: WorldCapacityHintsPatch;
  /** Evaluated after inspecting this exact upload, before any World is created. */
  worldNames?:
    | ReadonlyMap<number, string>
    | ((
        graph: WorldGraphDescriptor,
      ) => ReadonlyMap<number, string> | Promise<ReadonlyMap<number, string>>);
}

export interface WorldGraphDescriptor {
  readonly root: number;
  readonly nodes: readonly {
    readonly id: number;
    readonly symbolicId: string;
    readonly persistentId: bigint;
  }[];
}

export interface WorldGraphLoadResult {
  readonly root: WorldReference;
  readonly created: ReadonlyMap<number, WorldReference>;
}

/** Retains every known identity if acknowledgement or later cancellation cleanup fails. */
export class WorldGraphLoadError extends Error {
  constructor(
    readonly graph: WorldGraphLoadResult,
    cause: unknown,
    readonly pendingCleanup: ReadonlyMap<
      number,
      WorldReference
    > = graph.created,
  ) {
    super(
      "World graph load did not complete; exact cleanup identities retained",
      { cause },
    );
    this.name = "WorldGraphLoadError";
  }
}

/** `ipp_protocol::MAX_FIELD_BYTES`, checked by `tools/check_repo.py`. */
const CHUNK_BYTES = 65_536;
const TRANSFER_WINDOW = 8;

async function transferChunks(
  start: number,
  total: number,
  signal: AbortSignal | undefined,
  transfer: (offset: number) => Promise<void>,
): Promise<void> {
  for (
    let offset = start;
    offset < total;
    offset += CHUNK_BYTES * TRANSFER_WINDOW
  ) {
    signal?.throwIfAborted();
    const pending: Promise<void>[] = [];
    for (let index = 0; index < TRANSFER_WINDOW; index++) {
      const next = offset + index * CHUNK_BYTES;
      if (next >= total) break;
      pending.push(transfer(next));
    }
    // A failed or cancelled transfer drains every submitted request before the
    // caller cancels Host staging in its finally block.
    const results = await Promise.allSettled(pending);
    const failure = results.find(
      (result): result is PromiseRejectedResult => result.status === "rejected",
    );
    if (failure) throw failure.reason;
  }
}

/** Optional persistence surface; generated Hosts inherit it only with snapshot enabled. */
export abstract class WorldPersistenceHostClient<
  T extends Client,
> extends HostClientBase<T> {
  private transferring = false;
  protected abstract readonly graphMetadataPageSize: number;
  protected abstract readonly graphBindingPageSize: number;

  /** Capture underlying component/controller state with unchanged resource references. */
  async saveWorld(
    session: bigint,
    options: WorldTransferOptions = {},
  ): Promise<Uint8Array<ArrayBuffer>> {
    const maxBytes = this.beginTransfer(options);
    try {
      const accepted = await this.request(
        this.hostTag("HOST_REQUEST_SAVE_WORLD"),
        (writer) => writer.u64(session),
      );
      this.expect(accepted, this.hostTag("HOST_RESPONSE_READ"));
      const reference = readBulkReference(accepted);
      const length = accepted.boolean() ? accepted.u64() : undefined;
      accepted.end();
      return await this.reads.readAll(
        { reference, length },
        { maxBytes, signal: options.signal },
      );
    } finally {
      this.transferring = false;
    }
  }

  /** Inspect an owned upload without publishing Worlds or reserving names. */
  async inspectWorldGraph(
    input: Uint8Array,
    options: WorldTransferOptions = {},
  ): Promise<WorldGraphDescriptor> {
    const maxBytes = this.beginTransfer(options);
    let job: bigint | undefined;
    try {
      const bytes = input.slice();
      job = await this.beginLoad(bytes, maxBytes);
      await this.uploadLoad(job, bytes, options.signal);
      return await this.readGraph(job, options.signal);
    } finally {
      if (job !== undefined) await this.cancelTransfer(job);
      this.transferring = false;
    }
  }

  /** Publish the graph without opening sessions; return all independent World lifetimes. */
  async loadWorld(
    input: Uint8Array,
    options: WorldLoadOptions = {},
  ): Promise<WorldGraphLoadResult> {
    const maxBytes = this.beginTransfer(options);
    let job: bigint | undefined;
    let loaded: WorldGraphLoadResult | undefined;
    let acknowledgementSubmitted = false;
    let pendingCleanup: ReadonlyMap<number, WorldReference> | undefined;
    try {
      if (input.length < 32 || input.length > maxBytes)
        throw new RangeError("World file is outside the byte budget");
      // Caller edits cannot alter a transfer in flight.
      const bytes = input.slice();
      job = await this.beginLoad(bytes, maxBytes);
      await this.uploadLoad(job, bytes, options.signal);
      const graph = await this.readGraph(job, options.signal);
      const names =
        typeof options.worldNames === "function"
          ? await options.worldNames(graph)
          : options.worldNames;
      const replacements = [...(names ?? [])];
      const ids = new Set(graph.nodes.map((node) => node.id));
      for (const [node] of replacements)
        if (!ids.has(node)) throw new Error("Unknown graph node rename");
      for (
        let offset = 0;
        offset < replacements.length;
        offset += this.graphMetadataPageSize
      ) {
        options.signal?.throwIfAborted();
        const page = replacements.slice(
          offset,
          offset + this.graphMetadataPageSize,
        );
        await this.complete(
          this.hostTag("HOST_REQUEST_SET_WORLD_LOAD_NAMES"),
          (writer) => {
            writer.u64(job!);
            writer.u32(page.length);
            for (const [node, name] of page) {
              writer.u32(node);
              writer.string(name);
            }
          },
        );
      }
      options.signal?.throwIfAborted();
      const result = await this.request(
        this.hostTag("HOST_REQUEST_FINISH_WORLD_LOAD"),
        (writer) => {
          writer.u64(job!);
          writer.u8(options.symbolicId === undefined ? 0 : 1);
          if (options.symbolicId !== undefined)
            writer.string(options.symbolicId);
          writer.hints(options.capacityHints);
        },
      );
      this.expect(result, this.hostTag("HOST_RESPONSE_WORLD_GRAPH_LOADED"));
      if (result.u64() !== job)
        throw new Error("World graph transfer mismatch");
      const root = readWorldReference(result);
      const total = result.u32();
      result.end();
      if (total !== graph.nodes.length)
        throw new Error("World graph created count mismatch");
      const created = new Map<number, WorldReference>();
      loaded = { root, created };
      while (created.size < total) {
        options.signal?.throwIfAborted();
        const page = await this.request(
          this.hostTag("HOST_REQUEST_READ_WORLD_LOAD_BINDINGS"),
          (writer) => {
            writer.u64(job!);
            writer.u32(created.size);
          },
        );
        this.expect(page, this.hostTag("HOST_RESPONSE_WORLD_GRAPH_BINDINGS"));
        if (page.u64() !== job || page.u32() !== created.size)
          throw new Error("World graph binding offset mismatch");
        const count = page.count(this.graphBindingPageSize);
        if (!count || count > total - created.size)
          throw new Error("World graph binding count mismatch");
        for (let index = 0; index < count; index++) {
          const node = page.u32();
          const world = readWorldReference(page);
          if (!ids.has(node) || created.has(node))
            throw new Error("Invalid graph binding identity");
          created.set(node, world);
        }
        page.end();
      }
      const mappedRoot = created.get(graph.root);
      if (
        mappedRoot?.id !== root.id ||
        mappedRoot.incarnation !== root.incarnation
      )
        throw new Error("World graph root binding mismatch");
      options.signal?.throwIfAborted();
      acknowledgementSubmitted = true;
      await this.complete(
        this.hostTag("HOST_REQUEST_ACKNOWLEDGE_WORLD_LOAD"),
        (writer) => writer.u64(job!),
      );
      job = undefined;
      if (options.signal?.aborted) {
        const remaining = new Map(created);
        pendingCleanup = remaining;
        const entries = [...created];
        for (
          let offset = 0;
          offset < entries.length;
          offset += TRANSFER_WINDOW
        ) {
          const page = entries.slice(offset, offset + TRANSFER_WINDOW);
          const settled = await Promise.allSettled(
            page.map(([, world]) => this.destroyWorld(world)),
          );
          for (let index = 0; index < settled.length; index++)
            if (settled[index]!.status === "fulfilled")
              remaining.delete(page[index]![0]);
        }
        options.signal.throwIfAborted();
      }
      return loaded;
    } catch (error) {
      if (acknowledgementSubmitted && loaded)
        throw new WorldGraphLoadError(loaded, error, pendingCleanup);
      throw error;
    } finally {
      if (job !== undefined) await this.cancelTransfer(job);
      this.transferring = false;
    }
  }

  private async beginLoad(
    bytes: Uint8Array,
    maxBytes: number,
  ): Promise<bigint> {
    if (bytes.length < 32 || bytes.length > maxBytes)
      throw new RangeError("World file is outside the byte budget");
    const accepted = await this.request(
      this.hostTag("HOST_REQUEST_BEGIN_WORLD_LOAD"),
      (writer) => writer.u64(BigInt(bytes.length)),
    );
    this.expect(accepted, this.hostTag("HOST_RESPONSE_TRANSFER"));
    const job = accepted.u64();
    accepted.end();
    return job;
  }

  private uploadLoad(
    job: bigint,
    bytes: Uint8Array,
    signal?: AbortSignal,
  ): Promise<void> {
    return transferChunks(0, bytes.length, signal, (offset) =>
      this.complete(this.hostTag("HOST_REQUEST_WRITE_WORLD_LOAD"), (writer) => {
        writer.u64(job);
        writer.u64(BigInt(offset));
        writer.bytes(bytes.subarray(offset, offset + CHUNK_BYTES));
      }),
    );
  }

  private async readGraph(
    job: bigint,
    signal?: AbortSignal,
  ): Promise<WorldGraphDescriptor> {
    const nodes: WorldGraphDescriptor["nodes"][number][] = [];
    const ids = new Set<number>();
    let root: number | undefined;
    let total: number | undefined;
    do {
      signal?.throwIfAborted();
      const page = await this.request(
        this.hostTag("HOST_REQUEST_INSPECT_WORLD_LOAD"),
        (writer) => {
          writer.u64(job);
          writer.u32(nodes.length);
        },
      );
      this.expect(page, this.hostTag("HOST_RESPONSE_WORLD_GRAPH_PAGE"));
      if (page.u64() !== job) throw new Error("World graph transfer mismatch");
      const pageRoot = page.u32();
      const pageTotal = page.u32();
      root ??= pageRoot;
      total ??= pageTotal;
      if (
        !total ||
        root !== pageRoot ||
        total !== pageTotal ||
        page.u32() !== nodes.length
      )
        throw new Error("World graph preview mismatch");
      const count = page.count(this.graphMetadataPageSize);
      if (!count || count > total - nodes.length)
        throw new Error("World graph preview count mismatch");
      for (let index = 0; index < count; index++) {
        const id = page.u32();
        const symbolicId = page.string();
        const persistentId = page.u64() | (page.u64() << 64n);
        if (ids.has(id)) throw new Error("Duplicate graph node identity");
        ids.add(id);
        nodes.push({ id, symbolicId, persistentId });
      }
      page.end();
    } while (nodes.length < total);
    signal?.throwIfAborted();
    if (!ids.has(root)) throw new Error("Missing graph root");
    return { root, nodes };
  }

  private async cancelTransfer(job: bigint): Promise<void> {
    await this.complete(
      this.hostTag("HOST_REQUEST_CANCEL_WORLD_TRANSFER"),
      (writer) => writer.u64(job),
    ).catch(() => {});
  }

  private beginTransfer(options: WorldTransferOptions): number {
    options.signal?.throwIfAborted();
    // The core's default World persistence budget, checked by `tools/check_repo.py`.
    const max = options.maxBytes ?? 64 * 1024 * 1024;
    if (!Number.isSafeInteger(max) || max < 32 || max > 64 * 1024 * 1024)
      throw new RangeError("maxBytes must be in 32..=67108864");
    if (this.transferring)
      throw new Error("A World transfer is already active");
    this.transferring = true;
    return max;
  }
}
