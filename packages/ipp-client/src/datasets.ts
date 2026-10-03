import { HostWireReader, HostWireWriter } from "./host-protocol.js";

/** Exact core dataset kinds; matrices are column-major f32 lanes. */
export type DatasetValueKind =
  | "f32"
  | "i32"
  | "u32"
  | "bool"
  | "vec2"
  | "vec3"
  | "vec4"
  | "mat2"
  | "mat3"
  | "mat4"
  | "text";
export type DatasetValue =
  | { kind: "f32" | "i32" | "u32"; value: number }
  | { kind: "bool"; value: boolean }
  | { kind: "text"; value: string }
  | {
      kind: "vec2" | "vec3" | "vec4" | "mat2" | "mat3" | "mat4";
      value: readonly number[];
    };
export interface DatasetColumn {
  name: string;
  kind: DatasetValueKind;
  textMaxBytes?: bigint;
}
export interface DatasetProducer {
  readonly connection: bigint;
  readonly incarnation: bigint;
}
export type DatasetDelta =
  | { operation: "append"; rows: readonly (readonly DatasetValue[])[] }
  | {
      operation: "insert";
      index: bigint;
      rows: readonly (readonly DatasetValue[])[];
    }
  | { operation: "edit"; row: bigint; values: readonly DatasetValue[] }
  | { operation: "remove"; row: bigint };
export interface DatasetOutcome {
  committedDeltas: bigint;
  assignedRows: bigint;
  lastAssignedRow: bigint | null;
  failure?: { deltaIndex: bigint; reason: string };
}
export interface DatasetPage {
  name: string;
  incarnation: bigint;
  kind: "buffer" | "streaming";
  schema: DatasetColumn[];
  rows: { id: bigint; values: DatasetValue[] }[];
  nextOffset: bigint | null;
  memory: {
    retainedRows: bigint;
    retainedBytes: bigint;
    allocatedBytes: bigint;
    schemaBytes: bigint;
  };
}
/** Independent completed binding page; compare lifetime/tick fences across pages. */
export interface DataBindingPage {
  sourceIncarnation: bigint | null;
  bindingIncarnation: bigint;
  evaluatedTick: bigint | null;
  availability: {
    reason:
      | "Ready"
      | "NotEvaluated"
      | "Source"
      | "MissingAsset"
      | "MissingInput"
      | "InputType"
      | "InvalidInputName";
    detail: string;
    output: string;
    input: string;
  };
  dirty: boolean;
  totalRows: bigint;
  offset: bigint;
  nextOffset: bigint | null;
  columns: { name: string; kind: DatasetValueKind }[];
  rows: { id: bigint; values: ExpressionResult[] }[];
}
export type ExpressionResult =
  | { valid: true; value: DatasetValue }
  | {
      valid: false;
      reason: "MissingInput" | "InvalidInput" | "Calculation";
      slot: bigint | null;
    };
export interface ExpressionDriverStatus {
  availability: "Pending" | "Ready" | "Unavailable";
  state: "Prepared" | "Written" | "Retained";
  reason:
    | ""
    | "AssetUnavailable"
    | "AssetFailed"
    | "InputMapping"
    | "InputType"
    | "MissingInput"
    | "InvalidInput"
    | "Calculation"
    | "TargetUnavailable"
    | "TargetRejected"
    | "Cycle";
  slot: bigint | null;
  detail: string;
  recovered: boolean;
}
/** Constants read from the connected target's executed Rust contract. */
export interface DatasetContract {
  readonly requestMagic: readonly number[];
  readonly responseMagic: readonly number[];
  tag(name: string): number;
  limit(name: string): number;
}
export interface DatasetTransfer {
  readonly connection: bigint;
  readonly id: bigint;
  /** One final logical outcome; representation/cancellation failures reject. */
  readonly outcome: Promise<DatasetOutcome>;
}

type Pending = {
  resolve(reader: HostWireReader): void;
  reject(error: Error): void;
  timer?: ReturnType<typeof setTimeout>;
  final?: { resolve(reader: HostWireReader): void; reject(error: Error): void };
};

/** Host-wide sources on one connection, independent of World attachment lifetime. */
export class ClientDatasets {
  private nextId = 1n;
  private readonly pending = new Map<bigint, Pending>();
  private stopped?: Error;
  private operationTail: Promise<void> = Promise.resolve();
  private queuedBytes = 0;
  private queuedUpdates = 0;

