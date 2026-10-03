import type {
  AnimationWorldClient,
  Client,
  Command,
  DynamicValue,
} from "../../../packages/ipp-client/src/index.js";
import type {
  DataBindingPage,
  DatasetValueKind,
} from "../../../packages/ipp-client/src/datasets.js";
import type * as Generated from "../../../target/integration-artifacts/client/generated.js";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import {
  ANIMATION,
  ASSETS,
  CONSTRAINTS,
  selectSystems,
} from "../system-selections.js";
import {
  check,
  datasetRow,
  datasetSchema,
  type DatasetHost,
} from "./datasets.js";

type Contract = Pick<
  typeof Generated,
  | "ExpressionBuilder"
  | "encodeDataWindows"
  | "encodeExpressionDriverInputs"
  | "encodeAnimationClip"
>;
const DATA = selectSystems(ASSETS, CONSTRAINTS, ANIMATION, [
  "ipp.data-bindings",
]);
const handle = (id: bigint) => ({ kind: "handle" as const, id });
function equal(actual: unknown, expected: unknown, label: string): void {
  const json = (value: unknown) =>
    JSON.stringify(value, (_, value) =>
      typeof value === "bigint" ? `${value}n` : value,
    );
  check(
    json(actual) === json(expected),
    `${label}: ${json(actual)} != ${json(expected)}`,
  );
}

/** Readiness is bounded production observations, with no sleep or tick control. */
async function until<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  label: string,
): Promise<T> {
  const deadline = performance.now() + 10_000;
  let last: T;
  do {
    last = await read();
    if (ready(last)) return last;
  } while (performance.now() < deadline);
  throw new Error(
    `${label} timed out: ${JSON.stringify(last!, (_, value) => (typeof value === "bigint" ? `${value}n` : value))}`,
  );
}

function property(
  client: Client,
  entity: bigint,
  component: string,
  name: string,
  value: DynamicValue,
): Command {
  return {
    kind: "setDynamicProperty",
    entity: handle(entity),
    component: client.components[component]!.id,
    name,
    value,
  };
}
async function create(
  client: Client,
  symbol: string,
  component: string,
  values: Parameters<typeof componentFields>[2],
): Promise<bigint> {
  return aliasId(
    await client.batch([
      createEntity(1, symbol),
      insertComponent(client, component, { kind: "alias", alias: 1 }, values),
    ]),
    1,
  );
}
async function set(
  client: Client,
  entity: bigint,
  component: string,
  values: Parameters<typeof componentFields>[2],
): Promise<void> {
  successfulBatch(
    await client.batch(
      componentFields(client, component, values).map((field) => ({
        kind: "setField",
        entity: handle(entity),
        component: client.components[component]!.id,
        field,
      })),
    ),
  );
}
async function upload(
  client: AnimationWorldClient,
  kind: number,
  id: bigint,
  bytes: Uint8Array<ArrayBuffer>,
): Promise<string> {
  const asset = clientAssetSource(client.session, kind, id);
  await client.registerAsset(asset, bytes.buffer);
  return asset.source;
}
function output(page: DataBindingPage, name: string) {
  const index = page.columns.findIndex((column) => column.name === name);
  check(index >= 0, `missing output ${name}`);
  return page.rows.map((row) => row.values[index]);
}
function valid(kind: DatasetValueKind, value: unknown) {
  return { valid: true, value: { kind, value } };
}

