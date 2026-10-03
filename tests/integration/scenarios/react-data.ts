import { createElement as h } from "react";
import type {
  AnimationWorldClient,
  DataBindingPage,
  DatasetValue,
} from "../../../packages/ipp-client/src/index.js";
import {
  createRoot,
  DataSource,
  Entity,
  BufferDataSourceBinding,
  StreamingDataSourceBinding,
  ColumnBindingAsset,
  assetRef,
} from "../../../packages/ipp-react/src/index.js";
import type { DatasetHost } from "./datasets.js";
import { check } from "./datasets.js";
import type * as Generated from "../../../target/integration-artifacts/client/generated.js";
import { ASSETS, selectSystems } from "../system-selections.js";

type Contract = Pick<
  typeof Generated,
  "ExpressionBuilder" | "encodeDataWindows"
>;
const schema = [{ name: "value", kind: "f32" as const }];
const row = (value: number): DatasetValue[] => [{ kind: "f32", value }];
function equal(actual: unknown, expected: unknown, label: string) {
  const json = (value: unknown) =>
    JSON.stringify(value, (_, value) =>
      typeof value === "bigint" ? `${value}n` : value,
    );
  check(
    json(actual) === json(expected),
    `${label}: ${json(actual)} != ${json(expected)}`,
  );
}
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
  throw new Error(`${label}: readiness timed out`);
}
function values(page: DataBindingPage, name: string): unknown[] {
  const index = page.columns.findIndex((column) => column.name === name);
  check(index >= 0, `missing ${name}`);
  return page.rows.map((row) => row.values[index]);
}
const valid = (value: number) => ({
  valid: true,
  value: { kind: "f32", value },
});