  constructor(
    private readonly connection: () => bigint,
    private readonly contract: DatasetContract,
    private readonly send: (
      bytes: Uint8Array<ArrayBuffer>,
    ) => Promise<void> | void,
    private readonly timeoutMs: number,
    private readonly fail: (error: Error) => void,
  ) {}

  receive(bytes: Uint8Array): boolean {
    if (
      !this.contract.responseMagic.every(
        (value, index) => bytes[index] === value,
      )
    )
      return false;
    const reader = new HostWireReader(bytes);
    reader.raw(4);
    if (reader.u64() !== this.connection())
      throw new Error("Dataset connection mismatch");
    const id = reader.u64();
    const pending = this.pending.get(id);
    if (!pending) throw new Error("Unknown dataset result correlation");
    const tag = bytes[20];
    if (
      tag === this.tag("OUTCOME", true) ||
      tag === this.tag("REFUSED", true)
    ) {
      if (!pending.final) throw new Error("Unexpected dataset final result");
      this.pending.delete(id);
      clearTimeout(pending.timer);
      if (tag === this.tag("REFUSED", true)) {
        reader.u8();
        const error = new Error(reader.string());
        reader.end();
        pending.final.reject(error);
      } else pending.final.resolve(reader);
    } else if (tag === this.tag("ERROR", true)) {
      this.pending.delete(id);
      clearTimeout(pending.timer);
      reader.u8();
      const error = new Error(reader.string());
      reader.end();
      pending.reject(error);
      pending.final?.reject(error);
    } else {
      if (!pending.final) this.pending.delete(id);
      clearTimeout(pending.timer);
      delete pending.timer;
      pending.resolve(reader);
    }
    return true;
  }

  async create(
    name: string,
    kind: "buffer" | "streaming",
    schema: readonly DatasetColumn[],
  ): Promise<DatasetProducer> {
    const request = this.request("CREATE", (writer) => {
      this.name(writer, name);
      writer.u8(this.contract.tag(`DATASET_KIND_${kind.toUpperCase()}`));
      this.schema(writer, schema);
    });
    const reader = await request.reply;
    this.expect(reader, "CREATED");
    const incarnation = reader.u64();
    reader.end();
    return Object.freeze({ connection: this.connection(), incarnation });
  }

  release(producer: DatasetProducer): Promise<void> {
    return this.complete("RELEASE", producer);
  }
  destroy(producer: DatasetProducer): Promise<void> {
    return this.complete("DESTROY", producer);
  }

  private async complete(
    operation: "RELEASE" | "DESTROY",
    producer: DatasetProducer,
  ): Promise<void> {
    this.fence(producer);
    const reader = await this.request(operation, (writer) =>
      writer.u64(producer.incarnation),
    ).reply;
    this.expect(reader, "COMPLETE");
    reader.end();
  }

  /** A page is an independent observation; it neither retains history nor advances Host time. */
  async read(
    name: string,
    options: { incarnation?: bigint; offset?: bigint; limit?: number } = {},
  ): Promise<DatasetPage> {
    const reader = await this.request("READ", (writer) => {
      this.name(writer, name);
      writer.u64(options.incarnation ?? 0n);
      writer.u64(options.offset ?? 0n);
      writer.u32(options.limit ?? this.contract.limit("PAGE_ROWS"));
    }).reply;
    this.expect(reader, "PAGE");
    const page: DatasetPage = {
      name: reader.string(),
      incarnation: reader.u64(),
      kind: this.readKind(reader),
      schema: this.readSchema(reader),
      memory: {
        retainedRows: reader.u64(),
        retainedBytes: reader.u64(),
        allocatedBytes: reader.u64(),
        schemaBytes: reader.u64(),
      },
      rows: [],
      nextOffset: null,
    };
    const hasNext = reader.u8();
    if (hasNext > 1) throw new Error("Invalid dataset page option");
    const next = reader.u64();
    page.nextOffset = hasNext ? next : null;
    const count = reader.count(this.contract.limit("PAGE_ROWS"));
    for (let i = 0; i < count; i++)
      page.rows.push({ id: reader.u64(), values: this.readValues(reader) });
    reader.end();
    return page;
  }

