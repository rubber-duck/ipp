import type {
  AnimationWorldClient,
  DataBindingPage,
  DatasetValue,
} from "../../../packages/ipp-client/src/index.js";
import type * as Generated from "../../../target/integration-artifacts/client/generated.js";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import { ANIMATION, ASSETS, selectSystems } from "../system-selections.js";
import { check, type DatasetHost } from "./datasets.js";

const row = (value: number): DatasetValue[] => [{ kind: "f32", value }];

function scalar(page: DataBindingPage, id: bigint) {
  const index = page.columns.findIndex((column) => column.name === "displayed");
  const result = page.rows.find((row) => row.id === id)?.values[index];
  check(
    result?.valid && result.value.kind === "f32",
    `Missing scalar row ${id}`,
  );
  return result.value.value;
}

async function until<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  label: string,
): Promise<T> {
  const deadline = performance.now() + 10_000;
  do {
    const value = await read();
    if (ready(value)) return value;
  } while (performance.now() < deadline);
  throw new Error(`${label} timed out`);
}

/** Real clocks and typed observations; the same scenario runs over every dataset transport. */
export async function dataInterpolation(
  host: DatasetHost,
  contract: Pick<typeof Generated, "ExpressionBuilder" | "encodeAnimationClip">,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const world = await host.createWorld({
    selectedSystems: selectSystems(ASSETS, ANIMATION, ["ipp.data-bindings"]),
    symbolicId: "data-interpolation",
  });
  const client = (await host.openWorld(
    world.reference,
  )) as AnimationWorldClient;
  const source = "datasets://interpolation/rate";
  const producer = await host.datasets.create(source, "buffer", [
    { name: "raw", kind: "f32" },
  ]);
  let replacement: typeof producer | undefined;
  const clocks = new Map<bigint, number>();
  let collectingClocks = true;
  let clockFailure: unknown;
  const clockCollector = (async () => {
    let tick = 0n;
    while (collectingClocks) {
      const frame = await client.waitForFrame(tick);
      tick = frame.tick;
      clocks.set(tick, frame.time);
      if (clocks.size > 512) clocks.delete(clocks.keys().next().value!);
    }
  })().catch((error: unknown) => {
    if (collectingClocks) clockFailure = error;
  });
  try {
    await host.datasets.update(producer, [
      { operation: "append", rows: [row(2)] },
    ]);
    const builder = new contract.ExpressionBuilder();
    const definition = builder.encode(builder.input("column:raw", "f32"));
    const asset = clientAssetSource(client.session, 19, 1n);
    await client.registerAsset(asset, definition.buffer);
    const entity = aliasId(
      await client.batch([
        createEntity(1, "interpolation"),
        insertComponent(
          client,
          "BufferDataSourceBinding",
          { kind: "alias", alias: 1 },
          { source },
        ),
        {
          kind: "setDynamicProperty",
          entity: { kind: "alias", alias: 1 },
          component: client.components.BufferDataSourceBinding!.id,
          name: "displayed",
          value: { kind: "asset", value: { kind: 19, source: asset.source } },
        },
        {
          kind: "setDynamicProperty",
          entity: { kind: "alias", alias: 1 },
          component: client.components.BufferDataSourceBinding!.id,
          name: "displayed_interp",
          value: { kind: "f32", value: 8 },
        },
      ]),
      1,
    );
    const read = () => host.datasets.bindingView(client.session, entity);
    const initial = await until(
      read,
      (page) => page.availability.reason === "Ready" && page.rows.length === 1,
      "initial interpolation",
    );
    check(
      scalar(initial, 1n) === 2,
      "New interpolation row did not initialize immediately",
    );

    const timed = async () => {
      const result = await until(
        async () => {
          if (clockFailure) throw clockFailure;
          const page = await read();
          const time =
            page.evaluatedTick === null
              ? undefined
              : clocks.get(page.evaluatedTick);
          return { page, time };
        },
        ({ time }) => time !== undefined,
        "completed evaluated binding clock",
      );
      return {
        page: result.page,
        clock: { tick: result.page.evaluatedTick!, time: result.time! },
      };
    };
    await host.datasets.update(producer, [
      { operation: "edit", row: 1n, values: row(12) },
    ]);
    await client.waitForFrame();
    const first = await timed();
    check(
      scalar(first.page, 1n) > 2 && scalar(first.page, 1n) < 12,
      "Interpolation did not expose an intermediate value",
    );
    let frame = await client.waitForFrame(first.clock.tick);
    while (frame.time - first.clock.time < 0.15)
      frame = await client.waitForFrame(frame.tick);
    const second = await timed();
    const expected =
      scalar(first.page, 1n) + 8 * (second.clock.time - first.clock.time);
    check(
      Math.abs(scalar(second.page, 1n) - expected) < 0.0001,
      "Displayed rate disagrees with completed Host clock",
    );
    const raw = await host.datasets.read(source);
    check(
      raw.rows[0]!.values[0]!.value === 12,
      "Interpolation changed raw source data",
    );
    await record("data.interpolation.rate", { first, second, expected, raw });

    const component = client.components.BufferDataSourceBinding!.id;
    const clip = contract.encodeAnimationClip({
      duration: 2,
      tracks: [
        {
          property: { component, name: "displayed_interp" },
          keys: [
            {
              time: 0,
              value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
              interpolation: { kind: "linear" },
            },
            {
              time: 2,
              value: { kind: "dynamic", value: { kind: "f32", value: 8 } },
            },
          ],
        },
      ],
    });
    const clipAsset = clientAssetSource(client.session, 10, 2n);
    await client.registerAsset(clipAsset, clip.buffer);
    const controller = await client.createAnimationController({
      speed: 0,
      drivers: [
        {
          source: clipAsset.source,
          track: 0,
          target: entity,
          property: { component, name: "displayed_interp" },
        },
      ],
    });
    await client.controlAnimationController(controller, { action: "play" });
    await until(
      () => client.inspect(),
      (inspection) =>
        inspection.controllers?.some(
          (value) => value.id === controller && value.state === "playing",
        ) ?? false,
      "interpolation speed controller",
    );
    await client.controlAnimationController(controller, { action: "pause" });
    await client.controlAnimationController(controller, {
      action: "seek",
      time: 1,
    });
    await until(
      () => client.inspectPage({ collection: "entities" }),
      (inspection) =>
        inspection.entities
          .find((value) => value.id === entity)
          ?.components.find((value) => value.component === component)
          ?.properties?.displayed_interp?.value === 12,
      "animated interpolation speed",
    );

    await host.datasets.update(producer, [
      { operation: "edit", row: 1n, values: row(-4) },
    ]);
    await client.waitForFrame();
    const retargeted = await timed();
    check(
      scalar(retargeted.page, 1n) > -4 && scalar(retargeted.page, 1n) < 12,
      "Retarget did not start from displayed value",
    );
    frame = await client.waitForFrame(retargeted.clock.tick);
    while (frame.time - retargeted.clock.time < 0.15)
      frame = await client.waitForFrame(frame.tick);
    const faster = await timed();
    const fasterExpected = Math.max(
      -4,
      scalar(retargeted.page, 1n) -
        12 * (faster.clock.time - retargeted.clock.time),
    );
    check(
      Math.abs(scalar(faster.page, 1n) - fasterExpected) < 0.0001,
      "Animated interpolation speed did not use typed numeric update",
    );
    await record("data.interpolation.animatedSpeed", {
      retargeted,
      faster,
      fasterExpected,
    });
    await host.datasets.update(producer, [
      { operation: "insert", index: 0n, rows: [row(100)] },
    ]);
    const inserted = await until(
      read,
      (page) => page.rows.length === 2 && page.rows[0]!.id === 2n,
      "interpolated row insertion",
    );
    check(
      scalar(inserted, 2n) === 100 && scalar(inserted, 1n) < 12,
      "Insertion interpolated another row identity",
    );
    const reached = await until(
      read,
      (page) => scalar(page, 1n) === -4,
      "interpolation settlement",
    );
    // A paused rate controller remains a numeric writer. Release that writer
    // before measuring interpolation's own unchanged-input idle behavior.
    await client.controlAnimationController(controller, { action: "stop" });
    await client.waitForFrame();
    const settled = await read();
    check(
      scalar(settled, 1n) === -4,
      "Stopping the rate controller changed the settled projection",
    );
    const nextFrame = await client.waitForFrame(settled.evaluatedTick!);
    await client.waitForFrame(nextFrame.tick);
    const idle = await read();
    check(
      idle.evaluatedTick === settled.evaluatedTick,
      "Settled interpolation continued evaluating",
    );
    check(idle.dirty, "Read-only interpolation query cleared dirty");
    await record("data.interpolation.retarget", {
      retargeted,
      inserted,
      reached,
      settled,
      idle,
    });

    await host.datasets.destroy(producer);
    replacement = await host.datasets.create(source, "buffer", [
      { name: "raw", kind: "f32" },
    ]);
    await host.datasets.update(replacement, [
      { operation: "append", rows: [row(70)] },
    ]);
    const replaced = await until(
      read,
      (page) =>
        page.sourceIncarnation === replacement!.incarnation &&
        page.rows.length === 1,
      "interpolation source replacement",
    );
    check(
      scalar(replaced, 1n) === 70,
      "Replacement source borrowed old row motion",
    );
    successfulBatch(
      await client.batch([
        {
          kind: "removeDynamicProperty",
          entity: { kind: "handle", id: entity },
          component: client.components.BufferDataSourceBinding!.id,
          name: "displayed_interp",
        },
      ]),
    );
    await host.datasets.update(replacement, [
      { operation: "edit", row: 1n, values: row(-70) },
    ]);
    const disabled = await until(
      read,
      (page) => scalar(page, 1n) === -70,
      "disabled interpolation",
    );
    await record("data.interpolation.lifetime", { replaced, disabled });

    const property = (name: string, value: number) => ({
      kind: "setDynamicProperty" as const,
      entity: { kind: "handle" as const, id: entity },
      component,
      name,
      value: { kind: "f32" as const, value },
    });
    successfulBatch(
      await client.batch([
        property("displayed_interp_reference", 16),
        property("displayed_interp_percent", 25),
      ]),
    );
    await host.datasets.update(replacement, [
      { operation: "edit", row: 1n, values: row(-50) },
    ]);
    await client.waitForFrame();
    const percentageFirst = await timed();
    check(
      scalar(percentageFirst.page, 1n) > -70 &&
        scalar(percentageFirst.page, 1n) < -50,
      "Percentage did not expose intermediate output",
    );
    frame = await client.waitForFrame(percentageFirst.clock.tick);
    while (frame.time - percentageFirst.clock.time < 0.15)
      frame = await client.waitForFrame(frame.tick);
    const percentageSecond = await timed();
    const percentageExpected =
      scalar(percentageFirst.page, 1n) +
      4 * (percentageSecond.clock.time - percentageFirst.clock.time);
    check(
      Math.abs(scalar(percentageSecond.page, 1n) - percentageExpected) < 0.0001,
      "Explicit percentage reference rate disagrees with Host clocks",
    );

    successfulBatch(
      await client.batch([property("displayed_interp_reference", 0)]),
    );
    const held = await timed();
    frame = await client.waitForFrame(held.clock.tick);
    while (frame.time - held.clock.time < 0.15)
      frame = await client.waitForFrame(frame.tick);
    const stillHeld = await timed();
    check(
      scalar(stillHeld.page, 1n) === scalar(held.page, 1n),
      "Zero percentage reference did not hold the display",
    );
    successfulBatch(
      await client.batch([property("displayed_interp_reference", 32)]),
    );
    const resumed = await timed();
    frame = await client.waitForFrame(resumed.clock.tick);
    while (frame.time - resumed.clock.time < 0.15)
      frame = await client.waitForFrame(frame.tick);
    const resumedLater = await timed();
    const resumedExpected =
      scalar(resumed.page, 1n) +
      8 * (resumedLater.clock.time - resumed.clock.time);
    check(
      Math.abs(scalar(resumedLater.page, 1n) - resumedExpected) < 0.0001,
      "Changed percentage reference did not use its new rate",
    );
    await record("data.interpolation.percentage", {
      percentageFirst,
      percentageSecond,
      percentageExpected,
      held,
      stillHeld,
      resumed,
      resumedLater,
      resumedExpected,
    });
  } finally {
    collectingClocks = false;
    await client.close();
    await clockCollector;
    await host.destroyWorld(world.reference);
    await host.datasets.destroy(replacement ?? producer).catch(() => {});
  }
}
