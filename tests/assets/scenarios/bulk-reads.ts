/** Common read-lease assertions on a real transport, independent of generated codecs. */
import {
  hostHello,
  hostContractRequest,
  readHostAnnouncement,
  readHostContractDescriptor,
} from "../../../packages/ipp-client/src/host-contract.js";
import {
  HostWireReader,
  HostWireWriter,
} from "../../../packages/ipp-client/src/host-protocol.js";
import type { BulkReadDescriptor } from "../../../packages/ipp-client/src/bulk-reads.js";
import type { MessageTransport } from "../../../packages/ipp-client/src/transport.js";

export class BulkReadParticipant {
  connection = 0n;
  private id = 0n;
  private readonly queued: Uint8Array[] = [];
  private waiting:
    | {
        resolve(bytes: Uint8Array): void;
        reject(error: Error): void;
        timer: ReturnType<typeof setTimeout>;
      }
    | undefined;
  private failure?: Error;

  constructor(
    private readonly transport: MessageTransport,
    private readonly timeout: number,
  ) {}

  async open(): Promise<void> {
    this.transport.start({
      ready: () => this.transport.send(hostHello()),
      message: (bytes) => {
        const waiting = this.waiting;
        this.waiting = undefined;
        if (waiting) {
          clearTimeout(waiting.timer);
          waiting.resolve(bytes.slice());
        } else this.queued.push(bytes.slice());
      },
      error: (error) => this.fail(error),
      closed: () => this.fail(new Error("Bulk participant closed")),
    });
    this.connection = readHostAnnouncement(
      await this.receive(),
    ).announcement.connection;
  }

  private fail(error: Error): void {
    this.failure = error;
    if (this.waiting) {
      clearTimeout(this.waiting.timer);
      this.waiting.reject(error);
      this.waiting = undefined;
    }
  }

  receive(): Promise<Uint8Array> {
    const bytes = this.queued.shift();
    if (bytes) return Promise.resolve(bytes);
    if (this.failure) return Promise.reject(this.failure);
    if (this.waiting) throw new Error("Concurrent bulk receive");
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => this.fail(new Error("Bulk reply timed out")),
        this.timeout,
      );
      this.waiting = { resolve, reject, timer };
    });
  }

  async fixture(operation: number): Promise<Uint8Array> {
    const writer = new HostWireWriter();
    writer.raw(Uint8Array.of(73, 80, 68, 84));
    writer.u64(this.connection);
    writer.u64(++this.id);
    writer.u8(operation);
    this.transport.send(writer.finish());
    const response = await this.receive();
    if (new TextDecoder().decode(response.subarray(0, 4)) !== "IPDU")
      throw new Error("Unexpected instrumentation reply");
    return response;
  }

  async descriptor(): Promise<BulkReadDescriptor> {
    this.transport.send(hostContractRequest());
    return readHostContractDescriptor(await this.receive());
  }

  send(read: bigint, operation: number, position = 0n, eof = false): bigint {
    const writer = new HostWireWriter();
    writer.raw(Uint8Array.of(73, 80, 68, 82));
    writer.u64(this.connection);
    writer.u64(++this.id);
    writer.u64(read);
    writer.u8(operation);
    if (operation !== 2) writer.u64(position);
    if (operation === 1) writer.u8(eof ? 1 : 0);
    this.transport.send(writer.finish());
    return this.id;
  }

  async status(
    read: bigint,
    operation: number,
    position = 0n,
    eof = false,
  ): Promise<number> {
    const id = this.send(read, operation, position, eof);
    const response = new HostWireReader(await this.receive());
    this.header(response, id, read);
    const status = response.u8();
    if (status === 2) response.string();
    response.end();
    return status;
  }

  private header(reader: HostWireReader, id: bigint, read: bigint): void {
    if (
      new TextDecoder().decode(reader.raw(4)) !== "IPDS" ||
      reader.u64() !== this.connection ||
      reader.u64() !== id ||
      reader.u64() !== read
    )
      throw new Error("Bulk reply is not fenced to its request");
  }

  async readWithoutEofAcknowledgement(
    descriptor: BulkReadDescriptor,
  ): Promise<Uint8Array> {
    const length = descriptor.length;
    if (length === undefined || length > 64n * 1024n * 1024n)
      throw new Error("Invalid contract length");
    const output = new Uint8Array(Number(length));
    let offset = 0n;
    let eof = false;
    while (!eof) {
      const count = Math.max(
        1,
        Math.min(8, Number((length - offset + 65535n) / 65536n)),
      );
      const requests = Array.from({ length: count }, (_, index) => ({
        offset: offset + BigInt(index * 65536),
        id: this.send(
          descriptor.reference.read,
          0,
          offset + BigInt(index * 65536),
        ),
      }));
      for (const request of requests) {
        const response = new HostWireReader(await this.receive());
        this.header(response, request.id, descriptor.reference.read);
        if (response.u8() !== 0 || response.u64() !== request.offset)
          throw new Error("Invalid bulk chunk");
        eof = response.boolean();
        const bytes = response.bytes();
        response.end();
        output.set(bytes, Number(request.offset));
        offset = request.offset + BigInt(bytes.length);
      }
      if (
        !eof &&
        (await this.status(descriptor.reference.read, 1, offset)) !== 1
      )
        throw new Error("Prefix ack failed");
    }
    if (offset !== length) throw new Error("Bulk contract changed length");
    return output;
  }

  close(): Promise<void> {
    return this.transport.close();
  }
}

