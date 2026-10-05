/** Authored identities are checked with the receiving generated row codec. */
import assert from "node:assert/strict";
import test from "node:test";
import * as contract from "../../../target/integration-artifacts/client/generated.js";
import {
  PlotLine2d,
  PlotBars2d,
  PlotPie2d,
  PlotGridBars3d,
  PlotHeightSurface3d,
  PlotPoints3d,
  PlotPie3d,
} from "../src/plots.js";
import {
  PlotLegend,
  plotColorScaleColor,
  plotLegendPlacement,
  plotLegendSize,
  type PlotColorScale,
} from "../src/plot-legend.js";

const props = {
  contract,
  series: {
    nextSlot: 12,
    rows: new Map([[7, { name: "Retained series", x: "time", y: "signal" }]]),
  },
  labels: {
    nextSlot: 18,
    rows: new Map([
      [
        16,
        {
          series: 7,
          row: 9007199254740993n,
          text: "Exact source row",
          highlighted: true,
        },
      ],
    ]),
  },
};

test("all Plot wrappers retain sparse slots and lossless source row IDs", () => {
  const declarations = [
    PlotLine2d(props),
    PlotBars2d(props),
    PlotPie2d(props),
    PlotGridBars3d(props),
    PlotHeightSurface3d(props),
    PlotPoints3d(props),
    PlotPie3d(props),
  ];
  const names = [
    "PlotLine2d",
    "PlotBars2d",
    "PlotPie2d",
    "PlotGridBars3d",
    "PlotHeightSurface3d",
    "PlotPoints3d",
    "PlotPie3d",
  ] as const;
  declarations.forEach((declaration, index) => {
    const descriptor = contract.components[names[index]!];
    const series = contract.decodeRowsTable(
      descriptor.fields.series.rows,
      declaration.props.series!,
    );
    const labels = contract.decodeRowsTable(
      descriptor.fields.labels.rows,
      declaration.props.labels!,
    );
    assert.equal(series.nextSlot, 12);
    assert.deepEqual([...series.rows.keys()], [7]);
    assert.equal(series.rows.get(7)!.y, "signal");
    assert.equal(labels.nextSlot, 18);
    assert.deepEqual([...labels.rows.keys()], [16]);
    assert.equal(labels.rows.get(16)!.series, 7);
    assert.equal(labels.rows.get(16)!.row_id, "9007199254740993");
  });
});

test("labels keep u64 maximum and reject inexact authoring input", () => {
  const declaration = PlotLine2d({
    ...props,
    labels: {
      nextSlot: 1,
      rows: new Map([
        [0, { series: 7, row: 0xffffffffffffffffn, text: "Maximum row" }],
      ]),
    },
  });
  const labels = contract.decodeRowsTable(
    contract.components.PlotLine2d.fields.labels.rows,
    declaration.props.labels!,
  );
  assert.equal(labels.rows.get(0)!.row_id, "18446744073709551615");
  for (const row of [
    0n,
    -1n,
    0x10000000000000000n,
    9007199254740993 as unknown as bigint,
  ])
    assert.throws(
      () =>
        PlotLine2d({
          ...props,
          labels: {
            nextSlot: 1,
            rows: new Map([[0, { series: 7, row, text: "Invalid" }]]),
          },
        }),
      /positive u64 BigInt/,
    );
});

test("removed series slots stay dead when series order changes", () => {
  const declaration = PlotLine2d({
    ...props,
    series: {
      nextSlot: 12,
      rows: new Map([
        [9, { name: "B" }],
        [7, { name: "A" }],
      ]),
    },
    interpolation: "smooth",
  });
  const series = contract.decodeRowsTable(
    contract.components.PlotLine2d.fields.series.rows,
    declaration.props.series!,
  );
  assert.deepEqual(
    [...series.rows.keys()].sort((a, b) => a - b),
    [7, 9],
  );
  assert.equal(series.nextSlot, 12);
  assert.equal(declaration.props.interpolation, 1);
  assert.equal(series.rows.has(8), false);
});