  /** Observations never prepare, advance time, acquire demand or clear dirty. */
  async bindingView(
    session: bigint,
    entity: bigint,
    options: { offset?: bigint; limit?: number } = {},
  ): Promise<DataBindingPage> {
    const limit = options.limit ?? this.contract.limit("PAGE_ROWS");
    if (
      !Number.isInteger(limit) ||
      limit < 1 ||
      limit > this.contract.limit("PAGE_ROWS")
    )
      throw new RangeError("Binding page row limit");
    const reply = await this.request("BINDING_VIEW", (writer) => {
      writer.u64(session);
      writer.u64(entity);
      writer.u64(options.offset ?? 0n);
      writer.u32(limit);
    }).reply;
    const reader = this.observation(reply, "BINDING_VIEW");
    const page: DataBindingPage = {
      sourceIncarnation: this.optional(reader),
      bindingIncarnation: reader.u64(),
      evaluatedTick: this.optional(reader),
      availability: {
        reason: this.choice(reader, [
          "Ready",
          "NotEvaluated",
          "Source",
          "MissingAsset",
          "MissingInput",
          "InputType",
          "InvalidInputName",
        ]),
        detail: reader.string(),
        output: reader.string(),
        input: reader.string(),
      },
      dirty: this.boolean(reader),
      totalRows: reader.u64(),
      offset: reader.u64(),
      nextOffset: this.optional(reader),
      columns: [],
      rows: [],
    };
    const columns = reader.count(this.contract.limit("COLUMNS"));
    for (let i = 0; i < columns; i++)
      page.columns.push({
        name: reader.string(),
        kind: this.valueKind(reader.u8()),
      });
    const rows = reader.count(this.contract.limit("PAGE_ROWS"));
    for (let i = 0; i < rows; i++) {
      const row: DataBindingPage["rows"][number] = {
        id: reader.u64(),
        values: [],
      };
      for (let column = 0; column < columns; column++) {
        if (this.boolean(reader)) {
          const values = this.readValues(reader);
          if (
            values.length !== 1 ||
            values[0]!.kind !== page.columns[column]!.kind
          )
            throw new Error("Invalid binding value kind");
          row.values.push({ valid: true, value: values[0]! });
        } else
          row.values.push({
            valid: false,
            reason: this.choice(reader, [
              "MissingInput",
              "InvalidInput",
              "Calculation",
            ]),
            slot: this.optional(reader),
          });
      }
      page.rows.push(row);
    }
    reader.end();
    return page;
  }

  async driverStatus(
    session: bigint,
    entity: bigint,
  ): Promise<ExpressionDriverStatus> {
    const reply = await this.request("DRIVER_STATUS", (writer) => {
      writer.u64(session);
      writer.u64(entity);
    }).reply;
    const reader = this.observation(reply, "DRIVER_STATUS");
    const status: ExpressionDriverStatus = {
      availability: this.choice(reader, ["Pending", "Ready", "Unavailable"]),
      state: this.choice(reader, ["Prepared", "Written", "Retained"]),
      reason: this.choice(reader, [
        "",
        "AssetUnavailable",
        "AssetFailed",
        "InputMapping",
        "InputType",
        "MissingInput",
        "InvalidInput",
        "Calculation",
        "TargetUnavailable",
        "TargetRejected",
        "Cycle",
      ]),
      slot: this.optional(reader),
      detail: reader.string(),
      recovered: this.boolean(reader),
    };
    reader.end();
    return status;
  }

  private observation(reply: HostWireReader, kind: string): HostWireReader {
    this.expect(reply, kind);
    const size = reply.count(this.contract.limit("PAGE_BYTES"));
    const bytes = reply.raw(size);
    reply.end();
    return new HostWireReader(bytes);
  }
  private boolean(reader: HostWireReader): boolean {
    const value = reader.u8();
    if (value > 1) throw new Error("Invalid observation boolean");
    return value === 1;
  }
  private optional(reader: HostWireReader): bigint | null {
    const present = this.boolean(reader),
      value = reader.u64();
    if (!present && value !== 0n) throw new Error("Invalid observation option");
    return present ? value : null;
  }
  private choice<const T extends readonly string[]>(
    reader: HostWireReader,
    choices: T,
  ): T[number] {
    const value = reader.string();
    if (!choices.includes(value))
      throw new Error(`Invalid observation reason: ${value}`);
    return value;
  }

