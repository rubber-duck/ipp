import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import type { Client, DatasetProducer, HostClientBase } from "@ipp/client";

export interface LiveSeries {
  name: string;
  producer: DatasetProducer;
  session: Client["session"];
  entity: bigint;
  spatial: boolean;
}

/** Deterministic full-source edits. Prepare row values before timing. The measured update includes wire encoding. */
export async function chartUpdates(
  host: HostClientBase<Client>,
  series: readonly LiveSeries[],
) {
  assert.ok(series.length);
  const sources = await Promise.all(
    series.map(async (source) => {
      const first = await host.datasets.read(source.name);
      assert.equal(first.incarnation, source.producer.incarnation);
      const rows = [...first.rows];
      let offset = first.nextOffset;
      while (offset !== null) {
        const page = await host.datasets.read(source.name, {
          incarnation: first.incarnation,
          offset,
        });
        rows.push(...page.rows);
        offset = page.nextOffset;
      }
      assert.ok(rows.length);
      const y = first.schema.findIndex((column) => column.name === "y");
      assert.ok(y >= 0);
      const changing = first.schema.map(({ name }) =>
        ["y", "y2", "lower"].includes(name),
      );
      return { ...source, rows, y, changing };
    }),
  );
  return {
    rowCount: sources.reduce((sum, s) => sum + s.rows.length, 0),
    changes(cycle: number) {
      const updates = sources.map((s) => {
        const deltas = s.rows.map(({ id, values }, index) => {
          const factor = 0.8 + 0.32 * Math.sin(cycle * 1.19 + index * 0.17);
          return {
            operation: "edit" as const,
            row: id,
            values: values.map((v, i) =>
              s.changing[i] && v.kind === "f32"
                ? { kind: "f32" as const, value: Math.fround(v.value * factor) }
                : v,
            ),
          };
        });
        const expected = deltas[0]!.values[s.y]!;
        assert.ok(expected.kind === "f32");
        return { source: s, deltas, expected: expected.value };
      });
      return async () => {
        // The dataset transport delivers updates in order and bounds queued transfers.
        // Apply backpressure when the full board changes several sources together.
        for (const { source, deltas } of updates) {
          const result = await host.datasets.update(source.producer, deltas);
          assert.equal(result.failure, undefined);
          assert.equal(result.committedDeltas, BigInt(deltas.length));
        }
        const committed = performance.now();
        await Promise.all(
          updates.map(async ({ source, expected }) => {
            const deadline = performance.now() + 10_000;
            for (;;) {
              const page = await host.datasets.bindingView(
                source.session,
                source.entity,
                { limit: 1 },
              );
              const column = page.columns.findIndex(
                (c) => c.name === (source.spatial ? "value" : "y"),
              );
              const cell = page.rows[0]?.values[column];
              if (
                !page.dirty &&
                page.availability.reason === "Ready" &&
                cell?.valid &&
                cell.value.kind === "f32" &&
                cell.value.value === expected
              ) {
                assert.equal(
                  page.sourceIncarnation,
                  source.producer.incarnation,
                );
                assert.equal(page.totalRows, BigInt(source.rows.length));
                assert.equal(page.rows[0]!.id, source.rows[0]!.id);
                return;
              }
              assert.ok(
                performance.now() < deadline,
                "Changed source did not reach binding " + source.name,
              );
              await delay(1);
            }
          }),
        );
        return committed;
      };
    },
  };
}