/** Two recipients, withheld/invalid EOF acknowledgement, release, disconnect and replacement. */
export async function bulkReadLeases(
  connect: () => MessageTransport,
  expected: ArrayLike<number>,
  timeout: number,
) {
  const first = new BulkReadParticipant(connect(), timeout);
  let peer: BulkReadParticipant | undefined;
  let replacement: BulkReadParticipant | undefined;
  try {
    await first.open();
    peer = new BulkReadParticipant(connect(), timeout);
    await peer.open();
    const a = await first.descriptor();
    const b = await peer.descriptor();
    const bytes = await first.readWithoutEofAcknowledgement(a);
    if (
      bytes.length !== expected.length ||
      !bytes.every((byte, at) => byte === expected[at])
    )
      throw new Error("Bulk bytes differ from target contract");
    // EOF was delivered, but prefix without EOF still leaves the read lease live.
    if (
      (await first.status(a.reference.read, 1, a.length, false)) !== 1 ||
      (await first.status(a.reference.read, 1, 0n, true)) !== 2
    )
      throw new Error("Invalid/withheld EOF ack consumed a lease");
    if ((await peer.status(a.reference.read, 0)) !== 2)
      throw new Error("Peer consumed foreign read authority");
    if (
      (await first.status(a.reference.read, 2)) !== 1 ||
      (await first.status(a.reference.read, 0)) !== 2
    )
      throw new Error("Release did not fence stale reads");
    const peerBytes = await peer.readWithoutEofAcknowledgement(b);
    if (!peerBytes.every((byte, at) => byte === expected[at]))
      throw new Error("Peer lease changed after producer release");
    await first.close();
    replacement = new BulkReadParticipant(connect(), timeout);
    await replacement.open();
    if (
      replacement.connection === first.connection ||
      (await replacement.status(a.reference.read, 0)) !== 2
    )
      throw new Error("Replacement connection inherited stale authority");
    if ((await peer.status(b.reference.read, 1, b.length, true)) !== 1)
      throw new Error("Peer EOF ack failed");
    return {
      bytes: bytes.length,
      peerIsolated: true,
      withheldEofRetained: true,
      staleFenced: true,
    };
  } finally {
    await Promise.allSettled([
      first.close(),
      peer?.close(),
      replacement?.close(),
    ]);
  }
}