  /** Snapshot one bounded logical update before yielding. Queued bytes are bounded too. */
  update(
    producer: DatasetProducer,
    deltas: readonly DatasetDelta[],
  ): Promise<DatasetOutcome> {
    this.fence(producer);
    const bytes = this.encodeUpdate(deltas);
    if (
      bytes.length > this.contract.limit("UPDATE_BYTES") ||
      bytes.length > this.contract.limit("STAGING_BYTES") - this.queuedBytes ||
      this.queuedUpdates >= this.contract.limit("TRANSFERS")
    )
      return Promise.reject(
        new Error("Dataset client pressure: queued update capacity exhausted"),
      );
    this.queuedBytes += bytes.length;
    this.queuedUpdates++;
    const queued = this.operationTail.then(() => this.deliver(producer, bytes));
    this.operationTail = queued.then(
      () => {},
      () => {},
    );
    return queued.finally(() => {
      this.queuedBytes -= bytes.length;
      this.queuedUpdates--;
    });
  }

  /** Portable typed payload for the same finite-transfer boundary used by update(). */
  encodeUpdate(deltas: readonly DatasetDelta[]): Uint8Array<ArrayBuffer> {
    const writer = new HostWireWriter();
    this.count(writer, deltas.length, "DELTAS");
    for (const delta of deltas) {
      writer.u8(
        this.contract.tag(`DATASET_DELTA_${delta.operation.toUpperCase()}`),
      );
      if (delta.operation === "insert") writer.u64(delta.index);
      if (delta.operation === "edit" || delta.operation === "remove")
        writer.u64(delta.row);
      if (delta.operation === "edit") this.values(writer, delta.values);
      if (delta.operation === "append" || delta.operation === "insert") {
        writer.u32(delta.rows.length);
        for (const row of delta.rows) this.values(writer, row);
      }
    }
    const bytes = writer.finish();
    if (bytes.length > this.contract.limit("UPDATE_BYTES"))
      throw new RangeError("Dataset update byte limit");
    return bytes;
  }

  /** Explicit finite-transfer boundary for applications delivering already encoded typed bytes. */
  async begin(
    producer: DatasetProducer,
    length: bigint,
  ): Promise<DatasetTransfer> {
    this.fence(producer);
    let finalResolve!: (reader: HostWireReader) => void;
    let finalReject!: (error: Error) => void;
    const final = new Promise<HostWireReader>((resolve, reject) => {
      finalResolve = resolve;
      finalReject = reject;
    });
    const outcome = final.then((reader) => this.readOutcome(reader));
    // A refusal can precede the caller awaiting the final result.
    void outcome.catch(() => {});
    const request = this.request(
      "BEGIN",
      (writer) => {
        writer.u64(producer.incarnation);
        writer.u64(length);
      },
      { resolve: finalResolve, reject: finalReject },
    );
    const reader = await request.reply;
    this.expect(reader, "CREDIT");
    reader.end();
    return Object.freeze({
      connection: this.connection(),
      id: request.id,
      outcome,
    });
  }

  async chunk(
    transfer: DatasetTransfer,
    offset: bigint,
    bytes: Uint8Array,
  ): Promise<void> {
    this.fenceTransfer(transfer);
    if (bytes.length === 0 || bytes.length > this.contract.limit("CHUNK_BYTES"))
      throw new RangeError("Invalid dataset chunk size");
    await this.credit("CHUNK", (writer) => {
      writer.u64(transfer.id);
      writer.u64(offset);
      writer.bytes(bytes);
    });
  }

  async finish(transfer: DatasetTransfer): Promise<DatasetOutcome> {
    this.fenceTransfer(transfer);
    await this.credit("FINISH", (writer) => writer.u64(transfer.id));
    return transfer.outcome;
  }

  async cancel(transfer: DatasetTransfer): Promise<void> {
    this.fenceTransfer(transfer);
    await this.credit("CANCEL", (writer) => writer.u64(transfer.id));
  }

