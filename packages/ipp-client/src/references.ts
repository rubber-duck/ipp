import type {
  CanvasOutputReference,
  OutputReference,
  WorldReference,
} from "./types.js";

interface ReferenceReader {
  u8(): number;
  u64(): bigint;
}

interface ReferenceWriter {
  u8(value: number): unknown;
  u64(value: bigint): unknown;
}

export function readWorldReference(reader: ReferenceReader): WorldReference {
  return { id: reader.u64(), incarnation: reader.u64() };
}

export function writeWorldReference(
  writer: ReferenceWriter,
  value: WorldReference,
): void {
  writer.u64(value.id);
  writer.u64(value.incarnation);
}

/** The World-level canvas output of a World that selects the Canvas System. */
export function canvasOutput(world: WorldReference): CanvasOutputReference {
  return { world, kind: "canvas" };
}

/** The exact producer entity a reference names: a Camera's. The World
 * canvas names none. */
export function outputProducer(
  output: OutputReference,
): { entity: bigint; incarnation: bigint } | undefined {
  return output.kind === "camera"
    ? { entity: output.entity, incarnation: output.incarnation }
    : undefined;
}

/** Exact equality of two output selections, including their lifetimes. */
export function sameOutputReference(
  left: OutputReference,
  right: OutputReference,
): boolean {
  const leftProducer = outputProducer(left);
  const rightProducer = outputProducer(right);
  return (
    left.world.id === right.world.id &&
    left.world.incarnation === right.world.incarnation &&
    left.kind === right.kind &&
    leftProducer?.entity === rightProducer?.entity &&
    leftProducer?.incarnation === rightProducer?.incarnation
  );
}

const OUTPUT_TARGET_CANVAS = 0;
const OUTPUT_TARGET_CAMERA = 1;

export function readOutputReference(reader: ReferenceReader): OutputReference {
  const world = readWorldReference(reader);
  const target = reader.u8();
  if (target === OUTPUT_TARGET_CANVAS) return { world, kind: "canvas" };
  if (target !== OUTPUT_TARGET_CAMERA) throw new Error("Invalid output target");
  return {
    world,
    kind: "camera",
    entity: reader.u64(),
    incarnation: reader.u64(),
  };
}

export function writeOutputReference(
  writer: ReferenceWriter,
  value: OutputReference,
): void {
  if (value.kind !== "canvas" && value.kind !== "camera")
    throw new Error("Invalid output kind");
  writeWorldReference(writer, value.world);
  if (value.kind === "canvas") {
    writer.u8(OUTPUT_TARGET_CANVAS);
    return;
  }
  if (typeof value.entity !== "bigint" || typeof value.incarnation !== "bigint")
    throw new Error("Camera output requires its entity");
  writer.u8(OUTPUT_TARGET_CAMERA);
  writer.u64(value.entity);
  writer.u64(value.incarnation);
}