/** Shared combined scenario: generated authors, actual assets, Host DataService, commands and read-only results. */
export async function dataAuthoring(
  host: DatasetHost,
  contract: Contract,
  record: (label: string, value: unknown) => Promise<void> = async () => {},
) {
  const world = await host.createWorld({
    selectedSystems: DATA,
    symbolicId: "combined-data",
  });
  const client = (await host.openWorld(
    world.reference,
  )) as AnimationWorldClient;
  const worlds = [world.reference];
  const clients: Client[] = [client];
  const producers: Awaited<ReturnType<DatasetHost["datasets"]["create"]>>[] =
    [];
  const source = "datasets://combined/buffer";
  const producer = await host.datasets.create(source, "buffer", datasetSchema);
  producers.push(producer);
  const artifacts: unknown[] = [];
  const checkpoint = async (label: string, value: unknown) => {
    artifacts.push({ label, value });
    await record(label, value);
  };
  let snapshot: Uint8Array<ArrayBuffer> | undefined;
  try {
    const initialDeltas = [
      { operation: "append" as const, rows: [datasetRow(2), datasetRow(4)] },
    ];
    const updateStart = performance.now();
    const initialOutcome = await host.datasets.update(producer, initialDeltas);
    const ingestionMs = performance.now() - updateStart;
    const sourceMemory = (await host.datasets.read(source)).memory;
    check(
      sourceMemory.retainedRows === 2n &&
        sourceMemory.allocatedBytes >= sourceMemory.retainedBytes &&
        sourceMemory.schemaBytes > 0n,
      "source accounting missing",
    );
    await checkpoint("data.ingestion", {
      encodedBytes: host.datasets.encodeUpdate(initialDeltas).length,
      ingestionMs,
      initialOutcome,
      sourceMemory,
    });
    const binding = await create(
      client,
      "projection",
      "BufferDataSourceBinding",
      { source },
    );
    let assetId = 800n;
    for (const column of datasetSchema) {
      const builder = new contract.ExpressionBuilder();
      const input = builder.input(`column:${column.name}`, column.kind);
      const source = await upload(client, 19, assetId++, builder.encode(input));
      successfulBatch(
        await client.batch([
          property(
            client,
            binding,
            "BufferDataSourceBinding",
            column.kind === "f32" ? "identity" : column.name,
            { kind: "asset", value: { kind: 19, source } },
          ),
        ]),
      );
    }
    const builder = new contract.ExpressionBuilder();
    const raw = builder.input("column:scalar", "f32");
    const parameter = builder.input("parameter", "f32");
    const fallback = builder.fallback(
      parameter,
      builder.constant({ kind: "f32", value: 2 }),
    );
    const formula = await upload(
      client,
      19,
      assetId++,
      builder.encode(builder.binary("multiply", raw, fallback)),
    );
    const strict = await upload(
      client,
      19,
      assetId++,
      builder.encode(builder.binary("divide", raw, parameter)),
    );
    successfulBatch(
      await client.batch([
        property(client, binding, "BufferDataSourceBinding", "scaled", {
          kind: "asset",
          value: { kind: 19, source: formula },
        }),
        property(client, binding, "BufferDataSourceBinding", "strict", {
          kind: "asset",
          value: { kind: 19, source: strict },
        }),
      ]),
    );
    const read = () => host.datasets.bindingView(client.session, binding);
    let page = await until(
      read,
      (page) => page.availability.reason === "Ready" && page.rows.length === 2,
      "initial projections",
    );
    equal(
      output(page, "scaled"),
      [valid("f32", 5), valid("f32", 9)],
      "initial missing parameter fallback",
    );
    equal(
      output(page, "strict"),
      [
        { valid: false, reason: "MissingInput", slot: 1n },
        { valid: false, reason: "MissingInput", slot: 1n },
      ],
      "explicit missing parameter invalidity",
    );
    for (let i = 0; i < datasetSchema.length; i++) {
      const column = datasetSchema[i]!;
      equal(
        output(page, column.kind === "f32" ? "identity" : column.name),
        [2, 4].map((value) => ({ valid: true, value: datasetRow(value)[i] })),
        `identity ${column.kind}`,
      );
    }
    check(
      page.dirty &&
        page.sourceIncarnation === producer.incarnation &&
        page.evaluatedTick !== null,
      "completed binding fences/dirty missing",
    );
    const unchanged = await read();
    equal(
      { ...unchanged, evaluatedTick: null },
      { ...page, evaluatedTick: null },
      "unchanged observation changed prepared values",
    );
    check(unchanged.dirty, "observation cleared dirty");
    successfulBatch(
      await client.batch([
        property(
          client,
          binding,
          "BufferDataSourceBinding",
          "scaled_parameter",
          { kind: "f32", value: 3 },
        ),
        property(
          client,
          binding,
          "BufferDataSourceBinding",
          "strict_parameter",
          { kind: "f32", value: 0 },
        ),
      ]),
    );
    page = await until(
      read,
      (page) =>
        output(page, "scaled")[0]?.valid === true &&
        JSON.stringify(output(page, "scaled")[0]) ===
          JSON.stringify(valid("f32", 7.5)),
      "parameter edit",
    );
    equal(
      output(page, "strict"),
      [
        { valid: false, reason: "Calculation", slot: null },
        { valid: false, reason: "Calculation", slot: null },
      ],
      "division invalidity",
    );
    check(page.dirty, "changed observation cleared dirty");
    successfulBatch(
      await client.batch([
        {
          kind: "removeDynamicProperty",
          entity: handle(binding),
          component: client.components.BufferDataSourceBinding!.id,
          name: "scaled_parameter",
        },
      ]),
    );
    page = await until(
      read,
      (page) =>
        JSON.stringify(output(page, "scaled")[0]) ===
        JSON.stringify(valid("f32", 5)),
      "removed parameter fallback",
    );
    await host.datasets.update(producer, [
      { operation: "edit", row: 1n, values: datasetRow(8) },
      { operation: "remove", row: 2n },
      { operation: "insert", index: 0n, rows: [datasetRow(6)] },
    ]);
    page = await until(
      read,
      (page) => page.rows.length === 2 && page.rows[0]!.id === 3n,
      "row edits/removal",
    );
    equal(
      page.rows.map((row) => row.id),
      [3n, 1n],
      "stable edited/inserted row identities",
    );
    equal(
      output(page, "scaled"),
      [valid("f32", 13), valid("f32", 17)],
      "edited row independent arithmetic",
    );
    for (let i = 0; i < datasetSchema.length; i++) {
      const column = datasetSchema[i]!;
      equal(
        output(page, column.kind === "f32" ? "identity" : column.name),
        [6, 8].map((value) => ({ valid: true, value: datasetRow(value)[i] })),
        `edited multidimensional ${column.kind}`,
      );
    }
    const first = await host.datasets.bindingView(client.session, binding, {
      limit: 1,
    });
    const second = await host.datasets.bindingView(client.session, binding, {
      offset: first.nextOffset!,
      limit: 1,
    });
    equal(
      [first.rows[0]!.id, second.rows[0]!.id],
      [3n, 1n],
      "binding page continuation",
    );
    equal(
      [first.bindingIncarnation, first.sourceIncarnation],
      [second.bindingIncarnation, second.sourceIncarnation],
      "page lifetime fences",
    );
    check(first.dirty && second.dirty, "paging cleared dirty");
    await checkpoint("data.projections", {
      initialAndEdited: page,
      pages: [first, second],
    });

    // Real AnimationController targets the ordinary dynamic property by name.
    successfulBatch(
      await client.batch([
        property(
          client,
          binding,
          "BufferDataSourceBinding",
          "scaled_parameter",
          { kind: "f32", value: 2 },
        ),
      ]),
    );
    const component = client.components.BufferDataSourceBinding!.id;
    const clipSource = await upload(
      client,
      10,
      assetId++,
      contract.encodeAnimationClip({
        duration: 2,
        tracks: [
          {
            property: { component, name: "scaled_parameter" },
            keys: [
              {
                time: 0,
                value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
                interpolation: { kind: "linear" },
              },
              {
                time: 2,
                value: { kind: "dynamic", value: { kind: "f32", value: 4 } },
              },
            ],
          },
        ],
      }),
    );
    const controller = await client.createAnimationController({
      speed: 0,
      drivers: [
        {
          source: clipSource,
          track: 0,
          target: binding,
          property: { component, name: "scaled_parameter" },
        },
      ],
    });
    await client.controlAnimationController(controller, { action: "play" });
    await until(
      () => client.inspect(),
      (result) =>
        result.controllers?.some(
          (item) => item.id === controller && item.state === "playing",
        ) ?? false,
      "animation asset/controller readiness",
    );
    await client.controlAnimationController(controller, { action: "pause" });
    await client.controlAnimationController(controller, {
      action: "seek",
      time: 1,
    });
    page = await until(
      read,
      (page) =>
        JSON.stringify(output(page, "scaled")[0]) ===
        JSON.stringify(valid("f32", 26)),
      "animated parameter projection",
    );
    equal(
      output(page, "scaled"),
      [valid("f32", 26), valid("f32", 34)],
      "animated projection independent expected values",
    );

    // Driver writes 3 absolutely, then animation adds its paused contribution 2, then projection reads 5.
    const scalar = await create(client, "parameter-input", "Scalar", {
      value: 3,
    });
    const expression = new contract.ExpressionBuilder();
    const identity = await upload(
      client,
      19,
      assetId++,
      expression.encode(expression.input("x", "f32")),
    );
    const inspect = await client.inspect();
    const parameterOffset = inspect.entities
      .find((entity) => entity.id === binding)!
      .components.find((item) => item.component === component)!
      .propertyDescriptors!.scaled_parameter!.offset;
    successfulBatch(
      await client.batch([
        insertComponent(client, "ExpressionDriver", handle(binding), {
          source: scalar,
          expression_source: identity,
          target_component: component,
          target_offset: parameterOffset,
          inputs: contract.encodeExpressionDriverInputs([
            {
              name: "x",
              property: {
                component: client.components.Scalar!.id,
                offset: client.components.Scalar!.fields.value!.offset,
              },
            },
          ]),
        }),
      ]),
    );
    await until(
      () => host.datasets.driverStatus(client.session, binding),
      (status) => status.state === "Written",
      "driven parameter readiness",
    );
    page = await until(
      read,
      (page) =>
        JSON.stringify(output(page, "scaled")[0]) ===
        JSON.stringify(valid("f32", 32.5)),
      "driver before animation before projection",
    );
    equal(
      output(page, "scaled"),
      [valid("f32", 32.5), valid("f32", 42.5)],
      "fixed driver/animation/projection order",
    );
    await checkpoint("data.drivenAnimated", {
      drivenAnimated: page,
      driver: await host.datasets.driverStatus(client.session, binding),
    });
    await client.controlAnimationController(controller, { action: "stop" });
    successfulBatch(
      await client.batch([
        {
          kind: "removeComponent",
          entity: handle(binding),
          component: client.components.ExpressionDriver!.id,
        },
      ]),
    );

    // Multiple Worlds retain one shared stream through separate, intersected windows.
    const streamName = "datasets://combined/stream";
    const stream = await host.datasets.create(streamName, "streaming", [
      { name: "scalar", kind: "f32" },
    ]);
    producers.push(stream);
    const secondWorld = await host.createWorld({
      selectedSystems: DATA,
      symbolicId: "combined-retention",
    });
    worlds.push(secondWorld.reference);
    const other = (await host.openWorld(
      secondWorld.reference,
    )) as AnimationWorldClient;
    clients.push(other);
    const makeStream = async (
      client: AnimationWorldClient,
      symbol: string,
      count: bigint,
    ) => {
      const binding = await create(
        client,
        symbol,
        "StreamingDataSourceBinding",
        {
          source: streamName,
          windows: contract.encodeDataWindows([
            { kind: "count", count },
            {
              kind: "range",
              column: "scalar",
              width: Number(count - 1n),
              anchor: { kind: "latest" },
            },
          ]),
        },
      );
      // Connection-scoped immutable asset references may be shared across Worlds.
      const id = new contract.ExpressionBuilder();
      const source = await upload(
        client,
        19,
        assetId++,
        id.encode(id.input("column:scalar", "f32")),
      );
      successfulBatch(
        await client.batch([
          property(client, binding, "StreamingDataSourceBinding", "value", {
            kind: "asset",
            value: { kind: 19, source },
          }),
        ]),
      );
      await until(
        () => host.datasets.bindingView(client.session, binding),
        (page) => page.availability.reason === "Ready",
        "stream binding readiness",
      );
      return binding;
    };
    const short = await makeStream(client, "short-window", 2n),
      long = await makeStream(other, "long-window", 4n);
    await host.datasets.update(stream, [
      {
        operation: "append",
        rows: [1, 2, 3, 4, 5].map((value) => [{ kind: "f32" as const, value }]),
      },
    ]);
    await until(
      () => host.datasets.bindingView(other.session, long),
      (page) => page.rows.length === 4,
      "shared window union",
    );
    equal(
      (await host.datasets.read(streamName)).rows.map((row) => row.id),
      [2n, 3n, 4n, 5n],
      "source union retained larger window",
    );
    equal(
      (await host.datasets.bindingView(client.session, short)).rows.map(
        (row) => row.id,
      ),
      [4n, 5n],
      "World-local smaller window",
    );
    await other.close();
    await host.destroyWorld(secondWorld.reference);
    worlds.pop();
    await until(
      () => host.datasets.read(streamName),
      (page) => page.rows.length === 2,
      "detach expiry",
    );
    equal(
      (await host.datasets.read(streamName)).rows.map((row) => row.id),
      [4n, 5n],
      "detached World released shared history",
    );

    await set(client, short, "StreamingDataSourceBinding", {
      windows: contract.encodeDataWindows([
        { kind: "count", count: 4n },
        {
          kind: "range",
          column: "scalar",
          width: 3,
          anchor: { kind: "supplied", value: { kind: "f32", value: 7 } },
        },
      ]),
    });
    equal(
      (await host.datasets.read(streamName)).rows.map((row) => row.id),
      [4n, 5n],
      "widening cannot resurrect expired history",
    );
    await host.datasets.update(stream, [
      {
        operation: "append",
        rows: [6, 7].map((value) => [{ kind: "f32" as const, value }]),
      },
    ]);
    const retained = await until(
      () => host.datasets.bindingView(client.session, short),
      (page) => page.rows.length === 4,
      "widened future arrivals",
    );
    equal(
      retained.rows.map((row) => row.id),
      [4n, 5n, 6n, 7n],
      "widened windows retained only surviving history and arrivals",
    );
    await checkpoint("data.retention", {
      retained,
      memory: (await host.datasets.read(streamName)).memory,
    });

    // Recreate the source under one stable name: old incarnation never retargets queued work.
    await host.datasets.destroy(producer);
    await until(
      read,
      (page) => page.availability.reason === "Source",
      "source destruction",
    );
    const wrongKind = await host.datasets.create(
      source,
      "streaming",
      datasetSchema,
    );
    producers.push(wrongKind);
    await until(
      read,
      (page) =>
        page.availability.reason === "Source" &&
        page.sourceIncarnation === wrongKind.incarnation,
      "kind mismatch",
    );
    await host.datasets.destroy(wrongKind);
    const wrongSchema = await host.datasets.create(source, "buffer", [
      { name: "scalar", kind: "u32" },
    ]);
    producers.push(wrongSchema);
    await until(
      read,
      (page) =>
        page.availability.reason === "InputType" ||
        page.availability.reason === "MissingInput",
      "schema mismatch",
    );
    await host.datasets.destroy(wrongSchema);
    const typeMismatch = await host.datasets.create(
      source,
      "buffer",
      datasetSchema.map((column) => ({
        ...column,
        kind: column.name === "scalar" ? "u32" : column.kind,
      })),
    );
    producers.push(typeMismatch);
    await until(
      read,
      (page) =>
        page.availability.reason === "InputType" &&
        page.availability.input === "column:scalar",
      "exact schema type mismatch",
    );
    await host.datasets.destroy(typeMismatch);
    const recovered = await host.datasets.create(
      source,
      "buffer",
      datasetSchema,
    );
    producers.push(recovered);
    await host.datasets.update(recovered, [
      { operation: "append", rows: [datasetRow(10)] },
    ]);
    page = await until(
      read,
      (page) => page.availability.reason === "Ready" && page.rows.length === 1,
      "compatible source recovery",
    );
    check(
      page.sourceIncarnation !== producer.incarnation,
      "source replacement reused incarnation",
    );
    await checkpoint("data.sourceRecovery", { recovered: page });

    await expressionDrivers(
      host,
      client,
      contract,
      scalar,
      identity,
      assetId,
      checkpoint,
    );
    // Metadata snapshot deliberately excludes source rows, expression payloads and prepared output.
    snapshot = await host.saveWorld(client.session);
    await checkpoint("data.snapshot", { snapshotBytes: snapshot.length });
    return { artifacts, snapshot, source, systems: DATA };
  } finally {
    await Promise.allSettled(clients.map((client) => client.close()));
    for (const reference of worlds.reverse())
      await host.destroyWorld(reference).catch(() => {});
    await Promise.allSettled(
      producers.map((producer) =>
        host.datasets.destroy(producer).catch(() => {}),
      ),
    );
  }
}

