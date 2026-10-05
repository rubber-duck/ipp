/** Actual fitted percentage motion, completed pixels and source-row picks on both renderers. */
import {
  canvasOutput,
  clientAssetSource,
  type AssetWorldClient,
  type Client,
  type Command,
  type DatasetProducer,
  type DatasetValue,
  type HostClientBase,
  type PickingWorldClient,
  type RootBinding,
  type RowPropertyValue,
} from "@ipp/client";
import type { DataBindingPage } from "../../../packages/ipp-client/src/datasets.js";
import type * as Generated from "@ipp/host-contract";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import type { PlotCapture, PlotFrame } from "../plot-capture.js";

type Contract = Pick<typeof Generated, "ExpressionBuilder" | "encodeRowsTable">;
type Timed = { page: DataBindingPage; tick: bigint; time: number };
const extent = { width: 1400, height: 1000, devicePixelRatio: 1 };
const positions = [60, 760] as const;
const cyan = [0, 0.8, 1, 1] as const;
const percent = 25;

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function until<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  label: string,
): Promise<T> {
  const deadline = performance.now() + 15_000;
  do {
    const value = await read();
    if (ready(value)) return value;
  } while (performance.now() < deadline);
  throw new Error(`Percentage Plot timed out: ${label}`);
}

function scalar(page: DataBindingPage, name: string, row: number): number {
  const index = page.columns.findIndex((column) => column.name === name);
  const value = page.rows[row]?.values[index];
  check(
    value?.valid && value.value.kind === "f32",
    `Missing percentage scalar ${name}/${row}`,
  );
  return value.value.value;
}

function scale(page: DataBindingPage): number {
  // Bars include zero; these two complete, visible samples are nondegenerate.
  return Math.max(
    Math.abs(scalar(page, "temperature", 0)),
    Math.abs(scalar(page, "temperature", 1)),
  );
}

function row(
  position: number,
  temperature: number,
  offset = 1,
): DatasetValue[] {
  return [position, temperature, offset].map((value) => ({
    kind: "f32",
    value,
  }));
}

function series(ambiguous = false): Readonly<Record<string, RowPropertyValue>> {
  return {
    name: "Measured signal",
    x: ambiguous ? "temperature" : "position",
    y: "temperature",
    z: "",
    value: "",
    radius: "",
    height: "",
    color_column: "",
    color: cyan,
    visible: true,
  };
}

function cyanAt(frame: PlotFrame, x: number, y: number): number {
  let count = 0;
  for (let dy = -3; dy <= 3; dy++)
    for (let dx = -3; dx <= 3; dx++) {
      const index =
        (Math.round(y) + dy) * frame.width * 4 + (Math.round(x) + dx) * 4;
      if (
        frame.pixels[index]! < 110 &&
        frame.pixels[index + 1]! > 170 &&
        frame.pixels[index + 2]! > 190
      )
        count++;
    }
  return count;
}

function cyanPanel(frame: PlotFrame, panel: number): number {
  let count = 0;
  for (let y = 150; y < 570; y++)
    for (let x = positions[panel]! + 40; x < positions[panel]! + 580; x++) {
      const index = (y * frame.width + x) * 4;
      if (
        frame.pixels[index]! < 110 &&
        frame.pixels[index + 1]! > 170 &&
        frame.pixels[index + 2]! > 190
      )
        count++;
    }
  return count;
}

function rateBounds(first: Timed, second: Timed) {
  const elapsed = second.time - first.time;
  const before = scale(first.page),
    after = scale(second.page);
  const displacement = Math.abs(
    scalar(second.page, "temperature", 1) -
      scalar(first.page, "temperature", 1),
  );
  // The fitted magnitude grows monotonically between these observations. Every
  // unseen Host step therefore uses a reference inside these independent bounds.
  return {
    elapsed,
    before,
    after,
    displacement,
    lower: (percent / 100) * before * elapsed,
    upper: (percent / 100) * after * elapsed,
  };
}

