import { useLayoutEffect, useRef } from "react";
import type { RowsInput } from "@ipp/client";

export interface KeyedPlotRow<Row> {
  readonly key: string | number | bigint;
  readonly value: Row;
}

interface Slots {
  nextSlot: number;
  keys: Map<string | number | bigint, number>;
}

/** Authored keys preserve occupied slots; removed slots are never allocated again. */
export function keyedPlotRows<Row extends object>(
  previous: Slots,
  input: readonly KeyedPlotRow<Row>[],
): { table: RowsInput<Row>; slots: Slots } {
  let nextSlot = previous.nextSlot;
  const keys = new Map<string | number | bigint, number>();
  const rows = new Map<number, Row>();
  for (const { key, value } of input) {
    if (keys.has(key))
      throw new Error(`Duplicate Plot row key: ${String(key)}`);
    const slot = previous.keys.get(key) ?? nextSlot++;
    if (slot >= 0xffffffff)
      throw new RangeError("Plot row slot capacity exhausted");
    keys.set(key, slot);
    rows.set(slot, value);
  }
  return { table: { nextSlot, rows }, slots: { nextSlot, keys } };
}

/** Only a committed render retires keys; speculative React renders do not alter identity. */
export function usePlotRows<Row extends object>(
  input: readonly KeyedPlotRow<Row>[],
): RowsInput<Row> {
  const committed = useRef<Slots>({ nextSlot: 0, keys: new Map() });
  const result = keyedPlotRows(committed.current, input);
  useLayoutEffect(() => {
    committed.current = result.slots;
  });
  return result.table;
}
