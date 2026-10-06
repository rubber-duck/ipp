import type { DatasetProducer } from "../../../packages/ipp-client/src/datasets.js";
import type { DatasetHost } from "../scenarios/datasets.js";

/** Fault injection stays with the transfer driver, independently of scenario intent. */
export async function malformedDatasetPayload(
  host: DatasetHost,
  producer: DatasetProducer,
): Promise<void> {
  const complete = host.datasets.encodeUpdate([
    { operation: "append", rows: [[{ kind: "u32", value: 111 }]] },
    { operation: "append", rows: [[{ kind: "u32", value: 222 }]] },
  ]);
  const truncated = complete.subarray(0, complete.length - 1);
  const transfer = await host.datasets.begin(
    producer,
    BigInt(truncated.length),
  );
  await host.datasets.chunk(transfer, 0n, truncated);
  try {
    await host.datasets.finish(transfer);
  } catch (error) {
    if (error instanceof Error && /dataset transport/.test(error.message))
      return;
    throw error;
  }
  throw new Error("Malformed complete dataset representation was admitted");
}

/** A full staging reservation with a valid payload, measured by the production encoder. */
export function pressureDatasetPayload(
  host: DatasetHost,
): Uint8Array<ArrayBuffer> {
  const targetBytes = 1048576;
  const lengths = [...Array.from({ length: 14 }, () => 65536), 70000, 60000, 0];
  const encode = () =>
    host.datasets.encodeUpdate([
      {
        operation: "append",
        rows: lengths.map((length) => [
          { kind: "text", value: "p".repeat(length) },
        ]),
      },
    ]);
  lengths[16] = targetBytes - encode().length;
  const payload = encode();
  if (payload.length !== targetBytes)
    throw new Error("Pressure fixture does not fill the declared reservation");
  return payload;
}

export async function deliverPressureDatasetPayload(
  host: DatasetHost,
  transfer: Parameters<DatasetHost["datasets"]["chunk"]>[0],
  bytes: Uint8Array,
) {
  for (let offset = 0; offset < bytes.length; offset += 65536)
    await host.datasets.chunk(
      transfer,
      BigInt(offset),
      bytes.subarray(offset, offset + 65536),
    );
  return host.datasets.finish(transfer);
}
