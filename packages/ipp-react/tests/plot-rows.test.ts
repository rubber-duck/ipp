import assert from "node:assert/strict";
import test from "node:test";
import { keyedPlotRows } from "../src/plots/rows.js";

test("keyed Plot rows retain live identities and never reuse retired slots", () => {
  const empty = {
    nextSlot: 0,
    keys: new Map<string | number | bigint, number>(),
  };
  const first = keyedPlotRows(empty, [
    { key: "a", value: { row: 9007199254740993n } },
    { key: "b", value: { row: 2n } },
  ]);
  const second = keyedPlotRows(first.slots, [{ key: "b", value: { row: 3n } }]);
  const third = keyedPlotRows(second.slots, [
    { key: "a", value: { row: 9007199254740993n } },
    { key: "b", value: { row: 3n } },
  ]);
  assert.deepEqual([...third.table.rows.keys()], [2, 1]);
  assert.equal(third.table.nextSlot, 3);
  assert.equal(third.table.rows.get(2)?.row, 9007199254740993n);
  assert.throws(
    () =>
      keyedPlotRows(empty, [
        { key: "a", value: {} },
        { key: "a", value: {} },
      ]),
    /Duplicate/,
  );
  assert.equal(first.table.nextSlot, 2);
});
