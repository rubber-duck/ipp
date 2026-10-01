import { isAssetSourceResponse } from "./asset-sources.js";
import { acceptHostAnnouncement, hostHello } from "./host-contract.js";
import { HostPhysicalInput } from "./host-input.js";
import type {
  CameraOutputReference,
  OutputReference,
  WorldReference,
} from "./types.js";
import {
  HostPresentation,
  readRootBinding,
  writeRootBinding,
  type RootBinding,
} from "./host-presentation.js";
import {
  readWorldReference,
  writeWorldReference,
  readOutputReference,
  writeOutputReference,
} from "./references.js";
import {
  RequestNotSentError,
  validateOptions,
  type Client,
  type ConnectOptions,
} from "./client.js";
import type { MessageTransport, TransportEvents } from "./transport.js";
import { bindPresentation, presentationOf } from "./presentation.js";
import { BatchIdentities } from "./command-pages.js";
import {
  HostWireReader,
  HostWireWriter,
  type WorldCapacityHintsPatch,
  type WorldCreateOptions,
  type WorldDescriptor,
  type WorldManifest,
  type WorldSelector,
  type CreatedWorld,
  WorldSelectionRequiredError,
} from "./host-protocol.js";

export type {
  WorldCapacityHints,
  WorldCapacityHintsPatch,
  WorldCreateOptions,
  WorldDescriptor,
  WorldManifest,
  WorldSelector,
  CreatedWorld,
} from "./host-protocol.js";
export { WorldSelectionRequiredError } from "./host-protocol.js";

interface HostRequestWaiter {
  resolve(reader: HostWireReader): void;
  reject(error: Error): void;
  /** The reply deadline, from when the request goes on the wire. */
  timer: ReturnType<typeof setTimeout> | undefined;
  accept?: (reader: HostWireReader) => void;
}

interface WorldAttachment<T extends Client> {
  reference: WorldReference;
  session: bigint;
  world: WorldDescriptor;
  manifest: WorldManifest;
  client?: T;
  events?: TransportEvents;
  queued: Uint8Array[];
  closeHost: boolean;
  closing?: Promise<void>;
}

/** One physical connection with independently fenced World authoring sessions. */
export abstract class HostClientBase<T extends Client> {
  readonly input = new HostPhysicalInput(
    (tag, encode, accept) => this.request(tag, encode, accept),
    (name) => this.hostTag(name),
    (name) => this.hostLimit(name),
  );
  readonly presentation = new HostPresentation(
    (tag, encode) => this.request(tag, encode),
    (name) => this.hostTag(name),
  );
  private connection = 0n;
  private nextRequest = 1n;
  private readonly pending = new Map<bigint, HostRequestWaiter>();
  private readonly attachments = new Map<bigint, WorldAttachment<T>>();
  /** Batch identities are unique per connection, across its World sessions. */
  private readonly batchIdentities = new BatchIdentities();
  private stopped = false;
  private closing?: Promise<void>;
  protected readonly timeoutMs: number;

  protected constructor(
    private readonly transport: MessageTransport,
    protected readonly options: ConnectOptions = {},
  ) {
    this.timeoutMs = options.timeoutMs ?? 10_000;
    // Statistics and testing controls reach the presentation through
    // `@ipp/client/diagnostics` and `/testing`, not a member of this type.
    bindPresentation(this, presentationOf(transport));
  }

  protected abstract hostTag(name: string): number;
  /** A named Host protocol bound of the connected target contract. */
  protected abstract hostLimit(name: string): number;
  protected abstract hostMagic(response: boolean): Uint8Array<ArrayBuffer>;
  /** Compatibility hash of the contract this client was generated from. */
  abstract readonly schemaHash: bigint;
  /** Wire revision of the contract this client was generated from. */
  protected abstract readonly protocolRevision: number;
  protected abstract createWorldClient(
    transport: MessageTransport,
    session: bigint,
    world: WorldDescriptor,
    manifest: WorldManifest,
    reference: WorldReference,
  ): T;