  private async deliver(
    producer: DatasetProducer,
    bytes: Uint8Array,
  ): Promise<DatasetOutcome> {
    const transfer = await this.begin(producer, BigInt(bytes.length));
    try {
      for (
        let offset = 0;
        offset < bytes.length;
        offset += this.contract.limit("CHUNK_BYTES")
      )
        await this.chunk(
          transfer,
          BigInt(offset),
          bytes.subarray(offset, offset + this.contract.limit("CHUNK_BYTES")),
        );
      return await this.finish(transfer);
    } catch (error) {
      await this.cancel(transfer).catch(() => {});
      throw error;
    }
  }

  private async credit(
    operation: string,
    encode: (writer: HostWireWriter) => void,
  ): Promise<void> {
    const reader = await this.request(operation, encode).reply;
    this.expect(reader, "CREDIT");
    reader.end();
  }

  private request(
    operation: string,
    encode: (writer: HostWireWriter) => void,
    final?: Pending["final"],
  ): { id: bigint; reply: Promise<HostWireReader> } {
    const id = this.nextId++;
    const writer = new HostWireWriter();
    writer.raw(new Uint8Array(this.contract.requestMagic));
    writer.u64(this.connection());
    writer.u64(id);
    writer.u8(this.tag(operation));
    encode(writer);
    const bytes = writer.finish();
    if (bytes.length > this.contract.limit("FRAME_BYTES"))
      throw new RangeError("Dataset frame exceeds byte budget");
    const reply = new Promise<HostWireReader>((resolve, reject) => {
      if (this.stopped) {
        reject(this.stopped);
        final?.reject(this.stopped);
        return;
      }
      const waiter: Pending = { resolve, reject, ...(final ? { final } : {}) };
      this.pending.set(id, waiter);
      const deadline = () => {
        if (this.pending.get(id) === waiter)
          waiter.timer = setTimeout(
            () =>
              this.fail(
                new Error("Dataset request timed out; outcome unknown"),
              ),
            this.timeoutMs,
          );
      };
      try {
        const leaving = this.send(bytes);
        if (leaving)
          void leaving.then(deadline, (error: unknown) =>
            this.fail(asError(error)),
          );
        else deadline();
      } catch (error) {
        this.pending.delete(id);
        reject(asError(error));
        final?.reject(asError(error));
      }
    });
    return { id, reply };
  }