async function expressionDrivers(
  host: DatasetHost,
  client: AnimationWorldClient,
  contract: Contract,
  scalar: bigint,
  identity: string,
  assetId: bigint,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const target = await create(client, "expression-target", "Scalar", {
    value: 99,
  });
  const component = client.components.Scalar!;
  const driver = (entity: bigint, source: bigint, expression: string) =>
    insertComponent(client, "ExpressionDriver", handle(entity), {
      source,
      expression_source: expression,
      target_component: component.id,
      target_offset: component.fields.value!.offset,
      inputs: contract.encodeExpressionDriverInputs([
        {
          name: "x",
          property: {
            component: component.id,
            offset: component.fields.value!.offset,
          },
        },
      ]),
    });
  successfulBatch(await client.batch([driver(target, scalar, identity)]));
  const status = () => host.datasets.driverStatus(client.session, target);
  await until(
    status,
    (status) => status.state === "Written",
    "identity driver",
  );
  const value = async () =>
    (await client.inspect()).entities
      .find((entity) => entity.id === target)!
      .components.find((item) => item.component === component.id)!.fields.value;
  equal(await value(), 3, "identity driver result");
  await record("data.driver", {
    phase: "identity driver result",
    status: await status(),
    value: await value(),
  });
  const divide = new contract.ExpressionBuilder();
  const division = await upload(
    client,
    19,
    assetId++,
    divide.encode(
      divide.binary(
        "divide",
        divide.input("x", "f32"),
        divide.constant({ kind: "f32", value: 0 }),
      ),
    ),
  );
  await set(client, target, "ExpressionDriver", {
    expression_source: division,
  });
  await until(
    status,
    (status) => status.reason === "Calculation",
    "invalid driver calculation",
  );
  equal(await value(), 3, "invalid driver retained stored target");
  await record("data.driver", {
    phase: "invalid driver retained stored target",
    status: await status(),
    value: await value(),
  });
  const fallback = new contract.ExpressionBuilder();
  const input = fallback.input("x", "f32");
  const rescued = await upload(
    client,
    19,
    assetId++,
    fallback.encode(
      fallback.fallback(input, fallback.constant({ kind: "f32", value: 7 })),
    ),
  );
  await set(client, target, "ExpressionDriver", { expression_source: rescued });
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: handle(scalar),
        component: component.id,
      },
    ]),
  );
  await until(
    status,
    (status) => status.state === "Written",
    "missing driver input fallback",
  );
  equal(await value(), 7, "fallback driver independent result");
  await record("data.driver", {
    phase: "fallback driver independent result",
    status: await status(),
    value: await value(),
  });
  successfulBatch(
    await client.batch([
      insertComponent(client, "Scalar", handle(scalar), { value: 4 }),
    ]),
  );
  await set(client, target, "ExpressionDriver", {
    expression_source: identity,
  });
  await until(
    status,
    (status) => status.state === "Written" && status.reason === "",
    "driver lifecycle recovery",
  );
  equal(await value(), 4, "driver restored input component");
  await record("data.driver", {
    phase: "driver restored input component",
    status: await status(),
    value: await value(),
  });
  successfulBatch(await client.batch([driver(scalar, target, identity)]));
  await until(
    status,
    (status) => status.reason === "Cycle",
    "expression dependency cycle",
  );
  equal(await value(), 4, "cycle retained target");
  await record("data.driver", {
    phase: "cycle retained target",
    status: await status(),
    value: await value(),
  });
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: handle(scalar),
        component: client.components.ExpressionDriver!.id,
      },
    ]),
  );
  await until(
    status,
    (status) => status.state === "Written",
    "cycle correction",
  );
  successfulBatch(
    await client.batch([{ kind: "delete", entity: handle(scalar) }]),
  );
  await set(client, target, "ExpressionDriver", {
    expression_source: identity,
  });
  await until(
    status,
    (status) => status.reason === "MissingInput",
    "deleted driver source",
  );
  equal(await value(), 4, "deleted source retained destination");
  await record("data.driver", {
    phase: "deleted source retained destination",
    status: await status(),
    value: await value(),
  });
  // Persist only live typed entity references; stale references correctly refuse export.
  const replacement = await create(client, "persisted-input", "Scalar", {
    value: 11,
  });
  await set(client, target, "ExpressionDriver", { source: replacement });
  await until(
    status,
    (status) => status.state === "Written",
    "explicit driver reauthoring after source deletion",
  );
  equal(await value(), 11, "reauthored source result");
  await record("data.driver", {
    phase: "reauthored source result",
    status: await status(),
    value: await value(),
  });
}

