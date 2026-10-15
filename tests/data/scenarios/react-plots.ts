/** Real React → generated client → dataset transport → Plot → completed frame. */
import { createElement as h, Fragment, useLayoutEffect } from "react";
import {
  canvasOutput,
  f32,
  type Client,
  type DatasetValue,
  type HostClientBase,
  type RootBinding,
} from "@ipp/client";
import {
  FontAsset,
  Animation,
  AnimationAsset,
  BufferDataSourceBinding,
  Children,
  ColumnBindingAsset,
  DataSource,
  Entity,
  PlotFrame2d,
  PlotLegend,
  PlotLine2d,
  PlotSkinProvider,
  ShaderAsset,
  PaintShader,
  assetRef,
  createWorldRoot,
  useAsset,
  useDataSource,
  useEntityId,
  useField,
  useWorld,
  usePlotRows,
  type ReactAssetState,
  type ReactDataSourceState,
  type ReactFieldState,
  plotLegendPlacement,
  plotLegendSize,
  type AnimationHandle,
  type ReactWorldContract,
  type PlotSeries,
} from "@ipp/react";
import { Paint, Style } from "@ipp/react/gui";
import { glassSkin } from "@ipp/react/gui-glass";
import type * as Generated from "@ipp/host-contract";
import { check, until as untilReady } from "../../harness/page/checks.js";

type Contract = ReactWorldContract &
  Pick<typeof Generated, "ExpressionBuilder">;
interface Frame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}
type Capture = (label: string, binding: RootBinding) => Promise<Frame>;

function srgb(value: number) {
  return Math.round(
    255 *
      (value <= 0.0031308 ? 12.92 * value : 1.055 * value ** (1 / 2.4) - 0.055),
  );
}

function colorPixels(
  frame: Frame,
  box: readonly number[],
  color: readonly number[],
) {
  const expected = color.slice(0, 3).map(srgb);
  let matching = 0;
  for (let y = Math.floor(box[1]!); y < Math.ceil(box[3]!); y++)
    for (let x = Math.floor(box[0]!); x < Math.ceil(box[2]!); x++) {
      const at = (y * frame.width + x) * 4;
      if (
        expected.every(
          (value, channel) =>
            Math.abs(frame.pixels[at + channel]! - value) < 20,
        )
      )
        matching++;
    }
  return matching;
}

function glyphInkPixels(frame: Frame, box: readonly number[]) {
  let ink = 0;
  for (let y = Math.floor(box[1]!); y < Math.ceil(box[3]!); y++)
    for (let x = Math.floor(box[0]!); x < Math.ceil(box[2]!); x++) {
      const at = (y * frame.width + x) * 4;
      const rgb = [...frame.pixels.slice(at, at + 3)];
      if (Math.min(...rgb) > 180 && Math.max(...rgb) - Math.min(...rgb) < 25)
        ink++;
    }
  return ink;
}

function until<T>(read: () => Promise<T>, ready: (value: T) => boolean) {
  return untilReady(read, ready, "React Plot readiness", {
    timeoutMs: 30_000,
    intervalMs: 10,
  });
}