  close(error: Error): void {
    if (this.stopped) return;
    this.stopped = error;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
      pending.final?.reject(error);
    }
    this.pending.clear();
  }

  private tag(name: string, response = false): number {
    return this.contract.tag(
      `DATASET_${response ? "RESPONSE" : "REQUEST"}_${name}`,
    );
  }
  private expect(reader: HostWireReader, name: string): void {
    if (reader.u8() !== this.tag(name, true))
      throw new Error("Unexpected dataset response");
  }
  private fenceTransfer(transfer: DatasetTransfer): void {
    if (transfer.connection !== this.connection())
      throw new Error("Dataset transfer belongs to another connection");
  }
  private fence(producer: DatasetProducer): void {
    if (
      producer.connection !== this.connection() ||
      producer.incarnation === 0n
    )
      throw new Error("Dataset producer belongs to another connection");
  }
  private count(writer: HostWireWriter, count: number, bound: string): void {
    if (count > this.contract.limit(bound))
      throw new RangeError(`Dataset ${bound} limit`);
    writer.u32(count);
  }
  private name(writer: HostWireWriter, name: string): void {
    if (
      new TextEncoder().encode(name).length > this.contract.limit("NAME_BYTES")
    )
      throw new RangeError("Dataset name byte limit");
    writer.string(name);
  }
  private schema(
    writer: HostWireWriter,
    columns: readonly DatasetColumn[],
  ): void {
    this.count(writer, columns.length, "COLUMNS");
    for (const column of columns) {
      this.name(writer, column.name);
      writer.u8(
        this.contract.tag(`DATASET_VALUE_${column.kind.toUpperCase()}`),
      );
      writer.u8(column.textMaxBytes === undefined ? 0 : 1);
      if (column.textMaxBytes !== undefined) writer.u64(column.textMaxBytes);
    }
  }
  private values(
    writer: HostWireWriter,
    values: readonly DatasetValue[],
  ): void {
    this.count(writer, values.length, "COLUMNS");
    for (const value of values) {
      writer.u8(this.contract.tag(`DATASET_VALUE_${value.kind.toUpperCase()}`));
      switch (value.kind) {
        case "i32":
          if (
            !Number.isInteger(value.value) ||
            value.value < -2147483648 ||
            value.value > 2147483647
          )
            throw new RangeError("Expected i32");
          writer.u32(value.value >>> 0);
          break;
        case "u32":
          writer.u32(value.value);
          break;
        case "f32":
          this.float(writer, value.value);
          break;
        case "bool":
          writer.u8(value.value ? 1 : 0);
          break;
        case "text":
          // Dataset text belongs to the complete update budget, rather than the
          // smaller Host-control field budget; the finite transfer chunks it.
          {
            const bytes = new TextEncoder().encode(value.value);
            writer.u32(bytes.length);
            writer.raw(bytes);
          }
          break;
        default:
          if (value.value.length !== lanes[value.kind])
            throw new RangeError("Dataset float lane count");
          for (const number of value.value) this.float(writer, number);
      }
    }
  }
  private float(writer: HostWireWriter, value: number): void {
    // IEEE-754 bits are transport representation. Finite-value validation belongs
    // to the ordered DataService delta boundary, including NaN and infinities.
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setFloat32(0, value, true);
    writer.raw(bytes);
  }
  private readKind(reader: HostWireReader): "buffer" | "streaming" {
    const tag = reader.u8();
    if (tag === this.contract.tag("DATASET_KIND_BUFFER")) return "buffer";
    if (tag === this.contract.tag("DATASET_KIND_STREAMING")) return "streaming";
    throw new Error("Invalid dataset kind");
  }
  private readSchema(reader: HostWireReader): DatasetColumn[] {
    const result: DatasetColumn[] = [];
    const count = reader.count(this.contract.limit("COLUMNS"));
    for (let i = 0; i < count; i++) {
      const name = reader.string();
      const kind = this.valueKind(reader.u8());
      const present = reader.u8();
      if (present > 1) throw new Error("Dataset text bound option");
      result.push({
        name,
        kind,
        ...(present ? { textMaxBytes: reader.u64() } : {}),
      });
    }
    return result;
  }
  private valueKind(tag: number): DatasetValueKind {
    for (const kind of kinds)
      if (tag === this.contract.tag(`DATASET_VALUE_${kind.toUpperCase()}`))
        return kind;
    throw new Error("Invalid dataset value kind");
  }
  private readValues(reader: HostWireReader): DatasetValue[] {
    const result: DatasetValue[] = [];
    const count = reader.count(this.contract.limit("COLUMNS"));
    for (let i = 0; i < count; i++) {
      const kind = this.valueKind(reader.u8());
      switch (kind) {
        case "i32":
          result.push({ kind, value: reader.u32() | 0 });
          break;
        case "u32":
          result.push({ kind, value: reader.u32() });
          break;
        case "f32":
          result.push({ kind, value: reader.f32() });
          break;
        case "bool": {
          const value = reader.u8();
          if (value > 1) throw new Error("Invalid dataset bool");
          result.push({ kind, value: value === 1 });
          break;
        }
        case "text":
          result.push({ kind, value: reader.string() });
          break;
        default:
          result.push({
            kind,
            value: Array.from({ length: lanes[kind] }, () => reader.f32()),
          });
      }
    }
    return result;
  }
  private readOutcome(reader: HostWireReader): DatasetOutcome {
    this.expect(reader, "OUTCOME");
    const failed = reader.u8();
    if (failed > 1) throw new Error("Dataset outcome tag");
    const committedDeltas = reader.u64();
    const assignedRows = reader.u64();
    const last = reader.u64();
    const result: DatasetOutcome = {
      committedDeltas,
      assignedRows,
      lastAssignedRow: last === 0n ? null : last,
    };
    if (failed)
      result.failure = { deltaIndex: reader.u64(), reason: reader.string() };
    reader.end();
    return result;
  }
}

const kinds: readonly DatasetValueKind[] = [
  "f32",
  "i32",
  "u32",
  "bool",
  "vec2",
  "vec3",
  "vec4",
  "mat2",
  "mat3",
  "mat4",
  "text",
];
const lanes = {
  vec2: 2,
  vec3: 3,
  vec4: 4,
  mat2: 4,
  mat3: 9,
  mat4: 16,
} as const;
function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
