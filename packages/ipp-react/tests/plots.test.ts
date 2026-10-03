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
