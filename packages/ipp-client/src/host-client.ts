import { isAssetSourceResponse } from "./asset-sources.js";
import { validateOptions, type Client, type ConnectOptions } from "./client.js";
import type { MessageTransport, TransportEvents } from "./transport.js";
import type { GlyphAtlasLimits, Presentation } from "./presentation.js";
import {
  HostWireReader,
  HostWireWriter,
  type WorldCapacityHintsPatch,
  type WorldCreateOptions,
  type WorldDescriptor,
  type WorldSelector,
} from "./host-protocol.js";

export type {
  WorldCapacityHints,
  WorldCapacityHintsPatch,
  WorldCreateOptions,
  WorldDescriptor,
  WorldSelector,
} from "./host-protocol.js";

interface HostRequestWaiter {
  resolve(reader: HostWireReader): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

interface WorldAttachment<T extends Client> {
  session: bigint;
  world: WorldDescriptor;
  client?: T;
  events?: TransportEvents;
  queued: Uint8Array[];
  closeHost: boolean;
}

/** One physical connection, with at most one independently fenced World attachment. */
export abstract class HostClientBase<T extends Client> {
  private connection = 0n;
  private nextRequest = 1n;
  private readonly pending = new Map<bigint, HostRequestWaiter>();
  private attachment: WorldAttachment<T> | undefined;
  private stopped = false;
  private closing?: Promise<void>;
  protected readonly timeoutMs: number;

  protected constructor(
    private readonly transport: MessageTransport,
    protected readonly options: ConnectOptions = {},
  ) {
    this.timeoutMs = options.timeoutMs ?? 10_000;
  }

  protected abstract hostTag(name: string): number;
  protected abstract hostMagic(response: boolean): Uint8Array<ArrayBuffer>;
  protected abstract bootstrap(): Uint8Array<ArrayBuffer>;
  protected abstract acceptBootstrap(bytes: Uint8Array): bigint;
  protected abstract createWorldClient(
    transport: MessageTransport,
    session: bigint,
    world: WorldDescriptor,
  ): T;

  get world(): T | undefined {
    return this.attachment?.client;
  }

  protected async initialize(): Promise<this> {
    try {
      validateOptions(this.options);
      await new Promise<void>((resolve, reject) => {
        let settled = false;
        const finish = (error?: Error) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          this.options.signal?.removeEventListener("abort", abort);
          if (error) reject(error);
          else resolve();
        };
        const fail = (error: Error) => {
          finish(error);
          this.stop(error);
        };
        const abort = () => fail(new Error("Host connection aborted"));
        const timer = setTimeout(
          () => fail(new Error("Host bootstrap timed out")),
          this.timeoutMs,
        );
        this.options.signal?.addEventListener("abort", abort, { once: true });
        this.transport.start({
          ready: () => {
            try {
              this.transport.send(this.bootstrap());
            } catch (error) {
              fail(asError(error));
            }
          },
          error: fail,
          closed: () => fail(new Error("Host transport closed")),
          message: (bytes) => {
            if (this.stopped) return;
            try {
              if (this.connection === 0n) {
                this.connection = this.acceptBootstrap(bytes);
                finish();
              } else this.receive(bytes);
            } catch (error) {
              fail(asError(error));
            }
          },
        });
      });
      return this;
    } catch (error) {
      await this.close().catch(() => {});
      throw error;
    }
  }