/** A truly fresh Host lacks all dataset and expression payloads; restore cannot silently recover them. */
export async function restoreDataAuthoring(
  host: DatasetHost,
  contract: Contract,
  saved: Awaited<ReturnType<typeof dataAuthoring>>,
) {
  const world = await host.loadWorld(saved.snapshot!, {
    symbolicId: "combined-restored",
  });
  const client = (await host.openWorld(world.root)) as AnimationWorldClient;
  let producer:
    | Awaited<ReturnType<DatasetHost["datasets"]["create"]>>
    | undefined;
  try {
    const inspection = await client.inspect();
    const binding = inspection.entities.find(
      (entity) => entity.metadata.symbolicId === "projection",
    )!;
    const page = await host.datasets.bindingView(client.session, binding.id);
    check(
      page.availability.reason === "Source" && page.rows.length === 0,
      "fresh Host restored source payload or prepared view",
    );
    const component = client.components.BufferDataSourceBinding!.id;
    const config = binding.components.find(
      (item) => item.component === component,
    )!;
    equal(config.fields.source, saved.source, "metadata restored source name");
    check(
      config.properties?.scaled?.kind === "asset",
      "metadata lost expression asset declaration",
    );
    producer = await host.datasets.create(
      saved.source,
      "buffer",
      datasetSchema,
    );
    await host.datasets.update(producer, [
      { operation: "append", rows: [datasetRow(12)] },
    ]);
    await until(
      () => host.datasets.bindingView(client.session, binding.id),
      (page) => page.availability.reason === "MissingAsset",
      "fresh Host requires expression resupply",
    );
    // Replace references with fresh immutable client assets through ordinary authoring.
    const builder = new contract.ExpressionBuilder();
    const expression = await upload(
      client,
      19,
      990n,
      builder.encode(builder.input("column:scalar", "f32")),
    );
    const operations: Command[] = Object.keys(config.properties!)
      .filter((name) => !name.endsWith("_parameter"))
      .map((name) => ({
        kind: "removeDynamicProperty",
        entity: handle(binding.id),
        component,
        name,
      }));
    operations.push(
      property(client, binding.id, "BufferDataSourceBinding", "resupplied", {
        kind: "asset",
        value: { kind: 19, source: expression },
      }),
    );
    successfulBatch(await client.batch(operations));
    const restored = await until(
      () => host.datasets.bindingView(client.session, binding.id),
      (page) => page.availability.reason === "Ready",
      "fresh Host client resupply",
    );
    equal(
      output(restored, "resupplied"),
      [valid("f32", 12.5)],
      "fresh Host recovered projection",
    );
    const driver = inspection.entities.find(
      (entity) => entity.metadata.symbolicId === "expression-target",
    )!;
    const driverStatus = await host.datasets.driverStatus(
      client.session,
      driver.id,
    );
    check(
      driverStatus.state === "Retained" &&
        ["AssetUnavailable", "AssetFailed"].includes(driverStatus.reason),
      "fresh Host restored driver payload",
    );
    const driverExpression = new contract.ExpressionBuilder();
    const driverAsset = await upload(
      client,
      19,
      991n,
      driverExpression.encode(driverExpression.input("x", "f32")),
    );
    await set(client, driver.id, "ExpressionDriver", {
      expression_source: driverAsset,
    });
    const recoveredDriver = await until(
      () => host.datasets.driverStatus(client.session, driver.id),
      (status) => status.state === "Written",
      "fresh Host driver resupply",
    );
    const recoveredState = await client.inspect();
    equal(
      recoveredState.entities
        .find((entity) => entity.id === driver.id)!
        .components.find(
          (component) => component.component === client.components.Scalar!.id,
        )!.fields.value,
      11,
      "fresh Host driver remapped source result",
    );
    const streamBinding = inspection.entities.find(
      (entity) => entity.metadata.symbolicId === "short-window",
    )!;
    equal(
      [
        ...(streamBinding.components.find(
          (component) =>
            component.component ===
            client.components.StreamingDataSourceBinding!.id,
        )!.fields.windows as Uint8Array),
      ],
      [
        ...contract.encodeDataWindows([
          { kind: "count", count: 4n },
          {
            kind: "range",
            column: "scalar",
            width: 3,
            anchor: { kind: "supplied", value: { kind: "f32", value: 7 } },
          },
        ]),
      ],
      "metadata preserved windows",
    );
    return {
      unavailable: page,
      resupplied: restored,
      driverStatus,
      recoveredDriver,
      snapshotBytes: saved.snapshot!.length,
    };
  } finally {
    await client.close();
    await host.destroyWorld(world.root);
    if (producer) await host.datasets.destroy(producer);
  }
}