/** Same React declarations and independent expectations over every real Host. */
export async function reactData(
  host: DatasetHost,
  contract: Contract,
  record: (label: string, value: unknown) => Promise<void> = async () => {},
) {
  const world = await host.createWorld({
    selectedSystems: selectSystems(ASSETS, ["ipp.data-bindings"]),
    symbolicId: "react-data",
  });
  const client = (await host.openWorld(
    world.reference,
  )) as AnimationWorldClient;
  const root = createRoot(client);
  const source = "datasets://react/owned";
  const borrowed = "datasets://react/borrowed";
  const external = await host.datasets.create(borrowed, "buffer", schema);
  const stream = "datasets://react/stream";
  const builder = new contract.ExpressionBuilder();
  const input = builder.input("column:value", "f32");
  const parameter = builder.input("parameter", "f32");
  const scaled = builder.encode(
    builder.binary(
      "multiply",
      input,
      builder.fallback(parameter, builder.constant({ kind: "f32", value: 1 })),
    ),
  );
  const identityBuilder = new contract.ExpressionBuilder();
  const identity = identityBuilder.encode(
    identityBuilder.input("column:value", "f32"),
  );
  let registrations = 0;
  let batches = 0;
  const register = client.registerAsset.bind(client);
  const batch = client.batch.bind(client);
  client.registerAsset = (...args) => {
    registrations++;
    return register(...args);
  };
  client.batch = (...args) => {
    batches++;
    return batch(...args);
  };
  const states: string[] = [];
  root.onDataSourceChange((state) =>
    states.push(`${state.name}:${state.status}`),
  );
  const render = (
    a = 2,
    includeParameter: boolean | "omit" = true,
    definition = scaled,
    includeOwned = true,
    count = 2n,
    includeRaw = true,
  ) =>
    h(
      DataSource,
      { name: borrowed, ownership: "borrowed" },
      h(ColumnBindingAsset, { id: "formula", definition }),
      h(ColumnBindingAsset, { id: "identity", definition: identity }),
      includeOwned
        ? h(
            DataSource,
            {
              name: source,
              ownership: "producer",
              kind: "buffer",
              schema,
              datasets: host.datasets,
            },
            h(
              Entity,
              { id: "first" },
              h(BufferDataSourceBinding, {
                source,
                columns: {
                  scaled: {
                    definition: assetRef("formula"),
                    ...(includeParameter === "omit"
                      ? {}
                      : { parameter: includeParameter ? a : null }),
                  },
                  raw: includeRaw ? { definition: assetRef("identity") } : null,
                },
              }),
            ),
            h(
              Entity,
              { id: "second" },
              h(BufferDataSourceBinding, {
                source,
                columns: {
                  scaled: { definition: assetRef("formula"), parameter: 3 },
                },
              }),
            ),
          )
        : null,
      h(
        Entity,
        { id: "borrowed" },
        h(BufferDataSourceBinding, {
          source: borrowed,
          columns: { raw: { definition: assetRef("identity") } },
        }),
      ),
      h(
        DataSource,
        {
          name: stream,
          ownership: "producer",
          kind: "streaming",
          schema,
          datasets: host.datasets,
        },
        h(
          Entity,
          { id: "stream" },
          h(StreamingDataSourceBinding, {
            source: stream,
            columns: { raw: { definition: assetRef("identity") } },
            windows: [{ kind: "count", count }],
            encodeWindows: contract.encodeDataWindows,
          }),
        ),
      ),
    );
  const entity = async (symbolicId: string) => {
    const found = (await client.inspect()).entities.find(
      (entity) => entity.metadata.symbolicId === symbolicId,
    );
    check(found, `missing ${symbolicId}`);
    return found.id;
  };
  const ready = (id: bigint, count: number) =>
    until(
      () => host.datasets.bindingView(client.session, id),
      (page) =>
        page.availability.reason === "Ready" && page.rows.length === count,
      "React data binding",
    );
  try {
    const initial = render();
    equal(states, [], "render descriptions have no effects");
    await root.render(initial);
    const owned = root.getDataSource(source)?.handle;
    const streaming = root.getDataSource(stream)?.handle;
    check(owned && streaming, "missing producer readiness");
    await owned.update([{ operation: "append", rows: [row(2), row(5)] }]);
    await host.datasets.update(external, [
      { operation: "append", rows: [row(11)] },
    ]);
    await streaming.update([
      { operation: "append", rows: [row(1), row(2), row(3), row(4)] },
    ]);
    const first = await entity("first");
    const second = await entity("second");
    const streamEntity = await entity("stream");
    let page = await ready(first, 2);
    equal(values(page, "raw"), [valid(2), valid(5)], "identity projection");
    equal(values(page, "scaled"), [valid(4), valid(10)], "first parameter");
    equal(
      values(await ready(second, 2), "scaled"),
      [valid(6), valid(15)],
      "independent shared-source parameter",
    );
    equal(
      (await ready(streamEntity, 2)).rows.map((row) => row.id),
      [3n, 4n],
      "stream count window",
    );
    check(page.dirty, "headless initial dirty missing");
    check(
      (await host.datasets.bindingView(client.session, first)).dirty,
      "readonly query cleared dirty",
    );
    await root.flush();
    const initialRegistrations = registrations;
    const initialBatches = batches;
    await root.render(render());
    equal(
      registrations,
      initialRegistrations,
      "unchanged assets not reuploaded",
    );
    equal(batches, initialBatches, "unchanged components not recreated");
    equal(
      root.getDataSource(source)?.handle?.producer,
      owned.producer,
      "unchanged producer",
    );
    await root.render(render(4));
    page = await until(
      () => host.datasets.bindingView(client.session, first),
      (page) =>
        JSON.stringify(values(page, "scaled")) ===
        JSON.stringify([valid(8), valid(20)]),
      "parameter update",
    );
    equal(
      registrations,
      initialRegistrations,
      "parameter change did not upload definition",
    );
    await root.render(render(123, "omit"));
    equal(
      values(await ready(first, 2), "scaled"),
      [valid(8), valid(20)],
      "omitted parameter preserves last value",
    );
    await root.render(render(4, false));
    page = await until(
      () => host.datasets.bindingView(client.session, first),
      (page) =>
        JSON.stringify(values(page, "scaled")) ===
        JSON.stringify([valid(2), valid(5)]),
      "explicitly removed optional parameter fallback",
    );
    await root.render(render(4, false, scaled, true, 1n));
    equal(
      (await ready(streamEntity, 1)).rows.map((row) => row.id),
      [4n],
      "changed window expires history",
    );
    await root.render(render(4, false, scaled, true, 1n, false));
    await until(
      () => host.datasets.bindingView(client.session, first),
      (page) => page.columns.every((column) => column.name !== "raw"),
      "explicit output removal",
    );
    // A real failed decode preserves the previously ready immutable selection.
    await root.render(render(4, false, new Uint8Array([0])));
    await until(
      async () => {
        await root.flush();
        return root.getAsset("formula");
      },
      (asset) => asset?.status === "failed",
      "failed expression preparation",
    );
    equal(
      values(await ready(first, 2), "scaled"),
      [valid(2), valid(5)],
      "failed replacement preserved current",
    );
    // Superseded committed preparation is fenced through the existing scheduler.
    const corrected = root.render(render(4, false, identity));
    const latest = root.render(render(6, true, scaled));
    await Promise.all([corrected, latest]);
    await until(
      () => host.datasets.bindingView(client.session, first),
      (page) =>
        JSON.stringify(values(page, "scaled")) ===
        JSON.stringify([valid(12), valid(30)]),
      "superseded replacement",
    );
    await root.render(render(6, true, scaled, false));
    let staleRejected = false;
    try {
      await owned.update([{ operation: "append", rows: [row(99)] }]);
    } catch {
      staleRejected = true;
    }
    check(staleRejected, "removed producer handle was not fenced");
    equal(
      (await host.datasets.read(borrowed)).rows.map((row) => row.values),
      [row(11)],
      "borrowed source survived declaration removal",
    );
    await root.render(render());
    const replacement = root.getDataSource(source)?.handle;
    check(
      replacement &&
        replacement.producer.incarnation !== owned.producer.incarnation,
      "fresh producer incarnation missing",
    );
    await replacement.update([{ operation: "append", rows: [row(7)] }]);
    const replacementFirst = await entity("first");
    equal(
      values(await ready(replacementFirst, 1), "scaled"),
      [valid(14)],
      "replacement source projection",
    );
    await record("react-data.observations", {
      states,
      registrations,
      batches,
      incarnation: String(replacement.producer.incarnation),
      projected: values(await ready(replacementFirst, 1), "scaled"),
    });
    const retainedFormula = root.getAsset("formula")?.current;
    check(retainedFormula, "ready formula resource missing");
    await root.unmount();
    equal(
      (await host.datasets.read(source)).incarnation,
      replacement.producer.incarnation,
      "root unmount preserves source",
    );
    check(
      (await client.inspect()).entities.some(
        (entity) => entity.id === replacementFirst,
      ),
      "root unmount removed entity",
    );
    await host.datasets.update(replacement.producer, [
      { operation: "append", rows: [row(8)] },
    ]);
    equal(
      (await host.datasets.read(source)).rows.map((row) => row.values),
      [row(7), row(8)],
      "producer ownership survived root unmount",
    );
    // A new authoring session adopts the retained entity using fresh handles.
    await client.close();
    let staleSessionRejected = false;
    try {
      await replacement.update([{ operation: "append", rows: [row(99)] }]);
    } catch {
      staleSessionRejected = true;
    }
    check(
      staleSessionRejected,
      "closed-session producer helper remained active",
    );
    const reconnect = (await host.openWorld(
      world.reference,
    )) as AnimationWorldClient;
    const reconnectRoot = createRoot(reconnect);
    try {
      await reconnectRoot.render(
        h(
          DataSource,
          { name: source, ownership: "borrowed" },
          h(
            Entity,
            { id: "first" },
            h(BufferDataSourceBinding, {
              source,
              columns: {
                scaled: { definition: retainedFormula, parameter: 5 },
              },
            }),
          ),
        ),
      );
      const adopted = (await reconnect.inspect()).entities.find(
        (entity) => entity.metadata.symbolicId === "first",
      );
      check(
        adopted?.id === replacementFirst,
        "reconnect did not adopt retained entity",
      );
      const observed = await until(
        () => host.datasets.bindingView(reconnect.session, adopted.id),
        (page) =>
          JSON.stringify(values(page, "scaled")) ===
          JSON.stringify([valid(35), valid(40)]),
        "reconnected borrowed binding",
      );
      check(observed.dirty, "reconnect readonly observation cleared dirty");
      await reconnectRoot.render(null);
      equal(
        (await host.datasets.read(source)).rows.map((row) => row.values),
        [row(7), row(8)],
        "borrowed declaration removal preserved producer source",
      );
    } finally {
      await reconnectRoot.unmount();
      await reconnect.close();
    }
    await host.datasets.destroy(replacement.producer);
    await host.datasets.destroy(streaming.producer);
    return {
      registrations,
      batches,
      borrowedSurvived: true,
      rootUnmountPreserved: true,
    };
  } finally {
    await root.unmount();
    await host.datasets.destroy(external);
    await client.close();
    await host.destroyWorld(world.reference);
  }
}