/** Launch and transport stay in the existing Plot runners; this fixture owns no clock. */
export async function exercisePlotPercentage(
  host: HostClientBase<Client>,
  contract: Contract,
  font: Uint8Array<ArrayBuffer>,
  capture: PlotCapture,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const world = await host.createWorld({
    symbolicId: "plot-percentage",
    temporary: true,
    selectedSystems: [
      "ipp.asset-dependencies",
      "ipp.data-bindings",
      "ipp.plot",
      "ipp.canvas",
    ],
  });
  const client = (await host.openWorld(world.reference)) as AssetWorldClient;
  const producers: DatasetProducer[] = [],
    entities: bigint[] = [];
  let binding: RootBinding | undefined;
  const clocks = new Map<bigint, number>();
  let collecting = true,
    clockFailure: unknown;
  const collector = (async () => {
    let tick = 0n;
    while (collecting) {
      const frame = await client.waitForFrame(tick);
      tick = frame.tick;
      clocks.set(tick, frame.time);
      if (clocks.size > 1024) clocks.delete(clocks.keys().next().value!);
    }
  })().catch((error: unknown) => {
    if (collecting) clockFailure = error;
  });
  try {
    const fontAsset = clientAssetSource(client.session, 17, 1n);
    await client.registerAsset(fontAsset, font.buffer);
    const assets = new Map<string, string>();
    for (const [index, name] of [
      "position",
      "temperature",
      "offset",
    ].entries()) {
      const builder = new contract.ExpressionBuilder();
      const definition = builder.encode(builder.input(`column:${name}`, "f32"));
      const asset = clientAssetSource(client.session, 19, BigInt(index + 1));
      await client.registerAsset(asset, definition.buffer);
      assets.set(name, asset.source);
    }
    const chart = client.components.PlotBars2d!,
      frame = client.components.PlotFrame2d!;
    const seriesField = (ambiguous = false) => ({
      offset: chart.fields.series!.offset,
      value: {
        kind: "rows" as const,
        value: contract.encodeRowsTable(chart.fields.series!.rows!, {
          nextSlot: 1,
          rows: new Map([[0, series(ambiguous)]]),
        }),
      },
    });
    const property = (
      entity: bigint,
      name: string,
      value: number | null,
    ): Command =>
      value === null
        ? {
            kind: "removeDynamicProperty",
            entity: { kind: "handle", id: entity },
            component: client.components.BufferDataSourceBinding!.id,
            name,
          }
        : {
            kind: "setDynamicProperty",
            entity: { kind: "handle", id: entity },
            component: client.components.BufferDataSourceBinding!.id,
            name,
            value: { kind: "f32", value },
          };
    for (const [panel, initial] of [
      [10, 20],
      [-40, -10],
    ].entries()) {
      const source = `datasets://plot-percentage/${panel}`;
      const producer = await host.datasets.create(
        source,
        "buffer",
        ["position", "temperature", "offset"].map((name) => ({
          name,
          kind: "f32",
        })),
      );
      producers.push(producer);
      const outcome = await host.datasets.update(producer, [
        {
          operation: "append",
          rows: [row(0.5, initial[0]!), row(1.5, initial[1]!)],
        },
      ]);
      check(!outcome.failure, "Percentage Plot initial ingestion rejected");
      const ref = { kind: "alias" as const, alias: panel + 1 };
      const entity = aliasId(
        await client.batch([
          createEntity(ref.alias, `percentage-${panel}`),
          insertComponent(client, "CanvasStyle", ref, {
            x: positions[panel]!,
            y: 120,
          }),
          insertComponent(client, "PlotFrame2d", ref, {
            width: 600,
            height: 500,
            padding_left: 40,
            padding_top: 30,
            padding_right: 20,
            padding_bottom: 50,
            automatic_x: false,
            min_x: 0,
            max_x: 2,
            automatic_y: true,
            min_y: -999,
            max_y: 999,
            red: 0.25,
            green: 0.25,
            blue: 0.25,
            grid_red: 0.1,
            grid_green: 0.1,
            grid_blue: 0.1,
            source: fontAsset.source,
            x_title: "POSITION",
            y_title: panel === 0 ? "POSITIVE" : "SIGNED",
          }),
          {
            kind: "insertComponent",
            entity: ref,
            component: chart.id,
            fields: [
              ...componentFields(client, "PlotBars2d", { gap: 0.4 }),
              seriesField(),
            ],
          },
          insertComponent(client, "BufferDataSourceBinding", ref, { source }),
          ...[...assets].map(
            ([name, source]): Command => ({
              kind: "setDynamicProperty",
              entity: ref,
              component: client.components.BufferDataSourceBinding!.id,
              name,
              value: { kind: "asset", value: { kind: 19, source } },
            }),
          ),
          {
            kind: "setDynamicProperty",
            entity: ref,
            component: client.components.BufferDataSourceBinding!.id,
            name: "temperature_interp_percent",
            value: { kind: "f32", value: percent },
          },
          {
            kind: "setDynamicProperty",
            entity: ref,
            component: client.components.BufferDataSourceBinding!.id,
            name: "offset_interp",
            value: { kind: "f32", value: 4 },
          },
        ]),
        ref.alias,
      );
      entities.push(entity);
    }
    const read = (panel: number) =>
      host.datasets.bindingView(client.session, entities[panel]!);
    const timed = async (panel: number): Promise<Timed> => {
      const value = await until(
        async () => {
          if (clockFailure) throw clockFailure;
          const page = await read(panel);
          return {
            page,
            time:
              page.evaluatedTick === null
                ? undefined
                : clocks.get(page.evaluatedTick),
          };
        },
        ({ page, time }) =>
          page.availability.reason === "Ready" &&
          page.rows.length === 2 &&
          time !== undefined,
        "evaluated binding clock",
      );
      return {
        page: value.page,
        tick: value.page.evaluatedTick!,
        time: value.time!,
      };
    };
    const waitAfter = async (sample: Timed, duration: number) => {
      let clock = await client.waitForFrame(sample.tick);
      const deadline = performance.now() + 15_000;
      while (clock.time < sample.time + duration) {
        check(
          performance.now() < deadline,
          "Percentage Plot Host clock stopped",
        );
        clock = await client.waitForFrame(clock.tick);
      }
    };
    await Promise.all(
      [0, 1].map((panel) =>
        until(
          read.bind(null, panel),
          (page) =>
            page.availability.reason === "Ready" &&
            page.rows.length === 2 &&
            !page.dirty,
          "initial geometry",
        ),
      ),
    );
    binding = await host.setRootOutput(canvasOutput(world.reference), extent);
    const baseline = await capture("percentage-initial", binding);
    check(
      cyanPanel(baseline, 0) > 50_000 && cyanPanel(baseline, 1) > 50_000,
      "Initial percentage bars were not rendered",
    );
    const targets = [
      [50, 60],
      [-80, -50],
    ];
    await Promise.all(
      producers.map(async (producer, panel) => {
        const outcome = await host.datasets.update(
          producer,
          targets[panel]!.map((value, index) => ({
            operation: "edit" as const,
            row: BigInt(index + 1),
            values: row(index + 0.5, value, 100),
          })),
        );
        check(!outcome.failure, "Percentage retarget ingestion rejected");
      }),
    );
    await client.waitForFrame();
    const first = await Promise.all([0, 1].map(timed));
    await waitAfter(first[0]!, 0.7);
    const second = await Promise.all([0, 1].map(timed));
    await waitAfter(second[0]!, 0.7);
    const third = await Promise.all([0, 1].map(timed));
    const measurements = first.map((a, panel) => ({
      early: rateBounds(a, second[panel]!),
      late: rateBounds(second[panel]!, third[panel]!),
      fixed: {
        actual:
          scalar(third[panel]!.page, "offset", 0) - scalar(a.page, "offset", 0),
        expected: 4 * (third[panel]!.time - a.time),
      },
    }));
    await record("percentage-rate", { first, second, third, measurements });
    for (const measurement of measurements) {
      for (const interval of [measurement.early, measurement.late])
        check(
          interval.elapsed > 0.5 &&
            interval.displacement >= interval.lower - 0.002 &&
            interval.displacement <= interval.upper + 0.002,
          "Percentage displacement did not follow pre-step fitted bounds and Host time",
        );
      check(
        measurement.late.before > measurement.early.before * 1.1,
        "Automatic fitted reference did not grow",
      );
      check(
        Math.abs(measurement.fixed.actual - measurement.fixed.expected) < 0.002,
        "Mixed fixed and percentage outputs did not advance once together",
      );
    }
    // Explicit zero holds this actual intermediate display for a coherent paint/pick cut.
    successfulBatch(
      await client.batch(
        entities.flatMap((entity) => [
          property(entity, "temperature_interp_reference", 0),
          property(entity, "offset_interp", null),
        ]),
      ),
    );
    const frozen = await Promise.all([0, 1].map(read));
    const holdClock = await client.waitForFrame();
    const intermediate = await capture("percentage-intermediate", binding);
    await record("percentage-held", { frozen, holdClock });
    const picking = client as unknown as PickingWorldClient;
    const picks = [];
    for (const [panel, page] of frozen.entries()) {
      const mark = panel === 0 ? 0 : 1;
      const a = scalar(page, "temperature", mark),
        maximum = scale(page);
      const x = positions[panel]! + 40 + ((mark + 0.5) / 2) * 540;
      // The bar's X center follows its actual source position. Y=0 is the bottom for
      // positive values and the top for this all-negative fitted frame.
      const boundary =
        a > 0 ? 570 - (a / maximum) * 420 : 150 + (Math.abs(a) / maximum) * 420;
      const y = boundary + (a > 0 ? 12 : -12),
        outside = boundary + (a > 0 ? -12 : 12);
      const ink = cyanAt(intermediate, x, y),
        clear = cyanAt(intermediate, x, outside);
      const pick = await picking.query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding },
        x: x / extent.width,
        y: y / extent.height,
        includeViewPlane: false,
      });
      picks.push({ panel, boundary, ink, clear, pick });
      await record(`percentage-pick-${panel}`, picks.at(-1));
      check(
        ink > 40 && clear === 0,
        "Intermediate paint did not use the held displayed fit",
      );
      check(
        pick.ok &&
          pick.hit !== null &&
          pick.hit.entity === entities[panel] &&
          pick.hit.row?.rowId === BigInt(mark + 1) &&
          pick.hit.row.series === 0,
        "Percentage paint and pick disagree on displayed source row",
      );
    }
    await record("percentage-intermediate", { frozen, picks });
    await waitAfter({ ...third[0]!, ...holdClock }, 0.5);
    const held = await Promise.all([0, 1].map(read));
    check(
      held.every(
        (page, panel) =>
          scalar(page, "temperature", 0) ===
          scalar(frozen[panel]!, "temperature", 0),
      ),
      "Zero reference did not hold display",
    );
    const resumeOrigin = await client.waitForFrame();
    successfulBatch(
      await client.batch(
        entities.map((entity) =>
          property(entity, "temperature_interp_reference", null),
        ),
      ),
    );
    await client.waitForFrame();
    const resumed = await Promise.all([0, 1].map(timed));
    await waitAfter(resumed[0]!, 0.4);
    const progressing = await Promise.all([0, 1].map(timed));
    const resumeBounds = resumed.map((a, panel) =>
      rateBounds(a, progressing[panel]!),
    );
    const continuity = resumed.map((sample, panel) => ({
      displacement: Math.abs(
        scalar(sample.page, "temperature", 1) -
          scalar(frozen[panel]!, "temperature", 1),
      ),
      upper:
        (percent / 100) *
        scale(sample.page) *
        (sample.time - resumeOrigin.time),
    }));
    await record("percentage-resume", {
      resumeOrigin,
      resumed,
      progressing,
      resumeBounds,
      continuity,
    });
    for (const interval of continuity)
      check(
        interval.displacement <= interval.upper + 0.002,
        "Percentage resume included motion from held time",
      );
    for (const interval of resumeBounds)
      check(
        interval.displacement >= interval.lower - 0.002 &&
          interval.displacement <= interval.upper + 0.002,
        "Percentage resume caught up held time",
      );
    const settled = await Promise.all(
      [0, 1].map((panel) =>
        until(
          () => read(panel),
          (page) =>
            page.rows.every(
              (_, row) =>
                scalar(page, "temperature", row) === targets[panel]![row],
            ),
          "endpoint",
        ),
      ),
    );
    const endpoint = await capture("percentage-endpoint", binding);
    await record("percentage-settled", {
      settled,
      leftInk: cyanPanel(endpoint, 0),
      rightInk: cyanPanel(endpoint, 1),
    });
    check(
      cyanPanel(endpoint, 0) > 80_000 && cyanPanel(endpoint, 1) > 80_000,
      "Percentage endpoints were not rendered",
    );
    const idleTick = settled[0]!.evaluatedTick;
    await client.waitForFrame();
    const idle = await read(0);
    check(
      idle.evaluatedTick === idleTick && !idle.dirty,
      "Settled percentage Plot continued preparing work",
    );
    // A settled selector edit needs no reference yet. The next target movement
    // uses one output on both axes and must suppress the chart rather than guess.
    successfulBatch(
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: entities[0]! },
          component: chart.id,
          field: seriesField(true),
        },
        ...componentFields(client, "PlotFrame2d", { automatic_x: true }).map(
          (field): Command => ({
            kind: "setField",
            entity: { kind: "handle", id: entities[0]! },
            component: frame.id,
            field,
          }),
        ),
      ]),
    );
    const beforeInvalid = await capture(
      "percentage-ambiguous-settled",
      binding,
    );
    check(
      cyanPanel(beforeInvalid, 0) > 10_000,
      "Settled ambiguous selector changed before movement requested a reference",
    );
    await host.datasets.update(producers[0]!, [
      { operation: "edit", row: 1n, values: row(0.5, 100, 100) },
    ]);
    await client.waitForFrame();
    const invalid = await capture("percentage-ambiguous-active", binding);
    const invalidPage = await read(0);
    await record("percentage-ambiguous", {
      invalidPage,
      leftInk: cyanPanel(invalid, 0),
      rightInk: cyanPanel(invalid, 1),
    });
    check(
      cyanPanel(invalid, 0) === 0 &&
        cyanPanel(invalid, 1) > 80_000 &&
        scalar(invalidPage, "temperature", 0) === 50,
      "Ambiguous active percentage retained stale geometry or advanced display",
    );
    const raw = await host.datasets.read("datasets://plot-percentage/1");
    check(
      raw.rows[0]!.values[1]!.value === -80,
      "Percentage interpolation changed raw source values",
    );
    await record("percentage-final", { settled, idle, raw });
  } finally {
    collecting = false;
    if (binding) await host.clearRootOutput(binding);
    await client.close();
    await collector;
    await host.destroyWorld(world.reference);
    await Promise.allSettled(
      producers.map((producer) => host.datasets.destroy(producer)),
    );
  }
}