  get sessions(): ReadonlyMap<bigint, T> {
    return new Map(
      [...this.attachments].flatMap(([id, attachment]) =>
        attachment.client ? [[id, attachment.client] as const] : [],
      ),
    );
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
          void this.close().catch(() => {});
        };
        const abort = () => fail(new Error("Host connection aborted"));
        const timer = setTimeout(
          () => fail(new Error("Host hello timed out")),
          this.timeoutMs,
        );
        this.options.signal?.addEventListener("abort", abort, { once: true });
        this.transport.start({
          ready: () => {
            try {
              this.transport.send(hostHello());
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
                // The Host checks no claim: this client refuses a Host whose
                // contract differs from its own and sends nothing further.
                const { connection, trailer } = acceptHostAnnouncement(bytes, {
                  revision: this.protocolRevision,
                  schemaHash: this.schemaHash,
                });
                if (trailer.length !== 0)
                  throw new Error("Unexpected Host announcement trailer");
                this.connection = connection;
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
    accept?: (reader: HostWireReader) => void,
  ): Promise<HostWireReader> {
    if (this.stopped || this.connection === 0n)
      return Promise.reject(new Error("Host connection is closed"));
    // The transport holds the request until the connection's flow control
    // lets it leave; a busy Host delays Host requests instead of refusing them.
    const id = this.nextRequest++;
    const writer = new HostWireWriter();
    writer.raw(this.hostMagic(false));
    writer.u64(this.connection);
    writer.u64(id);
    writer.u8(tag);
    encode?.(writer);
    const bytes = writer.finish();
    return new Promise((resolve, reject) => {
      const waiter: HostRequestWaiter = {
        resolve,
        reject,
        timer: undefined,
        ...(accept ? { accept } : {}),
      };
      this.pending.set(id, waiter);
      // Waiting for connection credit is not a Host delay: the reply deadline
      // starts when the request goes on the wire.
      const deadline = () => {
        if (this.pending.get(id) !== waiter) return;
        waiter.timer = setTimeout(() => {
          const error = new Error(
            "Host request timed out; its outcome is unknown",
          );
          this.stop(error);
          void this.close().catch(() => {});
        }, this.timeoutMs);
      };
      try {
        const leaving = this.transport.send(bytes);
        if (leaving) void leaving.then(deadline);
        else deadline();
      } catch (error) {
        this.stop(
          new Error("Host request send failed; its outcome is unknown", {
            cause: error,
          }),
        );
        void this.close().catch(() => {});
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

  async createWorld(options: WorldCreateOptions): Promise<CreatedWorld> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_CREATE_WORLD"),
      (writer) => {
        const selected = options?.selectedSystems;
        if (selected === undefined) throw new WorldSelectionRequiredError();
        writer.string(options.symbolicId ?? "");
        writer.hints(options.capacityHints);
        writer.u8(1);
        if (
          selected.length > 1024 ||
          new Set(selected).size !== selected.length
        )
          throw new RangeError("Invalid selected systems");
        writer.u32(selected.length);
        for (const system of selected) writer.string(system);
        const canvas = options.canvas;
        writer.u8(canvas === undefined ? 0 : 1);
        if (canvas !== undefined) {
          if (!Array.isArray(canvas.extent) || canvas.extent.length !== 2)
            throw new RangeError("Canvas extent requires width and height");
          writer.f32(canvas.extent[0]);
          writer.f32(canvas.extent[1]);
          writer.f32(canvas.unitsPerMetre);
        }
        writer.u8(options.temporary ? 1 : 0);
      },
    );
    return this.acceptCreated(reader);
  }

  protected acceptCreated(reader: HostWireReader): CreatedWorld {
    this.expect(reader, this.hostTag("HOST_RESPONSE_CREATED"));
    const world = reader.world();
    const reference = readWorldReference(reader);
    reader.end();
    if (reference.id !== world.id)
      throw new Error("Invalid created World reference");
    return { ...world, reference };
  }

  async openWorld(world: WorldReference): Promise<T> {
    return this.acceptAttachment(
      await this.request(this.hostTag("HOST_REQUEST_OPEN_WORLD"), (writer) =>
        writeWorldReference(writer, world),
      ),
    );
  }

  async resolveWorld(world: WorldSelector): Promise<WorldReference> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_RESOLVE_WORLD"),
      (writer) =>
        writer.selector(
          world,
          this.hostTag("WORLD_SELECTOR_ID"),
          this.hostTag("WORLD_SELECTOR_SYMBOL"),
        ),
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_WORLD_REFERENCE"));
    const reference = readWorldReference(reader);
    reader.end();
    return reference;
  }

  /** Bind the current Camera output of `entity`. A World's canvas names no
   * entity and needs no binding: select it with `canvasOutput(world)`. */
  async bindOutput(
    world: WorldReference,
    entity: bigint,
    kind: "camera",
  ): Promise<CameraOutputReference> {
    if (kind !== "camera")
      throw new Error(
        "Only Camera outputs bind; canvasOutput(world) names a World's canvas",
      );
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_BIND_OUTPUT"),
      (writer) => {
        writeWorldReference(writer, world);
        writer.u64(entity);
        writer.u8(1);
      },
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_OUTPUT_REFERENCE"));
    const output = readOutputReference(reader);
    reader.end();
    if (output.kind !== "camera")
      throw new Error("The Host bound a non-Camera output");
    return output;
  }

  async resolveOutput(output: OutputReference): Promise<OutputReference> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_RESOLVE_OUTPUT"),
      (writer) => writeOutputReference(writer, output),
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_OUTPUT_REFERENCE"));
    const reference = readOutputReference(reader);
    reader.end();
    return reference;
  }

  async setRootOutput(
    output: OutputReference,
    viewport: { width: number; height: number; devicePixelRatio: number },
  ): Promise<RootBinding> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_SET_ROOT_OUTPUT"),
      (writer) => {
        writeOutputReference(writer, output);
        writer.u32(viewport.width);
        writer.u32(viewport.height);
        writer.f64(viewport.devicePixelRatio);
      },
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_ROOT_BINDING"));
    if (reader.u8() !== 1)
      throw new Error("Root binding acknowledgement is missing");
    const binding = readRootBinding(reader);
    reader.end();
    return binding;
  }

  async getRootOutputBinding(
    world: WorldReference,
  ): Promise<RootBinding | null> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_GET_ROOT_OUTPUT_BINDING"),
      (writer) => writeWorldReference(writer, world),
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_ROOT_BINDING"));
    const present = reader.u8();
    if (present > 1) throw new Error("Invalid root binding option");
    const binding = present === 1 ? readRootBinding(reader) : null;
    reader.end();
    return binding;
  }

  clearRootOutput(expected: RootBinding): Promise<void> {
    return this.complete(
      this.hostTag("HOST_REQUEST_CLEAR_ROOT_OUTPUT"),
      (writer) => writeRootBinding(writer, expected),
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

  destroyWorld(world: WorldReference): Promise<void> {
    return this.complete(this.hostTag("HOST_REQUEST_DESTROY_WORLD"), (writer) =>
      writeWorldReference(writer, world),
    );
  }

  detachWorld(session: bigint): Promise<void> {
    const attachment = this.attachments.get(session);
    if (!attachment)
      return Promise.reject(new Error("World session has ended"));
    attachment.closing ??= this.complete(
      this.hostTag("HOST_REQUEST_DETACH_WORLD"),
      (writer) => writer.u64(session),
    )
      .then(() =>
        this.invalidateAttachment(session, new Error("World detached")),
      )
      .catch((error: unknown) => {
        if (error instanceof RequestNotSentError) delete attachment.closing;
        throw error;
      });
    return attachment.closing;
  }

  async setCapacityHints(
    session: bigint,
    hints: WorldCapacityHintsPatch,
  ): Promise<WorldDescriptor> {
    const reader = await this.request(
      this.hostTag("HOST_REQUEST_SET_CAPACITY_HINTS"),
      (writer) => {
        writer.u64(session);
        writer.hints(hints);
      },
    );
    this.expect(reader, this.hostTag("HOST_RESPONSE_WORLD"));
    const descriptor = reader.world();
    reader.end();
    this.updateDescriptor(descriptor);
    return descriptor;
  }

  /** Used by IppClient.connect… convenience: closing that World also closes this Host. */
  ownWorldConnection(session: bigint): void {
    const attachment = this.attachments.get(session);
    if (!attachment) throw new Error("No World is attached");
    attachment.closeHost = true;
  }

  protected acceptAttachment(reader: HostWireReader): T {
    this.expect(reader, this.hostTag("HOST_RESPONSE_ATTACHED"));
    reader.world();
    const session = reader.u64();
    reader.manifest();
    readWorldReference(reader);
    reader.end();
    // The receive path creates the attachment before subsequent frames arrive.
    const attachment = this.attachments.get(session);
    if (!attachment || attachment.session !== session)
      throw new Error("Missing World attachment");
    const transport: MessageTransport = {
      batchIdentities: this.batchIdentities,
      start: (events) => {
        attachment.events = events;
        events.ready();
        for (const bytes of attachment.queued.splice(0)) events.message(bytes);
      },
      send: (bytes) => {
        this.requireAttachment(attachment);
        return this.transport.send(bytes);
      },
      ...(this.transport.sendParts
        ? {
            sendParts: (parts: Uint8Array<ArrayBuffer>[]) => {
              this.requireAttachment(attachment);
              return this.transport.sendParts!(parts);
            },
          }
        : {}),
      close: async () => {
        if (attachment.closeHost) return this.close();
        if (this.attachments.get(session) === attachment && !this.stopped)
          await this.detachWorld(session);
      },
    };
    attachment.client = this.createWorldClient(
      transport,
      session,
      attachment.world,
      attachment.manifest,
      attachment.reference,
    );
    return attachment.client;
  }

  private requireAttachment(attachment: WorldAttachment<T>): void {
    if (this.stopped || this.attachments.get(attachment.session) !== attachment)
      throw new Error("World session has ended");
  }

  private updateDescriptor(world: WorldDescriptor): void {
    for (const attachment of this.attachments.values())
      if (attachment.world.id === world.id)
        Object.assign(attachment.world, world);
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
        if (tag !== this.hostTag("HOST_RESPONSE_DETACHED")) {
          this.input.notification(new HostWireReader(bytes.subarray(24)));
          return;
        }
        const session = tagReader.u64();
        const reason = tagReader.string();
        tagReader.end();
        this.invalidateAttachment(session, new Error(reason));
        return;
      }
      const pending = this.pending.get(id);
      if (!pending) throw new Error("Unknown Host request correlation");
      if (tag === this.hostTag("HOST_RESPONSE_ERROR")) {
        const reason = tagReader.string();
        tagReader.end();
        this.pending.delete(id);
        clearTimeout(pending.timer);
        pending.reject(new Error(reason));
        return;
      }
      if (tag === this.hostTag("HOST_RESPONSE_ATTACHED")) {
        const world = tagReader.world();
        const session = tagReader.u64();
        const manifest = tagReader.manifest();
        const reference = readWorldReference(tagReader);
        tagReader.end();
        if (
          this.attachments.has(session) ||
          session === 0n ||
          reference.id !== world.id
        )
          throw new Error("Invalid World attachment transition");
        this.attachments.set(session, {
          reference,
          world,
          session,
          manifest,
          queued: [],
          closeHost: false,
        });
      }
      pending.accept?.(new HostWireReader(bytes.subarray(24)));
      this.pending.delete(id);
      clearTimeout(pending.timer);
      pending.resolve(reader);
    } else {
      const sessionOffset = isAssetSourceResponse(bytes) ? 4 : 0;
      if (bytes.length < sessionOffset + 8) return;
      const session = new DataView(
        bytes.buffer,
        bytes.byteOffset + sessionOffset,
        8,
      ).getBigUint64(0, true);
      const attachment = this.attachments.get(session);
      if (!attachment) return;
      if (attachment.events) attachment.events.message(bytes);
      else {
        if (attachment.queued.length >= 128)
          throw new Error("World attachment queue exhausted");
        attachment.queued.push(bytes.slice());
      }
    }
  }

  private invalidateAttachment(session: bigint, error: Error): void {
    const attachment = this.attachments.get(session);
    this.attachments.delete(session);
    attachment?.events?.error(error);
  }

  private stop(error: Error): void {
    if (this.stopped) return;
    this.stopped = true;
    this.input.stop(error);
    for (const session of [...this.attachments.keys()])
      this.invalidateAttachment(session, error);
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
