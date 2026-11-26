import { readFile } from "node:fs/promises";
import { join } from "node:path";
import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import type * as Generated from "@ipp/host-contract";
import {
  canvasOutput,
  type DatasetProducer,
  type PresentationView,
  type GuiPhysicalContext,
  type RootBinding,
} from "@ipp/client";
import { createRoot, type ReactWorldClient } from "@ipp/react";
import {
  closePlot3d,
  openPlot3d,
  readyPlot3d,
} from "../../../examples/demos/scenes/charts3d/content.js";
import type { ClientContext } from "../../../tools/shared-host/client.js";
import { chartRows, chartSchema } from "./gui-charts.js";
import {
  projectBoard,
  projectPixel,
  boardWidthMetres,
} from "./gui-projection.js";
import { chartUpdates } from "./gui-chart-updates.js";
import { chartCaseName, isSpatialCase, type BoardCase } from "./gui-cases.js";
import {
  boardSkins,
  bodyTop,
  GuiBoard,
  selectSlots,
  viewport,
} from "./gui-board.js";

export async function openGuiCase(
  context: ClientContext,
  { name, theme, projection }: BoardCase,
  font: Uint8Array<ArrayBuffer>,
) {
  const { host } = context;
  const contract = context.contract as unknown as typeof Generated;
  const authored = boardSkins[theme](contract);
  // Cartoon chooses from these authored tilts on mount. Use the same tilt for every comparison.
  const skin = authored.notificationAngles?.length
    ? { ...authored, notificationAngles: [authored.notificationAngles[0]!] }
    : authored;
  const live = name.endsWith("-live");
  if (isSpatialCase(name)) {
    const chartScene = await openPlot3d(
      host,
      contract,
      font,
      context.name + "-" + name,
      skin.tokens,
      {
        onlyChart: chartCaseName(name).slice(3).replace(/-1k$/, ""),
        skin,
        ...(chartCaseName(name) === "3d-height-surface-1k"
          ? { surfaceDivisions: 31 }
          : {}),
        ...(chartCaseName(name) === "3d-point-plot-1k"
          ? { pointRows: 1024 }
          : {}),
      },
    );
    const chart = chartScene.charts[0]!;
    return {
      binding: chart.binding,
      ready: () => readyPlot3d(chartScene),
      ...(live
        ? {
            updates: () =>
              chartUpdates(host, [
                {
                  name: chart.source,
                  producer: chart.producer,
                  session: chart.client.session,
                  entity: chart.entity,
                  spatial: true,
                },
              ]),
          }
        : {}),
      close: async () => {
        try {
          await host.clearRootOutput(chart.binding);
        } finally {
          await closePlot3d(chartScene);
        }
      },
    };
  }
  const selected = selectSlots(name);
  const wave = Uint8Array.from(
    await readFile(join(context.assetRoot, "demos-drawings/waveform.ippd")),
  );
  const world = await host.createWorld({
    symbolicId: context.name + "/" + name,
    temporary: true,
    canvas: {
      extent: [viewport.width, viewport.height],
      unitsPerMetre: viewport.width / boardWidthMetres,
    },
    selectedSystems: [
      "ipp.animation",
      "ipp.gui",
      "ipp.gui-layout",
      "ipp.canvas",
      "ipp.plot",
      "ipp.data-bindings",
      "ipp.asset-dependencies",
      "ipp.lifecycle-publisher",
    ],
  });
  const client = await host.openWorld(world.reference);
  const errors: Error[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: (e) => errors.push(e),
  });
  const producers: DatasetProducer[] = [];
  const sources = new Map<string, string>();
  let binding: RootBinding | undefined;
  let parent: Awaited<ReturnType<typeof projectBoard>> | undefined;
  let input: GuiPhysicalContext | undefined;
  const draw = () =>
    root.render(
      <GuiBoard
        skin={skin}
        name={name}
        contract={contract}
        font={font}
        wave={wave}
        sources={sources}
        {...(input ? { input } : {})}
      />,
    );

  // Fence each wheel on the newly realized virtual rows, then on a presented
  // frame. The latency includes the client observation and the frame request.
  async function scrollGrid(view: PresentationView) {
    input = await host.input.open(view);
    const slot = selected[0]!;
    const [x, y] = projectPixel(
      slot.x + 64,
      slot.y + bodyTop + 60,
      viewport,
      projection,
    );
    const point = [x / viewport.width, y / viewport.height] as const;
    const firstRow = async () => {
      const rows = (await client.inspect()).entities.flatMap((entity) => {
        const row = /\/item\/(\d+)$/.exec(entity.metadata.symbolicId ?? "");
        return row ? [Number(row[1])] : [];
      });
      return rows.length ? Math.min(...rows) : undefined;
    };
    assert.equal(await firstRow(), 0, "Grid did not realize its first row");
    const latencies: number[] = [];
    let sequence = 0n;
    let furthestFirstRow = 0;
    for (let sample = 0; sample < 16; sample++) {
      const down = sample % 2 === 0;
      const start = performance.now();
      assert.equal(
        (
          await input.send({
            kind: "wheel",
            point,
            delta: [0, down ? 144 : -144],
          })
        ).disposition,
        "routed",
        "Wheel did not reach the grid",
      );
      for (;;) {
        const row = await firstRow();
        if (row !== undefined && (down ? row > 0 : row === 0)) {
          furthestFirstRow = Math.max(furthestFirstRow, row);
          break;
        }
        assert.ok(
          performance.now() - start < 5000,
          "Virtual rows did not follow the wheel",
        );
        await delay(2);
      }
      const shown = await host.presentation.frame(view, {
        afterSequence: sequence,
        afterOutputs: [canvasOutput(world.reference)],
      });
      assert.equal(shown.failedDrawCalls, 0);
      sequence = shown.sequence;
      latencies.push(performance.now() - start);
    }
    return { latencies, furthestFirstRow };
  }

  const close = async () => {
    const failures: unknown[] = [];
    for (const release of [
      () => input?.close(),
      () => binding && host.clearRootOutput(binding),
      () => root.unmount(),
      () => client.close(),
      () => parent?.close(),
      () => host.destroyWorld(world.reference),
      ...producers.map((producer) => () => host.datasets.destroy(producer)),
    ]) {
      try {
        await release();
      } catch (error) {
        failures.push(error);
      }
    }
    if (failures.length)
      throw new AggregateError(failures, "GUI case cleanup failed");
  };
  try {
    for (const { id } of selected.filter((s) => s.family === "charts")) {
      const source = "datasets://gui-rendering/" + name + "/" + id;
      const producer = await host.datasets.create(
        source,
        "buffer",
        chartSchema,
      );
      producers.push(producer);
      sources.set(id, source);
      const count =
        id === "pie"
          ? 9
          : id === "bars"
            ? 24
            : chartCaseName(name).endsWith("-1k")
              ? 1024
              : 128;
      const result = await host.datasets.update(producer, [
        { operation: "append", rows: chartRows(count) },
      ]);
      assert.ok(!result.failure, "Dataset ingestion failed for " + id);
    }
    await draw();
    if (errors.length) throw errors[0];
    parent =
      projection === "canvas"
        ? undefined
        : await projectBoard(host, world.reference, projection, viewport);
    binding = await host.setRootOutput(
      parent?.output ?? canvasOutput(world.reference),
      viewport,
    );
    return {
      binding,
      close,
      ...(live
        ? {
            updates: async () => {
              const entities = (await client.inspect()).entities;
              return chartUpdates(
                host,
                [...sources.entries()].map(([id, source], i) => {
                  const entity = entities.find(
                    (e) => e.metadata.symbolicId === "chart-" + id,
                  );
                  assert.ok(entity);
                  return {
                    name: source,
                    producer: producers[i]!,
                    session: client.session,
                    entity: entity.id,
                    spatial: false,
                  };
                }),
              );
            },
          }
        : {}),
      ...(name === "data-grid-scroll" ? { scrollGrid } : {}),
      async ready(view: PresentationView) {
        let charts: bigint[] = [];
        const deadline = performance.now() + 30000;
        for (;;) {
          await root.flush();
          if (errors.length) throw errors[0];
          if (charts.length < sources.size) {
            const snapshot = await client.inspect();
            charts = snapshot.entities
              .filter((e) =>
                [...sources.keys()].some(
                  (id) => e.metadata.symbolicId === "chart-" + id,
                ),
              )
              .map((e) => e.id);
          }
          const resources = (
            await client.inspectPage({ collection: "resources", limit: 256 })
          ).resources;
          const failed = resources.find((r) => r.status === "failed");
          assert.ok(!failed, failed?.error);
          const views = await Promise.all(
            charts.map((e) =>
              host.datasets.bindingView(client.session, e, { limit: 1 }),
            ),
          );
          if (
            charts.length === sources.size &&
            views.every((v) => v.availability.reason === "Ready" && !v.dirty) &&
            resources.some((r) => r.kind === 17 && r.status === "loaded") &&
            resources.every(
              (r) => r.status !== "progress" && r.status !== "start",
            )
          )
            break;
          assert.ok(
            performance.now() < deadline,
            "Resource or chart binding timed out in " + name,
          );
          await delay(16);
        }
        const popup = new Map([
          ["tooltip-open", "signal-tip"],
          ["hover-card-open", "station-card"],
          ["context-menu-open", "context-actions/surface"],
          ["trigger-menu-open", "mentions/popup"],
        ]).get(name);
        if (popup) {
          input = await host.input.open(view);
          if (name === "trigger-menu-open") await draw();
          await host.presentation.frame(view, {
            afterOutputs: [canvasOutput(world.reference)],
          });
          const [px, py] = projectPixel(
            24,
            name === "trigger-menu-open" ? 67 : 50,
            viewport,
            projection,
          );
          const point = [px / viewport.width, py / viewport.height] as const;
          await input.send({ kind: "pointerMove", pointer: 1n, point });
          if (name === "context-menu-open" || name === "trigger-menu-open") {
            const button =
              name === "context-menu-open" ? "secondary" : "primary";
            await input.send({
              kind: "pointerDown",
              pointer: 1n,
              point,
              button,
            });
            await input.send({ kind: "pointerUp", pointer: 1n, point, button });
          }
          const timeout = performance.now() + 5000;
          if (name === "trigger-menu-open") {
            while (!input.nativeText && performance.now() < timeout)
              await delay(10);
            assert.ok(
              input.nativeText,
              "Mention field did not acquire text input",
            );
            await input.editText(input.nativeText.fence, {
              kind: "text",
              text: "@n",
            });
          }
          for (;;) {
            const state = await client.inspect();
            const entity = state.entities.find(
              (e) => e.metadata.symbolicId === popup,
            );
            if (
              entity?.components.find(
                (c) => c.component === client.components.GuiBehavior!.id,
              )?.fields.visible === true
            )
              break;
            assert.ok(
              performance.now() < timeout,
              "Overlay did not open: " + popup,
            );
            await delay(25);
          }
        }
        if (client.manifest?.operations.includes("rendering"))
          assert.deepEqual(
            (await client.inspectPage({ collection: "renderDiagnostics" }))
              .renderDiagnostics,
            [],
          );
      },
    };
  } catch (error) {
    try {
      await close();
    } catch (cleanupError) {
      throw new AggregateError(
        [error, cleanupError],
        "GUI case setup and cleanup failed",
      );
    }
    throw error;
  }
}