/** Launch/capture drivers own their environment; this scenario owns its World. */
export async function reactPlots(
  host: HostClientBase<Client>,
  contract: Contract,
  font: Uint8Array<ArrayBuffer>,
  capture: Capture,
  record: (label: string, value: unknown) => Promise<void> = async () => {},
) {
  const owned = await createWorldRoot(
    host,
    {
      temporary: true,
      symbolicId: "react-plots",
      canvas: { extent: [1200, 400], unitsPerMetre: 96 },
      selectedSystems: [
        "ipp.animation",
        "ipp.asset-dependencies",
        "ipp.data-bindings",
        "ipp.plot",
        "ipp.canvas",
        "ipp.lifecycle-publisher",
      ],
    },
    { contract },
  );
  const { world, client, root } = owned;
  let watchAdds = 0;
  let watchRemoves = 0;
  const watchLifecycle = client.watchLifecycle.bind(client);
  client.watchLifecycle = async (targets, listener) => {
    const fieldWatch = targets.some(
      ({ target }) =>
        target.kind === "value" &&
        target.component === client.components.PlotFrame2d!.id,
    );
    if (fieldWatch) watchAdds++;
    const watch = await watchLifecycle(targets, listener);
    return {
      ...watch,
      remove: async () => {
        if (fieldWatch) watchRemoves++;
        return watch.remove();
      },
    };
  };
  const source = `datasets://react-plots/${client.session}`;
  let observedAsset: ReactAssetState | undefined;
  let observedSource: ReactDataSourceState | undefined;
  let observedField: ReactFieldState | undefined;
  const observedIds = new Set<string>();
  let observedFieldName = "width";
  let targetChangedPending = false;
  function ScopeProbe() {
    const scope = useWorld();
    const id = useEntityId();
    observedAsset = useAsset("font");
    observedSource = useDataSource(source);
    observedField = useField("plot-0", "PlotFrame2d", observedFieldName);
    if (observedFieldName === "height" && observedField.status === "pending")
      targetChangedPending = true;
    useLayoutEffect(() => {
      observedIds.add(id);
    });
    check(
      scope.client === client && scope.contract === contract,
      "Hooks resolve the receiving World and target contract",
    );
    return null;
  }
  const builder = new contract.ExpressionBuilder();
  const input = builder.input("column:value", "f32");
  const identity = builder.encode(input);
  const colorIdentity = builder.encode(builder.input("column:color", "vec4"));
  const scaled = builder.encode(
    builder.binary("multiply", input, builder.input("parameter", "f32")),
  );
  const cyan = [0, 0.8, 1, 1] as const;
  const yellow = [1, 0.8, 0, 1] as const;
  const categorical = {
    entries: [{ id: "stable", label: "Stable series", color: cyan }],
  };
  const numeric = { scale: { min: 1, max: 3, colors: [cyan, yellow] } };
  let categoryLabel = "Stable series";
  let categoryColor: readonly [number, number, number, number] = cyan;
  const legends = [categorical, numeric].map((content) => {
    const size = plotLegendSize({ ...content, title: "COLOR", width: 180 });
    const placement = plotLegendPlacement({
      bounds: [0, 0, 370, 340],
      size,
      yDirection: "down",
      origin: "top-left",
      gap: 16,
    });
    check(
      placement.canvasPosition[0] === 386 &&
        placement.canvasPosition[1] === 170 - size[1] / 2,
      "Legend is outside the right frame edge and vertically centered",
    );
    return { content, size, placement };
  });
  const glass = glassSkin(
    contract as unknown as Parameters<typeof glassSkin>[0],
  );
  const chartSkin = {
    ...glass,
    plots: (scope: string) => ({ paint: glass.plots!(scope).paint! }),
  };
  const seriesSlots = new Map<number, number>();
  function SeriesPlot({ index }: { index: number }) {
    const value: PlotSeries = {
      name: categoryLabel,
      x: "x",
      y: "y",
      ...(index === 0 ? { color: categoryColor } : { color_column: "color" }),
    };
    const series = usePlotRows([{ key: "stable", value }]);
    const slot = [...series.rows.keys()][0]!;
    seriesSlots.set(index, slot);
    const labels = usePlotRows([
      {
        key: "row-two",
        value: {
          series: slot,
          row: 2n,
          text: "ROW 2",
          highlighted: true,
          offset: [20, -25] as const,
        },
      },
    ]);
    const plot = h(PlotLine2d, {
      series,
      labels,
      interpolation: index === 0 ? "straight" : "smooth",
    });
    const explicit = h(Paint, { source: assetRef("chart-explicit") });
    return h(
      Fragment,
      null,
      ...(index === 0 ? [explicit, plot] : [plot, explicit]),
    );
  }
  let playing: boolean | undefined;
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
    const scene = () =>
      h(
        PlotSkinProvider,
        { id: "chart-skin", skin: chartSkin },
        h(
          DataSource,
          {
            name: source,
            ownership: "producer",
            datasets: host.datasets,
            kind: "buffer",
            schema: [
              { name: "value", kind: "f32" },
              { name: "color", kind: "vec4" },
            ],
          },
          h(ScopeProbe),
          h(
            ShaderAsset,
            { id: "chart-explicit", recipe: {}, parameters: {} },
            h(PaintShader, null, "return color;"),
          ),
          h(FontAsset, {
            id: "font",
            data: font,
          }),
          h(ColumnBindingAsset, { id: "identity", definition: identity }),
          h(ColumnBindingAsset, {
            id: "color-identity",
            definition: colorIdentity,
          }),
          h(ColumnBindingAsset, { id: "scaled", definition: scaled }),
          h(AnimationAsset, { id: "parameter-clip", clip }),
          ...[2, 3].map((parameter, index) =>
            h(
              Entity,
              { key: index, id: `plot-${index}` },
              h(Style, { x: 20 + index * 600, y: 35 }),
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
                  ...(index === 1
                    ? { color: { definition: assetRef("color-identity") } }
                    : {}),
                },
              }),
              h(SeriesPlot, { index }),
              h(
                Children,
                null,
                h(PlotLegend, {
                  id: `legend-${index}`,
                  font: assetRef("font"),
                  title: "COLOR",
                  width: 180,
                  ...(index === 0
                    ? {
                        entries: [
                          {
                            id: "stable",
                            label: categoryLabel,
                            color: categoryColor,
                          },
                        ],
                      }
                    : numeric),
                  x: legends[index]!.placement.canvasPosition[0],
                  y: legends[index]!.placement.canvasPosition[1],
                }),
              ),
              ...(index === 0
                ? [
                    h(Animation, {
                      source: assetRef("parameter-clip"),
                      autoPlay: false,
                      ...(playing === undefined ? {} : { playing }),
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
    await root.render(scene());
    const handle = await until(
      async () => root.getDataSource(source)?.handle,
      (value) => value !== undefined,
    );
    const row = (value: number): DatasetValue[] => {
      const fraction = Math.max(0, Math.min(1, (value - 1) / 2));
      return [
        { kind: "f32", value },
        { kind: "vec4", value: [fraction, 0.8, 1 - fraction, 1] },
      ];
    };
    const outcome = await handle!.update([
      { operation: "append", rows: [row(1), row(2), row(3)] },
    ]);
    check(!outcome.failure, "React source update rejected");
    // A provider mounts consumers after its required assets are ready.
    const inspection = await until(
      () => client.inspect(),
      (snapshot) =>
        [0, 1].every((index) =>
          snapshot.entities.some(
            (entity) => entity.metadata.symbolicId === `plot-${index}`,
          ),
        ),
    );
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
    const fontReadiness = await until(
      async () => {
        const asset = root.getAsset("font");
        const page = await client.inspectPage({ collection: "resources" });
        const resource = page.resources.find(
          (resource) =>
            resource.kind === 17 &&
            resource.variant === 0 &&
            resource.source === asset?.pendingSource,
        );
        if (asset?.status === "failed" || resource?.status === "failed")
          throw new Error(
            `React Plot font failed: ${asset?.error ?? resource?.error ?? resource?.source}`,
          );
        return { asset, resource };
      },
      ({ asset, resource }) =>
        asset?.status === "loaded" &&
        asset.current?.source === resource?.source &&
        resource?.status === "loaded" &&
        resource.representation.decoded,
    );
    // Ready-gated font selections can author source fields after the first
    // commit. Acknowledge those writes before capture requests a new output.
    await root.flush();
    await until(
      async () => {
        await client.waitForFrame();
        return observedField;
      },
      (value) => value?.status === "ready",
    );
    check(
      observedField?.value === 370,
      "Field hook reports native frame width",
    );
    check(
      observedSource?.status === "ready" && observedSource.handle === handle,
      "Data source hook observes acknowledged producer ownership",
    );
    check(
      observedAsset?.status === "loaded" &&
        observedAsset.current?.source === fontReadiness.asset?.current?.source,
      "Asset hook observes loaded font selection",
    );
    check(
      observedIds.size === 1,
      "Generated IDs retain identity across readiness renders",
    );
    await record("react.font-readiness", {
      asset: fontReadiness.asset,
      resource: fontReadiness.resource,
    });
    const binding = await host.setRootOutput(canvasOutput(world.reference), {
      width: 1200,
      height: 400,
      devicePixelRatio: 1,
    });
    await until(
      async () => root.getAsset("chart-explicit"),
      (asset) => asset?.status === "loaded",
    );
    const explicitSource = root.getAsset("chart-explicit")!.current!.source;
    const painted = await client.inspect();
    for (const index of [0, 1]) {
      const entity = painted.entities.find(
        (entry) => entry.metadata.symbolicId === `plot-${index}`,
      )!;
      check(
        entity.components.some(
          (component) => component.fields.source === explicitSource,
        ),
        `Explicit paint wins over the provider in declaration order ${index}`,
      );
    }
    const baseline = await capture("react-plots-baseline", binding);
    for (const start of [0, 600]) {
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
      check(
        cyan > (start === 0 ? 80 : 20),
        "React chart frame lacks rendered series pixels",
      );
    }
    const legendInspection = await client.inspect();
    const fields = (id: string, field: string) =>
      legendInspection.entities
        .find((entity) => entity.metadata.symbolicId === id)
        ?.components.find((component) => field in component.fields)?.fields;
    for (const [index, legend] of legends.entries()) {
      const style = fields(`legend-${index}`, "scale_x");
      check(
        style?.x === 386 && style.y === 170 - legend.size[1] / 2,
        "Acknowledged legend style retains the computed right/middle placement",
      );
      const title = fields(`legend-${index}/title`, "text");
      check(
        title?.text === "COLOR",
        "Legend title is acknowledged as real Canvas text",
      );
    }
    const swatch = fields("legend-0/entry/stable/swatch", "scale_x")!;
    const swatchBox = fields("legend-0/entry/stable/swatch", "width")!;
    const swatchX = 406 + Number(swatch.x),
      swatchY = 35 + legends[0]!.placement.canvasPosition[1] + Number(swatch.y);
    check(
      colorPixels(
        baseline,
        [
          swatchX,
          swatchY,
          swatchX + Number(swatchBox.width),
          swatchY + Number(swatchBox.height),
        ],
        cyan,
      ) > 20,
      "Categorical legend swatch renders the same cyan as the line",
    );
    for (const [strip, color] of [
      [0, yellow],
      [15, [16 / 31, 0.8, 15 / 31, 1]],
      [31, cyan],
    ] as const) {
      const style = fields(`legend-1/scale/${strip}`, "scale_x")!,
        box = fields(`legend-1/scale/${strip}`, "width")!,
        bandBottom =
          strip < 31
            ? Number(fields(`legend-1/scale/${strip + 1}`, "scale_x")!.y)
            : Number(style.y) + Number(box.height);
      const x = 1006 + Number(style.x),
        y = 35 + legends[1]!.placement.canvasPosition[1] + Number(style.y);
      check(
        colorPixels(
          baseline,
          [x, y, x + Number(box.width), y + bandBottom - Number(style.y)],
          color,
        ) > 15,
        `Numeric legend renders independently calculated strip ${strip} color`,
      );
    }
    // This palette keeps green at 0.8 throughout the scale. Every interior
    // pixel must preserve it, including the boundaries between ramp strips.
    const rampTop = fields("legend-1/scale/0", "scale_x")!,
      rampBottom = fields("legend-1/scale/31", "scale_x")!,
      rampTopBox = fields("legend-1/scale/0", "width")!,
      rampBottomBox = fields("legend-1/scale/31", "width")!;
    const rampX = Math.round(
        1006 + Number(rampTop.x) + Number(rampTopBox.width) / 2,
      ),
      rampY = 35 + legends[1]!.placement.canvasPosition[1];
    const rampGreen: number[] = [];
    for (
      let y = Math.ceil(rampY + Number(rampTop.y)) + 2;
      y <
      Math.floor(rampY + Number(rampBottom.y) + Number(rampBottomBox.height)) -
        2;
      y++
    )
      rampGreen.push(baseline.pixels[(y * baseline.width + rampX) * 4 + 1]!);
    check(
      rampGreen.length > 50 &&
        rampGreen.every((green) => Math.abs(green - srgb(0.8)) <= 8),
      `Numeric ramp preserves continuous opaque coverage across strip boundaries (green ${Math.min(...rampGreen)}..${Math.max(...rampGreen)}, expected ${srgb(0.8)})`,
    );
    await record("react.legend-ramp-coverage", {
      x: rampX,
      samples: rampGreen.length,
      expectedGreen: srgb(0.8),
      minGreen: Math.min(...rampGreen),
      maxGreen: Math.max(...rampGreen),
    });
    check(
      fields("legend-1/min", "text")?.text === "1" &&
        fields("legend-1/max", "text")?.text === "3",
      "Numeric legend renders the authored endpoint values",
    );
    const glyphs = [
      [0, "legend-0/entry/stable/label", 30],
      [1, "legend-1/min", 5],
      [1, "legend-1/max", 5],
    ] as const;
    const glyphMeasurements = glyphs.map(([index, id, minimum]) => {
      const style = fields(id, "scale_x")!,
        x = 406 + index * 600 + Number(style.x),
        y = 35 + legends[index]!.placement.canvasPosition[1] + Number(style.y);
      const ink = glyphInkPixels(baseline, [
        x,
        y,
        x + Number(style.clip_max_x),
        y + Number(style.clip_max_y),
      ]);
      check(
        ink > minimum,
        `${id}: actual label region contains readable glyph ink (${ink})`,
      );
      return { id, ink };
    });
    await record("react.legend-glyphs", glyphMeasurements);
    check(
      colorPixels(baseline, [620, 35, 990, 375], cyan) > 20 &&
        colorPixels(baseline, [620, 35, 990, 375], yellow) > 20,
      "Real source color bindings render both numeric endpoint colors in the line",
    );
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
    categoryLabel = "Updated series";
    categoryColor = [1, 0.15, 0, 1];
    await root.render(scene());
    const updatedInspection = await client.inspect();
    for (const id of [
      "legend-0",
      "legend-0/entry/stable/swatch",
      "legend-0/entry/stable/label",
    ])
      check(
        updatedInspection.entities.find(
          (entity) => entity.metadata.symbolicId === id,
        )?.id ===
          legendInspection.entities.find(
            (entity) => entity.metadata.symbolicId === id,
          )?.id,
        "Legend edits preserve the root and stable entry entities",
      );
    const updatedLabel = updatedInspection.entities
      .find(
        (entity) =>
          entity.metadata.symbolicId === "legend-0/entry/stable/label",
      )
      ?.components.find((component) => "text" in component.fields)?.fields.text;
    check(
      updatedLabel === "Updated series",
      "Rerender updates the real legend label",
    );
    const updatedFrame = await capture("react-plots-legend-updated", binding);
    check(
      colorPixels(
        updatedFrame,
        [
          swatchX,
          swatchY,
          swatchX + Number(swatchBox.width),
          swatchY + Number(swatchBox.height),
        ],
        categoryColor,
      ) > 20 &&
        colorPixels(updatedFrame, [20, 35, 390, 375], categoryColor) > 80,
      "Rerender applies the same new color to the legend and rendered series",
    );
    check(
      Number(watchAdds) === 1 && Number(watchRemoves) === 0,
      `Routine commits and native value reports reuse one admitted field watch (${watchAdds} adds/${watchRemoves} removals)`,
    );
    check(
      seriesSlots.get(0) === 0 && seriesSlots.get(1) === 0,
      "Keyed Plot rows retain series slots through color edits",
    );
    observedFieldName = "height";
    await root.render(scene());
    await until(
      async () => {
        await client.waitForFrame();
        return observedField;
      },
      (value) => value?.status === "ready" && value.value === 340,
    );
    check(
      targetChangedPending &&
        Number(watchAdds) === 2 &&
        Number(watchRemoves) === 1,
      "Field target changes clear stale values and replace exactly one watch",
    );
    const controller = (await client.inspect()).controllers![0]!;
    await animation!.playAtSpeed(0);
    playing = true;
    await root.render(scene());
    check(
      (await client.inspect()).controllers![0]?.state === "playing",
      "Live playing prop starts the retained native controller",
    );
    playing = false;
    await root.render(scene());
    const paused = (await client.inspect()).controllers![0]!;
    check(
      paused.state === "paused" && paused.id === controller.id,
      "Live playing prop pauses without controller replacement",
    );
    await record("react.legend", { legends, categoryLabel, categoryColor });
  } finally {
    try {
      await root.render(null);
      const cleaned = await client.inspect();
      check(
        cleaned.entities.length === 0 &&
          (cleaned.controllers?.length ?? 0) === 0,
        "Removing React chart/legend declarations releases every owned entity and controller",
      );
      const sourceReleased = await host.datasets.read(source).then(
        () => false,
        (error: unknown) => String(error) === "Error: MissingSource",
      );
      check(
        sourceReleased,
        "Removing the last chart consumer and producer releases the dataset",
      );
    } finally {
      await owned.dispose();
    }
  }
}
