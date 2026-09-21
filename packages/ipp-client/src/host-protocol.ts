/** Host control wire primitives. World authoring keeps its target-generated codec. */
export interface WorldCapacityHints {
  entities: number;
  systems: Record<string, Record<string, number>>;
}

export interface WorldCapacityHintsPatch {
  entities?: number;
  systems?: Record<string, Record<string, number>>;
}

export interface WorldDescriptor {
  id: bigint;
  symbolicId: string;
  persistentId: bigint;
  capacityHints: WorldCapacityHints;
}

export type WorldSelector = bigint | string;

export interface WorldCreateOptions {
  symbolicId?: string;
  capacityHints?: WorldCapacityHintsPatch;
  /** Destroy on the creating Host connection's close. Defaults to retained. */
  temporary?: boolean;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

export class HostWireWriter {
  private parts: Uint8Array[] = [];
  private size = 0;

  raw(bytes: Uint8Array): void {
    if (this.size + bytes.length > 1_048_576)
      throw new RangeError("Host message exceeds byte budget");
    this.parts.push(bytes);
    this.size += bytes.length;
  }

  u8(value: number): void {
    this.raw(Uint8Array.of(value));
  }

  u32(value: number): void {
    if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff)
      throw new RangeError("Expected unsigned 32-bit value");
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setUint32(0, value, true);
    this.raw(bytes);
  }

  u64(value: bigint): void {
    if (value < 0n || value > 0xffff_ffff_ffff_ffffn)
      throw new RangeError("Expected unsigned 64-bit value");
    const bytes = new Uint8Array(8);
    new DataView(bytes.buffer).setBigUint64(0, value, true);
    this.raw(bytes);
  }

  bytes(value: Uint8Array): void {
    if (value.length > 65_536)
      throw new RangeError("Host field exceeds byte budget");
    this.u32(value.length);
    this.raw(value);
  }

  string(value: string): void {
    this.bytes(encoder.encode(value));
  }

  selector(value: WorldSelector, idTag: number, symbolTag: number): void {
    this.u8(typeof value === "bigint" ? idTag : symbolTag);
    if (typeof value === "bigint") this.u64(value);
    else this.string(value);
  }

  hints(value: WorldCapacityHintsPatch = {}): void {
    this.u8(value.entities === undefined ? 0 : 1);
    if (value.entities !== undefined) this.u32(value.entities);
    const systems = Object.entries(value.systems ?? {}).sort(([a], [b]) =>
      a.localeCompare(b),
    );
    if (systems.length > 1024) throw new RangeError("Too many systems");
    this.u32(systems.length);
    for (const [system, hints] of systems) {
      this.string(system);
      const values = Object.entries(hints).sort(([a], [b]) =>
        a.localeCompare(b),
      );
      if (values.length > 1024) throw new RangeError("Too many capacity hints");
      this.u32(values.length);
      for (const [key, value] of values) {
        this.string(key);
        this.u32(value);
      }
    }
  }

  finish(): Uint8Array<ArrayBuffer> {
    const bytes = new Uint8Array(this.size);
    let offset = 0;
    for (const part of this.parts) {
      bytes.set(part, offset);
      offset += part.length;
    }
    return bytes;
  }
}

export class HostWireReader {
  private at = 0;

  constructor(private readonly input: Uint8Array) {
    if (input.length > 1_048_576)
      throw new Error("Host message exceeds byte budget");
  }

  raw(length: number): Uint8Array {
    if (length > this.input.length - this.at)
      throw new Error("Truncated Host response");
    const value = this.input.subarray(this.at, this.at + length);
    this.at += length;
    return value;
  }

  u8(): number {
    return this.raw(1)[0]!;
  }

  u32(): number {
    const bytes = this.raw(4);
    return new DataView(bytes.buffer, bytes.byteOffset, 4).getUint32(0, true);
  }

  u64(): bigint {
    const bytes = this.raw(8);
    return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(
      0,
      true,
    );
  }

  count(max: number): number {
    const count = this.u32();
    if (count > max) throw new Error("Invalid Host collection length");
    return count;
  }

  bytes(): Uint8Array {
    return this.raw(this.count(65_536));
  }

  string(): string {
    return decoder.decode(this.bytes());
  }

  world(): WorldDescriptor {
    const id = this.u64();
    const symbolicId = this.string();
    const persistentId = this.u64() | (this.u64() << 64n);
    const entities = this.u32();
    const systems: Record<string, Record<string, number>> = Object.create(null);
    const count = this.count(1024);
    for (let i = 0; i < count; i++) {
      const name = this.string();
      if (Object.hasOwn(systems, name))
        throw new Error("Duplicate Host system");
      const values: Record<string, number> = Object.create(null);
      const count = this.count(1024);
      for (let j = 0; j < count; j++) {
        const key = this.string();
        if (Object.hasOwn(values, key))
          throw new Error("Duplicate capacity hint");
        values[key] = this.u32();
      }
      systems[name] = values;
    }
    if (id === 0n || persistentId === 0n)
      throw new Error("Invalid World identity");
    return {
      id,
      symbolicId,
      persistentId,
      capacityHints: { entities, systems },
    };
  }

  end(): void {
    if (this.at !== this.input.length)
      throw new Error("Trailing Host response bytes");
  }
}
