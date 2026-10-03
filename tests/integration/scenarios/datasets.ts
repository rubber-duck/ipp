import {
  malformedDatasetPayload,
  pressureDatasetPayload,
  deliverPressureDatasetPayload,
} from "../dataset-faults.js";
import type {
  Client,
  WorldPersistenceHostClient,
} from "../../../packages/ipp-client/src/index.js";
import type {
  DatasetColumn,
  DatasetValue,
} from "../../../packages/ipp-client/src/datasets.js";

export type DatasetHost = WorldPersistenceHostClient<Client>;

export function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
function equal(actual: unknown, expected: unknown, message: string) {
  const encode = (value: unknown) =>
    JSON.stringify(value, (_, value) =>
      typeof value === "bigint" ? `${value}n` : value,
    );
  check(
    encode(actual) === encode(expected),
    `${message}: ${encode(actual)} != ${encode(expected)}`,
  );
}
async function refused(action: Promise<unknown>, reason: RegExp) {
  try {
    await action;
  } catch (error) {
    check(
      error instanceof Error && reason.test(error.message),
      `wrong refusal: ${String(error)}`,
    );
    return;
  }
  throw new Error("Expected dataset refusal");
}

/** Independent exact typed fixture shared by every transport arrangement. */
export const datasetSchema: readonly DatasetColumn[] = [
  { name: "unsigned", kind: "u32" },
  { name: "signed", kind: "i32" },
  { name: "scalar", kind: "f32" },
  { name: "enabled", kind: "bool" },
  { name: "label", kind: "text", textMaxBytes: 32n },
  ...(["vec2", "vec3", "vec4", "mat2", "mat3", "mat4"] as const).map(
    (kind) => ({ name: kind, kind }),
  ),
];
export function datasetRow(value: number): DatasetValue[] {
  return [
    { kind: "u32", value: 4294967295 - value },
    { kind: "i32", value: -2147483648 + value },
    { kind: "f32", value: value + 0.5 },
    { kind: "bool", value: value % 2 === 0 },
    { kind: "text", value: `row α ${value}` },
    ...(
      [
        ["vec2", 2],
        ["vec3", 3],
        ["vec4", 4],
        ["mat2", 4],
        ["mat3", 9],
        ["mat4", 16],
      ] as const
    ).map(([kind, lanes]) => ({
      kind,
      value: Array.from({ length: lanes }, (_, i) => value + i / 2),
    })),
  ];
}

