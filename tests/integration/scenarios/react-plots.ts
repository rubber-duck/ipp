/** Real React → generated client → dataset transport → Plot → completed frame. */
import { createElement as h } from "react";
import {
  canvasOutput,
  f32,
  type Client,
  type DatasetValue,
  type HostClientBase,
  type RootBinding,
  type RowsInput,
} from "@ipp/client";
import {
  Asset,
  Animation,
  AnimationAsset,
  BufferDataSourceBinding,
  ColumnBindingAsset,
  DataSource,
  Entity,
  PlotFrame2d,
  PlotLine2d,
  assetRef,
  createRoot,
  type AnimationHandle,
  type PlotContract,
  type PlotLabel,
  type PlotSeries,
} from "@ipp/react";
import { Style } from "@ipp/react/gui";
import type * as Generated from "@ipp/host-contract";

type Contract = PlotContract & Pick<typeof Generated, "ExpressionBuilder">;
interface Frame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}
type Capture = (label: string, binding: RootBinding) => Promise<Frame>;
const check = (value: unknown, message: string) => {
  if (!value) throw new Error(message);
};
async function until<T>(read: () => Promise<T>, ready: (value: T) => boolean) {
  const deadline = performance.now() + 30_000;
  for (;;) {
    const value = await read();
    if (ready(value)) return value;
    if (performance.now() > deadline)
      throw new Error("React Plot readiness timed out");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

/** Launch/capture drivers own their environment; this scenario owns its World. */
export async function reactPlots(
  host: HostClientBase<Client>,
  contract: Contract,
  font: Uint8Array<ArrayBuffer>,
  capture: Capture,
  record: (label: string, value: unknown) => Promise<void> = async () => {},
) {
  const world = await host.createWorld({
    temporary: true,
    symbolicId: "react-plots",
    canvas: { extent: [800, 400], unitsPerMetre: 96 },
    selectedSystems: [
      "ipp.animation",
      "ipp.asset-dependencies",
      "ipp.data-bindings",
      "ipp.plot",
      "ipp.canvas",
      "ipp.lifecycle-publisher",
    ],
  });
  const client = await host.openWorld(world.reference);
  const root = createRoot(client);
  const source = `datasets://react-plots/${client.session}`;
  const builder = new contract.ExpressionBuilder();
  const input = builder.input("column:value", "f32");
  const identity = builder.encode(input);
  const scaled = builder.encode(
    builder.binary("multiply", input, builder.input("parameter", "f32")),
  );
  const series: RowsInput<PlotSeries> = {
    nextSlot: 5,
    rows: new Map([[2, { name: "Stable series", x: "x", y: "y" }]]),
  };
  const labels: RowsInput<PlotLabel> = {
    nextSlot: 8,
    rows: new Map([
      [
        7,
        {
          series: 2,
          row: 2n,
          text: "ROW 2",
          highlighted: true,
          offset: [20, -25],
        },
      ],
    ]),
  };
  let animation: AnimationHandle | null = null;
  const clip = {
    duration: 1,
    tracks: [
      {
        property: {
          component: client.components.BufferDataSourceBinding!.id,
          name: "y_parameter",
        },
        keys: [
          {
            time: 0,
            value: {
              kind: "dynamic" as const,
              value: { kind: "f32" as const, value: 2 },
            },
            interpolation: { kind: "linear" as const },
          },
          {
            time: 1,
            value: {
              kind: "dynamic" as const,
              value: { kind: "f32" as const, value: 4 },
            },
          },
        ],
      },
    ],
  };
  try {
    await root.render(
      h(
        DataSource,
        {
          name: source,
          ownership: "producer",
          datasets: host.datasets,
          kind: "buffer",
          schema: [{ name: "value", kind: "f32" }],
        },
        h(Asset<Uint8Array<ArrayBuffer>>, {
          id: "font",
          kind: 17,
          data: font,
          encode: (bytes) => bytes,
        }),
        h(ColumnBindingAsset, { id: "identity", definition: identity }),
        h(ColumnBindingAsset, { id: "scaled", definition: scaled }),
        h(AnimationAsset, { id: "parameter-clip", clip }),
        ...[2, 3].map((parameter, index) =>
          h(
            Entity,
            { key: index, id: `plot-${index}` },
            h(Style, { x: 20 + index * 390, y: 35 }),
            h(PlotFrame2d, {
              width: 370,
              height: 340,
              min_x: 0,
              max_x: 6,
              min_y: 0,
              max_y: 20,
              automatic_x: false,
              automatic_y: false,
              source: assetRef("font"),
              x_title: "SOURCE",
              y_title: "EVALUATED",
              font_size: 14,
            }),
            h(BufferDataSourceBinding, {
              source,
              columns: {
                x: { definition: assetRef("identity") },
                y: {
                  definition: assetRef("scaled"),
                  parameter: f32(parameter),
                },
              },
            }),
            h(PlotLine2d, {
              contract,
              series,
              labels,
              interpolation: index === 0 ? "straight" : "smooth",
            }),
            ...(index === 0
              ? [
                  h(Animation, {
                    source: assetRef("parameter-clip"),
                    autoPlay: false,
                    ref: (value) => {
                      animation = value;
                    },
                  }),
                ]
              : []),
          ),
        ),
      ),
    );
    const handle = await until(
      async () => root.getDataSource(source)?.handle,
      (value) => value !== undefined,
    );
    const row = (value: number): DatasetValue[] => [{ kind: "f32", value }];
    const outcome = await handle!.update([
      { operation: "append", rows: [row(1), row(2), row(3)] },
    ]);
    check(!outcome.failure, "React source update rejected");
    const inspection = await client.inspect();
    const entities = [0, 1].map(
      (index) =>
        inspection.entities.find(
          (entity) => entity.metadata.symbolicId === `plot-${index}`,
        )!.id,
    );
    const views = async () =>
      Promise.all(
        entities.map((entity) =>
          host.datasets.bindingView(client.session, entity),
        ),
      );
    const outputs = (
      page: Awaited<ReturnType<typeof host.datasets.bindingView>>,
    ) => {
      const column = page.columns.findIndex((column) => column.name === "y");
      return page.rows.map((row) => {
        const value = row.values[column];
        return value?.valid && value.value.kind === "f32"
          ? value.value.value
          : null;
      });
    };
    let pages = await until(views, (pages) =>
      pages.every(
        (page) =>
          page.availability.reason === "Ready" &&
          !page.dirty &&
          page.rows.length === 3,
      ),
    );
    check(
      pages[0]!.sourceIncarnation === pages[1]!.sourceIncarnation,
      "React duplicated the shared source",
    );
    check(
      JSON.stringify(pages.map(outputs)) === "[[2,4,6],[3,6,9]]",
      "Independent bindings produced wrong values",
    );
    const binding = await host.setRootOutput(canvasOutput(world.reference), {
      width: 800,
      height: 400,
      devicePixelRatio: 1,
    });
    const baseline = await capture("react-plots-baseline", binding);
    for (const start of [0, 400]) {
      let cyan = 0;
      for (let y = 35; y < 375; y++)
        for (let x = start; x < start + 400; x++) {
          const p = (y * baseline.width + x) * 4;
          if (
            baseline.pixels[p]! < 80 &&
            baseline.pixels[p + 1]! > 160 &&
            baseline.pixels[p + 2]! > 160
          )
            cyan++;
        }
      check(cyan > 80, "React chart frame lacks rendered series pixels");
    }
    const edit = await handle!.update([
      { operation: "edit", row: 2n, values: row(5) },
    ]);
    check(!edit.failure, "React source edit rejected");
    pages = await until(
      views,
      (pages) =>
        JSON.stringify(pages.map(outputs)) === "[[2,10,6],[3,15,9]]" &&
        pages.every((page) => !page.dirty),
    );
    await until(
      async () => animation,
      (value) => value !== null,
    );
    await animation!.playAtSpeed(0);
    await until(
      () => client.inspect(),
      (view) =>
        view.controllers?.some(
          (controller) => controller.state === "playing",
        ) ?? false,
    );
    await animation!.pause();
    await animation!.seek(1);
    pages = await until(
      views,
      (pages) =>
        JSON.stringify(pages.map(outputs)) === "[[4,20,12],[3,15,9]]" &&
        pages.every((page) => !page.dirty),
    );
    const changed = await capture("react-plots-changed", binding);
    let difference = 0;
    for (let p = 0; p < changed.pixels.length; p += 4)
      if (Math.abs(changed.pixels[p + 1]! - baseline.pixels[p + 1]!) > 50)
        difference++;
    check(
      difference > 100,
      "Real React source/parameter update did not change completed pixels",
    );
    check(
      pages[0]!.rows.map((row) => row.id.toString()).join(",") === "1,2,3",
      "Edits or animation changed source row identities",
    );
    await record("react.plots", {
      entities,
      sharedSource: pages[0]!.sourceIncarnation,
      animatedValues: pages.map(outputs),
      difference,
    });
  } finally {
    await root.render(null).catch(() => {});
    await root.unmount();
    await client.close();
    await host.destroyWorld(world.reference);
  }
}
