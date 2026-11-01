/** Ordinary-build Plot throughput, separated from capture and diagnostics. */
import type {
  Client,
  HostClientBase,
  RootBinding,
  RowPropertyValue,
} from "@ipp/client";
import { createWorkerHost } from "../../../packages/ipp-client/src/worker.js";
import { renderDiagnostics } from "../../../packages/ipp-client/src/diagnostics.js";
import {
  componentFields,
  insertComponent,
  successfulBatch,
} from "../../../examples/demos/scenes/charts/shared/commands.js";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import { encodeShaderDefinition } from "../../../packages/ipp-client/src/dynamic-properties.js";
import {
  openWorkload,
  type ChartContract,
} from "../../data/scenarios/chart-data-workload.js";

export async function open(runtime: string, count: number, name: string) {
  const contract = await import(`${runtime}/generated.js`);
  const canvas = document.createElement("canvas");
  canvas.width = 1600;
  canvas.height = 1000;
  document.body.replaceChildren(canvas);
  const owner = createWorkerHost(
    `${runtime}/wasm-worker.js`,
    `${runtime}/runtime.wasm`,
    contract.MAX_MESSAGE_BYTES,
    { canvas: canvas.transferControlToOffscreen() },
  );
  const host: HostClientBase<Client> =
    await contract.IppHostClient.connectTransport(owner.connect());
  let fixture: Awaited<ReturnType<typeof openWorkload>> | undefined;
  let binding: RootBinding | undefined;
  try {
    const font = new Uint8Array(
      await (
        await fetch("/target/font-assets/shure-tech-mono.ippf")
      ).arrayBuffer(),
    );
    // Comparison artifacts carry their own generated contract. Filter only the
    // benchmark's row declarations to that exact layout; runtime APIs stay strict.
    const compatible: ChartContract = {
      ...contract,
      encodeRowsTable: (layout, table) =>
        contract.encodeRowsTable(layout, {
          ...table,
          rows: new Map(
            [...table.rows].map(([slot, row]) => [
              slot,
              Object.fromEntries(
                layout.properties
                  .filter((p) => Object.hasOwn(row, p.name))
                  .map((p) => [
                    p.name,
                    (row as Record<string, unknown>)[p.name],
                  ]),
              ),
            ]),
          ),
        }),
    };
    const component =
      name === "bars"
        ? "PlotBars2d"
        : name === "pie"
          ? "PlotPie2d"
          : "PlotLine2d";
    fixture = await openWorkload(
      host,
      compatible,
      font,
      {
        name: `coverage-${name}-${count}`,
        component,
        rows: count,
        bindings: 1,
        labels: 0,
      },
      "performance",
    );
    const client = fixture.client,
      entity = fixture.entities[0]!;
    const filled = [
      "area",
      "band",
      "filled-markers",
      "multiband",
      "painted-band",
    ].includes(name);
    if (filled && !client.components.PlotLine2d!.fields.fill_opacity) {
      await host.clearRootOutput(fixture.binding);
      await fixture.close();
      await host.close();
      await owner.close();
      return {
        unsupported: "Fill authoring is absent in this comparison build",
      };
    }
    const set = async (
      component: string,
      values: Parameters<typeof componentFields>[2],
    ) =>
      successfulBatch(
        await client.batch(
          componentFields(client, component, values).map((field) => ({
            kind: "setField" as const,
            entity: { kind: "handle" as const, id: entity },
            component: client.components[component]!.id,
            field,
          })),
        ),
      );
    const colors = [
      [0, 0.8, 1, 1],
      [1, 0.1, 0.3, 1],
      [0.2, 1, 0.2, 1],
      [1, 0.7, 0, 1],
    ];
    const rows: Record<string, RowPropertyValue>[] = Array.from(
      { length: name === "multiline" || name === "multiband" ? 4 : 1 },
      (_, i) => ({
        name: `series-${i}`,
        x: "x",
        y: !filled && i % 2 ? "z" : "y",
        z: "z",
        value: "y",
        lower: name.includes("band") ? "z" : "",
        radius: "",
        height: "",
        color_column: "",
        color: colors[i]!,
        visible: true,
      }),
    );
    const layout = client.components[component]!.fields.series!.rows!;
    successfulBatch(
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: entity },
          component: client.components[component]!.id,
          field: {
            offset: client.components[component]!.fields.series!.offset,
            value: {
              kind: "rows",
              value: compatible.encodeRowsTable(layout, {
                nextSlot: rows.length,
                rows: new Map(rows.map((row, i) => [i, row])),
              }),
            },
          },
        },
      ]),
    );
    await set("PlotFrame2d", {
      width: 1584,
      height: 984,
      grid_alpha: 0,
      alpha: 0,
    });
    if (component === "PlotLine2d")
      await set(component, {
        line_width: 2,
        interpolation: 1,
        marker_size: name.includes("markers") ? 6 : 0,
        ...(filled ? { fill_opacity: 0.35, fill_baseline: 0 } : {}),
      });
    let paintSource: string | undefined;
    if (name === "painted-band") {
      const asset = clientAssetSource(client.session, 13, 1000n);
      const bytes = encodeShaderDefinition({
        parameters: {},
        backends: {
          ipp: {
            paint:
              "return vec4(mix(color.rgb, vec3(1.0), smoothstep(-2.0, 0.0, edge)), color.a);",
          },
        },
      });
      paintSource = asset.source;
      await client.registerAsset(asset, bytes.buffer);
      successfulBatch(
        await client.batch([
          insertComponent(
            client,
            "CanvasPaint",
            { kind: "handle", id: entity },
            { source: asset.source },
          ),
        ]),
      );
    }
    binding = await host.setRootOutput(fixture.binding.output, {
      width: 1600,
      height: 1000,
      devicePixelRatio: 1,
    });
    const surface = await host.presentation.surface();
    const view = await host.presentation.select(surface, binding);
    await fixture.verify();
    const deadline = performance.now() + 30000;
    for (;;) {
      const state = await host.datasets.bindingView(client.session, entity, {
        limit: 1,
      });
      if (state.availability.reason === "Ready" && !state.dirty) break;
      if (performance.now() > deadline)
        throw new Error("Plot never became ready");
      await client.waitForFrame();
    }
    if (paintSource) {
      for (;;) {
        const state = await client.inspect();
        const asset = state.resources.find(
          (resource) => resource.source === paintSource,
        );
        if (asset?.status === "failed") throw new Error(asset.error);
        if (asset?.status === "loaded") break;
        if (performance.now() > deadline)
          throw new Error("Plot paint never became ready");
        await client.waitForFrame();
      }
    }
    await host.presentation.frame(view);
    const inspection = await client.inspect();
    if (inspection.renderDiagnostics.length)
      throw new Error(
        JSON.stringify(inspection.renderDiagnostics, (_, v) =>
          typeof v === "bigint" ? String(v) : v,
        ),
      );
    return {
      device: (await renderDiagnostics(host)!.statistics()).device,
      async sample(milliseconds: number) {
        const first = await host.presentation.frame(view);
        const start = performance.now();
        await new Promise((resolve) => setTimeout(resolve, milliseconds));
        const last = await host.presentation.frame(view);
        const durationMs = performance.now() - start;
        if (first.failedDrawCalls || last.failedDrawCalls)
          throw new Error("Failed benchmark draw");
        return {
          durationMs,
          frames: Number(last.sequence - first.sequence),
          fps: (Number(last.sequence - first.sequence) * 1000) / durationMs,
          drawCalls: last.drawCalls,
          triangles: last.triangles,
          failedDrawCalls: last.failedDrawCalls,
        };
      },
      async statistics() {
        return renderDiagnostics(host)!.statistics();
      },
      async close() {
        await host.presentation.clear(view);
        await host.clearRootOutput(binding!);
        await fixture!.close();
        await host.close();
        await owner.close();
      },
    };
  } catch (error) {
    if (fixture) {
      await host.clearRootOutput(binding ?? fixture.binding).catch(() => {});
      await fixture.close().catch(() => {});
    }
    await host.close().catch(() => {});
    await owner.close();
    throw error;
  }
}