/** Real source-only milestone. Bindings/windows extend this same scenario after core integration. */
export async function datasets(
  host: DatasetHost,
  connectFresh: () => Promise<DatasetHost>,
) {
  const source = "datasets://shared/literal α";
  const producer = await host.datasets.create(source, "buffer", datasetSchema);
  let peer: DatasetHost | undefined;
  let replacement: DatasetHost | undefined;
  const observations: unknown[] = [];
  try {
    equal(
      await host.datasets.update(producer, [
        { operation: "append", rows: [datasetRow(0), datasetRow(1)] },
      ]),
      { committedDeltas: 1n, assignedRows: 2n, lastAssignedRow: 2n },
      "append outcome",
    );
    equal(
      (await host.datasets.read(source)).schema,
      datasetSchema,
      "exact raw schema",
    );
    await host.datasets.update(producer, [
      { operation: "insert", index: 1n, rows: [datasetRow(2)] },
      { operation: "edit", row: 1n, values: datasetRow(3) },
      { operation: "remove", row: 2n },
    ]);
    const page = await host.datasets.read(source, {
      incarnation: producer.incarnation,
    });
    equal(
      page.rows,
      [
        { id: 1n, values: datasetRow(3) },
        { id: 3n, values: datasetRow(2) },
      ],
      "buffer order and stable edited IDs",
    );
    equal(page.name, source, "source must not be asset/session rewritten");
    observations.push({ typedBuffer: page.rows });
    const prefix = await host.datasets.update(producer, [
      { operation: "append", rows: [datasetRow(4)] },
      { operation: "edit", row: 1n, values: [{ kind: "u32", value: 9 }] },
      { operation: "remove", row: 3n },
    ]);
    equal(
      prefix,
      {
        committedDeltas: 1n,
        assignedRows: 1n,
        lastAssignedRow: 4n,
        failure: { deltaIndex: 1n, reason: "InvalidRow" },
      },
      "ordered prefix failure",
    );
    equal(
      (await host.datasets.read(source)).rows.map((row) => row.id),
      [1n, 3n, 4n],
      "failed suffix made no changes",
    );
    observations.push({ prefix });

    // Logical update exceeds one transport chunk and paginates its production typed observations.
    await host.datasets.update(producer, [
      {
        operation: "append",
        rows: Array.from({ length: 500 }, (_, i) => datasetRow(i + 10)),
      },
    ]);
    const ids: bigint[] = [];
    let offset = 0n;
    do {
      const page = await host.datasets.read(source, {
        incarnation: producer.incarnation,
        offset,
      });
      ids.push(...page.rows.map((row) => row.id));
      if (page.nextOffset === null) break;
      check(page.nextOffset > offset, "read pagination stalled");
      offset = page.nextOffset;
    } while (true);
    equal(
      ids,
      [1n, 3n, 4n, ...Array.from({ length: 500 }, (_, i) => BigInt(i + 5))],
      "headless pagination preserved stable IDs",
    );

    const nonfinite = await host.datasets.create(
      "datasets://nonfinite",
      "buffer",
      [{ name: "value", kind: "f32" }],
    );
    const nonfiniteOutcome = await host.datasets.update(nonfinite, [
      { operation: "append", rows: [[{ kind: "f32", value: 1 }]] },
      { operation: "append", rows: [[{ kind: "f32", value: Number.NaN }]] },
      { operation: "append", rows: [[{ kind: "f32", value: 2 }]] },
    ]);
    equal(
      nonfiniteOutcome,
      {
        committedDeltas: 1n,
        assignedRows: 1n,
        lastAssignedRow: 1n,
        failure: { deltaIndex: 1n, reason: "InvalidRow" },
      },
      "float domain validation prevalidated the entire update",
    );
    equal(
      (await host.datasets.read("datasets://nonfinite")).rows,
      [{ id: 1n, values: [{ kind: "f32", value: 1 }] }],
      "nonfinite suffix erased valid prefix",
    );
    await host.datasets.destroy(nonfinite);

    const malformedProducer = await host.datasets.create(
      "datasets://malformed",
      "buffer",
      [{ name: "value", kind: "u32" }],
    );
    await malformedDatasetPayload(host, malformedProducer);
    equal(
      (await host.datasets.read("datasets://malformed")).rows,
      [],
      "transport refusal must not apply a valid-looking prefix",
    );
    await host.datasets.destroy(malformedProducer);

    peer = await connectFresh();
    await refused(
      peer.datasets.create(source, "buffer", datasetSchema),
      /ProducerExists/,
    );
    await refused(
      peer.datasets.begin(
        {
          connection: (
            await peer.datasets.create(
              "datasets://peer",
              "buffer",
              datasetSchema,
            )
          ).connection,
          incarnation: producer.incarnation,
        },
        4n,
      ),
      /StaleProducer/,
    );
    const pressureProducer = await host.datasets.create(
      "datasets://pressure",
      "buffer",
      [{ name: "text", kind: "text", textMaxBytes: 70000n }],
    );
    const pressurePayload = pressureDatasetPayload(host);
    const parked = await host.datasets.begin(
      pressureProducer,
      BigInt(pressurePayload.length),
    );
    const parked2 = await host.datasets.begin(producer, 1048576n);
    await refused(host.datasets.begin(producer, 1n), /pressure/);
    equal(
      (await peer.datasets.read(source)).memory.retainedRows,
      503n,
      "healthy peer progressed under staging pressure",
    );
    const admitted = await deliverPressureDatasetPayload(
      host,
      parked,
      pressurePayload,
    );
    equal(
      admitted.assignedRows,
      17n,
      "pressure lost accepted data or withheld continuation progress",
    );
    await refused(
      peer.datasets.read("datasets://pressure"),
      /dataset page row/,
    );
    const tail = await peer.datasets.read("datasets://pressure", {
      offset: 15n,
      limit: 1,
    });
    equal(
      tail.memory.retainedRows,
      17n,
      "pressure silently lost retained accepted rows",
    );
    equal(
      tail.rows,
      [{ id: 16n, values: [{ kind: "text", value: "p".repeat(60000) }] }],
      "oversized-record pagination and admitted data",
    );
    await host.datasets.destroy(pressureProducer);
    await host.datasets.cancel(parked2);
    await refused(parked2.outcome, /cancelled/);
    const incomplete = await host.datasets.begin(producer, 4n);
    await refused(host.datasets.finish(incomplete), /Incomplete/);
    const outOfOrder = await host.datasets.begin(producer, 4n);
    await refused(
      host.datasets.chunk(outOfOrder, 1n, Uint8Array.of(0)),
      /InvalidChunkBounds/,
    );
    await refused(outOfOrder.outcome, /InvalidChunkBounds/);
    equal(
      (await peer.datasets.read(source)).memory.retainedRows,
      503n,
      "interrupted transfers mutated source",
    );

    const stream = await host.datasets.create(
      "datasets://unconsumed",
      "streaming",
      datasetSchema,
    );
    const expired = await host.datasets.update(stream, [
      { operation: "append", rows: [datasetRow(0), datasetRow(1)] },
    ]);
    equal(
      expired.lastAssignedRow,
      2n,
      "no-consumer arrivals still assign identities",
    );
    equal(
      (await host.datasets.read("datasets://unconsumed")).rows,
      [],
      "stream read must not create demand",
    );
    await host.datasets.destroy(stream);
    observations.push({ unconsumedStream: expired, paginatedRows: ids.length });

    // A separate World batch is deliberately kept open while the source lane completes.
    const world = await host.createWorld({
      selectedSystems: [],
      symbolicId: "datasets-open-batch",
    });
    const client = await host.openWorld(world.reference);
    try {
      const batch = client.openBatch();
      batch.write(
        Array.from({ length: 1025 }, (_, i) => ({
          kind: "create" as const,
          alias: i + 1,
          adopt: false,
          metadata: { symbolicId: `dataset-batch-${i}`, classes: [] },
        })),
      );
      const update = await host.datasets.update(producer, [
        { operation: "append", rows: [datasetRow(700)] },
      ]);
      equal(
        update.lastAssignedRow,
        505n,
        "source lane blocked behind open World batch",
      );
      equal(
        (await client.inspectPage({ collection: "entities" })).entities.length,
        0,
        "open World batch committed prematurely",
      );
      const committed = await batch.finish();
      check(committed.ok, "open World batch failed after dataset progress");
    } finally {
      await client.close();
      await host.destroyWorld(world.reference);
    }

    const old = await host.datasets.begin(producer, 4n);
    await host.datasets.chunk(old, 0n, host.datasets.encodeUpdate([]));
    await host.datasets.release(producer);
    replacement = await connectFresh();
    const fresh = await replacement.datasets.create(
      source,
      "buffer",
      datasetSchema,
    );
    check(
      fresh.incarnation !== producer.incarnation,
      "replacement reused incarnation",
    );
    const stale = await host.datasets.finish(old);
    equal(
      stale.failure?.reason,
      "StaleProducer",
      "queued old producer retargeted replacement",
    );
    await refused(
      host.datasets.read(source, { incarnation: producer.incarnation }),
      /StaleSource/,
    );
    await refused(host.datasets.destroy(producer), /StaleProducer/);
    await replacement.datasets.update(fresh, [
      { operation: "append", rows: [datasetRow(8)] },
    ]);
    await replacement.close();
    replacement = undefined;
    await refused(host.datasets.read(source), /MissingSource/);
    const recovered = await peer.datasets.create(
      source,
      "buffer",
      datasetSchema,
    );
    await peer.datasets.destroy(recovered);
    await refused(host.datasets.read(source), /MissingSource/);
    return observations;
  } finally {
    await replacement?.close();
    await peer?.close();
    await host.datasets.destroy(producer).catch(() => {});
  }
}