  protected request(
    tag: number,
    encode?: (writer: HostWireWriter) => void,
  ): Promise<HostWireReader> {
    if (this.stopped || this.connection === 0n)
      return Promise.reject(new Error("Host connection is closed"));
    if (this.pending.size >= 64)
      return Promise.reject(new Error("Host request queue is full"));
    const id = this.nextRequest++;
    const writer = new HostWireWriter();
    writer.raw(this.hostMagic(false));
    writer.u64(this.connection);
    writer.u64(id);
    writer.u8(tag);
    encode?.(writer);
    const bytes = writer.finish();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const error = new Error(
          "Host request timed out; its outcome is unknown",
        );
        this.stop(error);
        void this.close().catch(() => {});
      }, this.timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      try {
        this.transport.send(bytes);
      } catch (error) {
        this.pending.delete(id);
        clearTimeout(timer);
        reject(asError(error));
      }
    });
  }

  protected async complete(
    tag: number,
    encode?: (writer: HostWireWriter) => void,
  ): Promise<void> {
    const reader = await this.request(tag, encode);
    this.expect(reader, this.hostTag("HOST_RESPONSE_COMPLETE"));
    reader.end();
  }

  protected expect(reader: HostWireReader, tag: number): void {
    if (reader.u8() !== tag) throw new Error("Unexpected Host result");
  }

  async listWorlds(): Promise<WorldDescriptor[]> {
    const worlds: WorldDescriptor[] = [];
    let after = 0n;
    do {
      const reader = await this.request(
        this.hostTag("HOST_REQUEST_LIST_WORLDS"),
        (writer) => writer.u64(after),
      );
      this.expect(reader, this.hostTag("HOST_RESPONSE_WORLDS"));
      const count = reader.count(32);
      for (let i = 0; i < count; i++) worlds.push(reader.world());
      const next = reader.u64();
      reader.end();
      if (next !== 0n && next <= after)
        throw new Error("World discovery cursor did not advance");
      after = next;
    } while (after !== 0n);
    return worlds;
  }

  async createWorld(options: WorldCreateOptions = {}): Promise<T> {
    return this.acceptAttachment(
      await this.request(
        this.hostTag("HOST_REQUEST_CREATE_WORLD"),
        (writer) => {
          writer.string(options.symbolicId ?? "");
          writer.hints(options.capacityHints);
          writer.u8(options.temporary ? 1 : 0);
        },
      ),
    );
  }

  async attachWorld(world: WorldSelector): Promise<T> {
    return this.acceptAttachment(
      await this.request(this.hostTag("HOST_REQUEST_ATTACH_WORLD"), (writer) =>
        writer.selector(
          world,
          this.hostTag("WORLD_SELECTOR_ID"),
          this.hostTag("WORLD_SELECTOR_SYMBOL"),
        ),
      ),
    );
  }

  async renameWorld(
    world: WorldSelector,
    symbolicId: string,
  ): Promise<WorldDescriptor> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_RENAME_WORLD"),
      (writer) => {
        writer.selector(
          world,
          this.hostTag("WORLD_SELECTOR_ID"),
          this.hostTag("WORLD_SELECTOR_SYMBOL"),
        );
        writer.string(symbolicId);
      },
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_WORLD"));
    const descriptor = reader.world();
    reader.end();
    this.updateDescriptor(descriptor);
    return descriptor;
  }

  destroyWorld(world: WorldSelector): Promise<void> {
    return this.complete(this.hostTag("HOST_REQUEST_DESTROY_WORLD"), (writer) =>
      writer.selector(
        world,
        this.hostTag("WORLD_SELECTOR_ID"),
        this.hostTag("WORLD_SELECTOR_SYMBOL"),
      ),
    );
  }

  async detachWorld(): Promise<void> {
    const current = this.attachment;
    await this.complete(this.hostTag("HOST_REQUEST_DETACH_WORLD"));
    if (this.attachment === current)
      this.invalidateAttachment(new Error("World detached"));
  }

  async setCapacityHints(
    hints: WorldCapacityHintsPatch,
  ): Promise<WorldDescriptor> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_SET_CAPACITY_HINTS"),
      (writer) => writer.hints(hints),
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_WORLD"));
    const descriptor = reader.world();
    reader.end();
    this.updateDescriptor(descriptor);
    return descriptor;
  }

  /** Used by IppClient.connect… convenience: closing that World also closes this Host. */
  ownWorldConnection(): void {
    if (!this.attachment) throw new Error("No World is attached");
    this.attachment.closeHost = true;
  }

  protected acceptAttachment(reader: HostWireReader): T {
    this.expect(reader, this.hostTag("HOST_RESPONSE_ATTACHED"));
    reader.world();
    const session = reader.u64();
    reader.end();
    // The receive path creates the attachment before subsequent frames arrive.
    const attachment = this.attachment;
    if (!attachment || attachment.session !== session)
      throw new Error("Missing World attachment");
    const transport: MessageTransport = {
      start: (events) => {
        attachment.events = events;
        events.ready();
        for (const bytes of attachment.queued.splice(0)) events.message(bytes);
      },
      send: (bytes) => {
        this.requireAttachment(attachment);
        this.transport.send(bytes);
      },
      ...(this.transport.sendParts
        ? {
            sendParts: (parts: Uint8Array<ArrayBuffer>[]) => {
              this.requireAttachment(attachment);
              this.transport.sendParts!(parts);
            },
          }
        : {}),
      close: async () => {
        if (attachment.closeHost) return this.close();
        if (this.attachment === attachment && !this.stopped)
          await this.detachWorld();
      },
    };
    const presentation = this.transport.presentation;
    if (presentation)
      Object.assign(transport, {
        presentation: {
          capture: async (
            _session: bigint,
            afterTick: bigint,
            timeoutMs: number,
          ) => {
            this.requireAttachment(attachment);
            const frame = await presentation.capture(
              this.connection,
              afterTick,
              timeoutMs,
            );
            this.requireAttachment(attachment);
            return { ...frame, session };
          },
          resize: (width: number, height: number) => {
            this.requireAttachment(attachment);
            presentation.resize(width, height);
          },
          loseContext: () => {
            this.requireAttachment(attachment);
            presentation.loseContext();
          },
          restoreContext: () => {
            this.requireAttachment(attachment);
            presentation.restoreContext();
          },
          setGlyphAtlasLimits: (limits: GlyphAtlasLimits) => {
            this.requireAttachment(attachment);
            presentation.setGlyphAtlasLimits(limits);
          },
          setSurfaceCacheBudget: (bytes: number) => {
            this.requireAttachment(attachment);
            presentation.setSurfaceCacheBudget(bytes);
          },
        } satisfies Presentation,
      });
    attachment.client = this.createWorldClient(
      transport,
      session,
      attachment.world,
    );
    return attachment.client;
  }

  private requireAttachment(attachment: WorldAttachment<T>): void {
    if (this.stopped || this.attachment !== attachment)
      throw new Error("World session has ended");
  }

  private updateDescriptor(world: WorldDescriptor): void {
    if (this.attachment?.world.id === world.id)
      Object.assign(this.attachment.world, world);
  }

  private receive(bytes: Uint8Array): void {
    if (this.hostMagic(true).every((value, index) => bytes[index] === value)) {
      const reader = new HostWireReader(bytes);
      reader.raw(8);
      if (reader.u64() !== this.connection)
        throw new Error("Host connection mismatch");
      const id = reader.u64();
      const tagReader = new HostWireReader(bytes.subarray(24));
      const tag = tagReader.u8();
      if (id === 0n) {
        if (tag !== this.hostTag("HOST_RESPONSE_DETACHED"))
          throw new Error("Unexpected Host notification");
        const session = tagReader.u64();
        const reason = tagReader.string();
        tagReader.end();
        if (this.attachment?.session === session)
          this.invalidateAttachment(new Error(reason));
        return;
      }
      const pending = this.pending.get(id);
      if (!pending) throw new Error("Unknown Host request correlation");
      this.pending.delete(id);
      clearTimeout(pending.timer);
      if (tag === this.hostTag("HOST_RESPONSE_ERROR")) {
        const reason = tagReader.string();
        tagReader.end();
        pending.reject(new Error(reason));
        return;
      }
      if (tag === this.hostTag("HOST_RESPONSE_ATTACHED")) {
        const world = tagReader.world();
        const session = tagReader.u64();
        tagReader.end();
        if (this.attachment || session === 0n)
          throw new Error("Invalid World attachment transition");
        this.attachment = { world, session, queued: [], closeHost: false };
      }
      pending.resolve(reader);
    } else {
      const attachment = this.attachment;
      const sessionOffset = isAssetSourceResponse(bytes) ? 4 : 0;
      if (!attachment || bytes.length < sessionOffset + 8) return;
      const session = new DataView(
        bytes.buffer,
        bytes.byteOffset + sessionOffset,
        8,
      ).getBigUint64(0, true);
      if (session !== attachment.session) return;
      if (attachment.events) attachment.events.message(bytes);
      else {
        if (attachment.queued.length >= 128)
          throw new Error("World attachment queue exhausted");
        attachment.queued.push(bytes.slice());
      }
    }
  }

  private invalidateAttachment(error: Error): void {
    const attachment = this.attachment;
    this.attachment = undefined;
    attachment?.events?.error(error);
  }

  private stop(error: Error): void {
    if (this.stopped) return;
    this.stopped = true;
    this.invalidateAttachment(error);
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
  }

  close(): Promise<void> {
    if (!this.closing) {
      this.stop(new Error("Host connection closed"));
      this.closing = this.transport.close();
    }
    return this.closing;
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