test("numeric legend colors share a clamped linear RGBA scale", () => {
  const scale: PlotColorScale = {
    min: -20,
    max: 80,
    colors: [
      [0, 0, 1, 0.25],
      [0, 1, 0, 0.5],
      [1, 0, 0, 1],
    ],
  };
  assert.deepEqual(plotColorScaleColor(scale, -40), scale.colors[0]);
  assert.deepEqual(plotColorScaleColor(scale, 100), scale.colors[2]);
  assert.deepEqual(plotColorScaleColor(scale, 5), [0, 0.5, 0.5, 0.375]);
  assert.deepEqual(plotColorScaleColor(scale, 55), [0.5, 0.5, 0, 0.75]);
  for (const invalid of [NaN, Infinity, -Infinity])
    assert.throws(() => plotColorScaleColor(scale, invalid), /finite/);
  assert.throws(
    () => plotColorScaleColor({ ...scale, max: -20 }, 0),
    /increasing range/,
  );
  assert.throws(
    () => plotColorScaleColor({ ...scale, colors: [[1, 0, 0, 1]] }, 0),
    /two colors/,
  );
  assert.throws(
    () =>
      plotColorScaleColor(
        {
          ...scale,
          colors: [
            [1, 0, 0, 2],
            [0, 1, 0, 1],
          ],
        },
        0,
      ),
    /0\.\.1/,
  );
});

test("legend placement centers outside bounds in canvas and scene XY conventions", () => {
  const options = {
    bounds: [20, 30, 620, 390],
    size: [180, 88],
    gap: 16,
  } as const;
  for (const yDirection of ["down", "up"] as const) {
    const placement = plotLegendPlacement({ ...options, yDirection });
    assert.deepEqual(placement.bounds, [636, 166, 816, 254]);
    assert.deepEqual(placement.center, [726, 210]);
    assert.deepEqual(placement.canvasPosition, [
      636,
      yDirection === "down" ? 166 : 254,
    ]);
    const left = plotLegendPlacement({
      ...options,
      yDirection,
      origin: "top-right",
    });
    assert.equal(left.bounds[2], options.bounds[0] - options.gap);
    assert.equal(left.center[1], placement.center[1]);
    for (const side of ["top", "bottom"] as const) {
      const edge = plotLegendPlacement({ ...options, yDirection, side });
      assert.equal(edge.center[0], (options.bounds[0] + options.bounds[2]) / 2);
      const above = (side === "top") === (yDirection === "down");
      assert.equal(
        above ? edge.bounds[3] : edge.bounds[1],
        above
          ? options.bounds[1] - options.gap
          : options.bounds[3] + options.gap,
      );
    }
  }
  assert.throws(
    () => plotLegendPlacement({ ...options, yDirection: "down", gap: -1 }),
    /ordered bounds/,
  );
  assert.throws(
    () =>
      plotLegendPlacement({ ...options, yDirection: "down", size: [0, 88] }),
    /ordered bounds/,
  );
  const base = plotLegendPlacement({
    bounds: options.bounds,
    size: options.size,
    yDirection: "up",
  });
  const scaled = plotLegendPlacement({
    bounds: options.bounds.map((value) => value / 100) as unknown as readonly [
      number,
      number,
      number,
      number,
    ],
    size: [1.8, 0.88],
    yDirection: "up",
  });
  scaled.bounds.forEach((value, index) =>
    assert.ok(Math.abs(value * 100 - base.bounds[index]!) < 1e-10),
  );
});

test("legend content validates identities and dimensions before declarations", () => {
  const entries = [
    { id: "series-a", label: "Series A", color: [0, 0.5, 1, 1] as const },
  ];
  const props = { id: "legend", font: "fonts://local", entries };
  assert.deepEqual(plotLegendSize({ ...props, title: "SERIES" }), [180, 64]);
  assert.equal(PlotLegend({ ...props, entries: [] }), null);
  assert.equal(PlotLegend({ ...props, entries: [], padding: 0 }), null);
  assert.deepEqual(
    plotLegendSize({ ...props, entries: [], padding: 0 }),
    [180, 24],
  );
  assert.throws(
    () => PlotLegend({ ...props, entries: [entries[0]!, entries[0]!] }),
    /unique nonempty ids/,
  );
  assert.throws(() => PlotLegend({ ...props, width: 30 }), /dimensions/);
  assert.throws(() => PlotLegend({ ...props, rowHeight: 10 }), /dimensions/);
  assert.throws(() => PlotLegend({ ...props, layer: -1 }), /u32/);
  assert.throws(() => PlotLegend({ ...props, x: NaN }), /finite/);
});
