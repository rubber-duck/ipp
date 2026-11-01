/** Real area/band coverage, row identity and shared-shape combinations. */
import type {
  Client,
  HostClientBase,
  PickingWorldClient,
  RootBinding,
  RowPropertyValue,
} from "@ipp/client";
import {
  componentFields,
  insertComponent,
  successfulBatch,
} from "../../../examples/demos/scenes/charts/shared/commands.js";
import { encodeShaderDefinition } from "../../../packages/ipp-client/src/dynamic-properties.js";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import { check } from "../../harness/page/checks.js";
import { openWorkload, type ChartContract } from "./chart-data-workload.js";
import { exerciseStrokeSurfaces } from "./stroke-surfaces.js";

type Frame = { width: number; height: number; pixels: Uint8Array };

export async function exercisePlotFills(
  host: HostClientBase<Client>,
  contract: ChartContract,
  font: Uint8Array<ArrayBuffer>,
  capture: (label: string, binding: RootBinding) => Promise<Frame>,
  record: (label: string, value: unknown) => Promise<void>,
  recover?: () => Promise<void>,
) {
  const fixture = await openWorkload(
    host,
    contract,
    font,
    {
      name: "filled-lines",
      component: "PlotLine2d",
      rows: 6,
      bindings: 1,
      labels: 0,
    },
    "integration",
  );
  const { client, binding } = fixture,
    entity = fixture.entities[0]!;
  let cleared = false;
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
  const series = async (rows: Record<string, RowPropertyValue>[]) =>
    successfulBatch(
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: entity },
          component: client.components.PlotLine2d!.id,
          field: {
            offset: client.components.PlotLine2d!.fields.series!.offset,
            value: {
              kind: "rows",
              value: contract.encodeRowsTable(
                client.components.PlotLine2d!.fields.series!.rows!,
                {
                  nextSlot: rows.length,
                  rows: new Map(rows.map((r, i) => [i, r])),
                },
              ),
            },
          },
        },
      ]),
    );
  const cyan = {
    name: "filled",
    z: "z",
    value: "y",
    radius: "",
    height: "",
    color_column: "",
    x: "x",
    y: "y",
    lower: "z",
    color: [0, 0.8, 1, 1] as const,
    visible: true,
  };
  const edit = async (
    x: readonly number[],
    upper: readonly number[],
    lower: readonly number[],
  ) =>
    fixture.editRows(
      x.map((v, i) =>
        [v, upper[i]!, lower[i]!].map((value) => ({
          kind: "f32" as const,
          value,
        })),
      ),
    );
  const pixel = (frame: Frame, x: number, y: number) => {
    const offset =
      (Math.round(8 + 56 + 548 * x) +
        Math.round(8 + 336 - 156 * y) * frame.width) *
      4;
    return [...frame.pixels.slice(offset, offset + 4)];
  };
  try {
    await fixture.verify();
    await edit(
      [0, 0.2, 0.4, 0.6, 0.8, 1],
      [1.4, 1.4, 1.4, 1.4, 1.4, 1.4],
      [0.6, 0.6, 0.6, 0.6, 0.6, 0.6],
    );
    await series([cyan]);
    await set("PlotFrame2d", { grid_alpha: 0, alpha: 0 });
    await set("PlotLine2d", {
      line_width: 0,
      marker_size: 0,
      fill_opacity: 0.35,
    });
    const rectangle = await capture("band-union", binding);
    const background = [...rectangle.pixels.slice(0, 4)];
    const quiet = (p: number[]) =>
      p.every((v, i) => Math.abs(v - background[i]!) <= 2);
    for (let x = 0.03; x < 0.98; x += 0.013) {
      const p = pixel(rectangle, x, 1);
      check(
        p[0]! < 10 && p[2]! > 150 && p[2]! < 170,
        `Band seam/alpha at ${x}: ${p}`,
      );
    }
    check(
      quiet(pixel(rectangle, 0.3, 1.7)) && quiet(pixel(rectangle, 0.3, 0.3)),
      "Band escaped lower/upper boundary",
    );
    // Authored paint sees the fill's exterior, never triangulation seams.
    const asset = clientAssetSource(client.session, 13, 1000n);
    const bytes = encodeShaderDefinition({
      parameters: { tint: "vec3" },
      backends: {
        ipp: {
          paint: "return vec4(edge > -1.0 ? vec3(1.0) : p_tint, color.a);",
        },
      },
    });
    await client.registerAsset(asset, bytes.buffer);
    successfulBatch(
      await client.batch([
        insertComponent(
          client,
          "CanvasPaint",
          { kind: "handle", id: entity },
          { source: asset.source },
        ),
        {
          kind: "setDynamicProperty",
          entity: { kind: "handle", id: entity },
          component: client.components.CanvasPaint!.id,
          name: "tint",
          value: { kind: "vec3", value: [1, 0, 0] },
        },
      ]),
    );
    let painted: Frame | undefined;
    const paintDeadline = performance.now() + 30000;
    do {
      painted = await capture("band-painted", binding);
      if (pixel(painted, 0.3, 1)[0]! > 150) break;
      check(performance.now() < paintDeadline, "Fill paint never became ready");
      await client.waitForFrame();
    } while (true);
    for (let x = 0.03; x < 0.98; x += 0.013) {
      const p = pixel(painted, x, 1);
      check(
        p[0]! > 150 &&
          p[1]! <= background[1]! + 2 &&
          p[2]! <= background[2]! + 2,
        `Internal triangle edge leaked into paint: ${p}`,
      );
    }
    const exterior = pixel(painted, 0.3, 1.4);
    check(
      exterior[1]! > 80 && exterior[2]! > 80,
      `Paint lost the exterior contour: ${exterior}`,
    );
    await edit(
      [0, 0.7, 0.2, 0.8, 0.8, 1],
      Array(6).fill(1.4),
      Array(6).fill(0.6),
    );
    const paintedOverlap = await capture("band-painted-overlap", binding);
    for (const x of [0.2, 0.7, 0.8]) {
      const p = pixel(paintedOverlap, x, 1);
      check(
        p[0]! > 150 &&
          p[1]! <= background[1]! + 2 &&
          p[2]! <= background[2]! + 2,
        `Overlapping interval leaked a paint boundary: ${p}`,
      );
    }
    if (recover) {
      await recover();
      const restored = await capture("band-painted-restored", binding);
      for (const x of [0.1, 0.3, 0.5, 0.7, 0.9])
        check(
          pixel(restored, x, 1).every(
            (v, i) => Math.abs(v - pixel(painted!, x, 1)[i]!) < 3,
          ),
          "Restored fill/paint changed coverage",
        );
    }
    successfulBatch(
      await client.batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id: entity },
          component: client.components.CanvasPaint!.id,
        },
      ]),
    );
    // Reversals and repeated X use the same renderer and union alpha.
    await edit(
      [0, 0.8, 0.2, 0.9, 0.9, 1],
      [1.4, 1.4, 1.4, 1.4, 1.4, 1.4],
      [0.6, 0.6, 0.6, 0.6, 0.6, 0.6],
    );
    const overlap = await capture("band-reversed-overlap", binding);
    for (const x of [0.1, 0.3, 0.5, 0.7, 0.95])
      check(
        Math.abs(pixel(overlap, x, 1)[2]! - pixel(rectangle, x, 1)[2]!) < 3,
        "Overlapping intervals blended repeatedly",
      );
    const pick = await (client as unknown as PickingWorldClient).query({
      type: "GeometryPickQuery",
      view: { kind: "bound", binding },
      x: (8 + 56 + 548 * 0.5) / 640,
      y: (8 + 336 - 156) / 400,
      includeViewPlane: false,
    });
    check(
      pick.ok && pick.hit?.row?.rowId === 3n && pick.hit.row.series === 0,
      "Filled interval lost painter-order source identity",
    );
    await record("band-pick", pick);
    // Separate series still blend independently in authored series order.
    await series([cyan, { ...cyan, color: [1, 0, 1, 1] }]);
    const multi = await capture("band-multiple-series", binding);
    check(
      pixel(multi, 0.5, 1)[0]! > 150 && pixel(multi, 0.5, 1)[2]! > 195,
      "Multiple filled series absent or unioned together",
    );
    await series([{ ...cyan, lower: "" }]);
    await set("PlotLine2d", { fill_baseline: 1 });
    await edit(
      [0, 0.2, 0.4, 0.6, 0.8, 1],
      [1.8, 0.2, 1.8, 0.2, 1.8, 0.2],
      [0, 0, 0, 0, 0, 0],
    );
    const crossing = await capture("area-baseline-crossings", binding);
    check(
      pixel(crossing, 0.02, 1.3)[2]! > 150 &&
        pixel(crossing, 0.18, 0.7)[2]! > 150,
      "Baseline lobes absent",
    );
    check(
      quiet(pixel(crossing, 0.1, 1.4)) && quiet(pixel(crossing, 0.1, 0.6)),
      "Crossing filled a bow-tie exterior",
    );
    await set("PlotLine2d", {
      interpolation: 1,
      line_width: 3,
      marker_size: 9,
      fill_opacity: 0.7,
    });
    const smooth = await capture("area-smooth-markers", binding);
    check(
      pixel(smooth, 0.02, 1.3)[2]! > 200,
      "Smooth area with markers disappeared",
    );
    // A thin rectangular band has an independently known integrated alpha even
    // when it is narrower than a pixel. Decode sRGB before integrating coverage.
    await series([cyan]);
    await set("PlotLine2d", {
      interpolation: 0,
      line_width: 0,
      marker_size: 0,
      fill_opacity: 0.35,
    });
    await edit(
      [0, 0.2, 0.4, 0.6, 0.8, 1],
      Array(6).fill(1.01),
      Array(6).fill(1),
    );
    const linear = (byte: number) => {
      const s = byte / 255;
      return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    };
    // Minified coverage keeps opacity and supports independent X/Y footprints.
    for (const [scale_x, scale_y] of [
      [0.1, 1],
      [1, 0.1],
    ] as const) {
      await set("CanvasStyle", { scale_x, scale_y });
      const thin = await capture(
        `area-minified-${scale_x}-${scale_y}`,
        binding,
      );
      const column = Math.round(8 + scale_x * (56 + 548 * 0.5));
      const center = 8 + scale_y * (336 - 156 * 1.005);
      const backgroundBlue = linear(thin.pixels[2]!);
      let alpha = 0;
      for (let y = Math.floor(center) - 4; y <= Math.ceil(center) + 4; y++) {
        const blue = linear(thin.pixels[(y * thin.width + column) * 4 + 2]!);
        alpha += (blue - backgroundBlue) / (1 - backgroundBlue);
      }
      const expected = 0.35 * 156 * 0.01 * scale_y;
      check(
        Math.abs(alpha - expected) < 0.015 + expected * 0.12,
        `Minified band alpha ${alpha}, expected ${expected}`,
      );
    }
    await set("CanvasStyle", { scale_x: 1, scale_y: 1 });
    await set("PlotLine2d", {
      interpolation: 1,
      line_width: 3,
      marker_size: 9,
      fill_opacity: 0.7,
    });
    await edit(
      [0, 0.2, 0.4, 0.6, 0.8, 1],
      [1.8, 0.2, 1.8, 0.2, 1.8, 0.2],
      [0.5, 0.5, 0.5, 0.5, 0.5, 0.5],
    );
    // Existing Surface fixture compares direct coverage with a high-resolution
    // retained image under cylinder/sphere mapping and both eye positions.
    await host.clearRootOutput(binding);
    cleared = true;
    await exerciseStrokeSurfaces(host, client, capture, record);
  } finally {
    if (!cleared) await host.clearRootOutput(binding).catch(() => {});
    await fixture.close();
  }
  await exerciseSubpixelFillIntervals(host, contract, font, capture);
  await exercisePieCoverage(host, contract, font, capture);
}

/** Many disjoint intervals in one pixel must retain their whole alpha union. */
async function exerciseSubpixelFillIntervals(
  host: HostClientBase<Client>,
  contract: ChartContract,
  font: Uint8Array<ArrayBuffer>,
  capture: (label: string, binding: RootBinding) => Promise<Frame>,
) {
  const fixture = await openWorkload(
    host,
    contract,
    font,
    {
      name: "subpixel-fill-intervals",
      component: "PlotLine2d",
      rows: 65,
      bindings: 1,
      labels: 0,
    },
    "integration",
  );
  const { client, binding } = fixture;
  const set = async (
    component: string,
    values: Parameters<typeof componentFields>[2],
  ) =>
    successfulBatch(
      await client.batch(
        componentFields(client, component, values).map((field) => ({
          kind: "setField" as const,
          entity: { kind: "handle" as const, id: fixture.entities[0]! },
          component: client.components[component]!.id,
          field,
        })),
      ),
    );
  const linear = (byte: number) => {
    const s = byte / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  try {
    await fixture.verify();
    await fixture.editRows(
      Array.from({ length: 65 }, (_, i) =>
        [i / 64, i % 2 ? 1.5 : 0.5, 0].map((value) => ({
          kind: "f32" as const,
          value,
        })),
      ),
    );
    await set("PlotFrame2d", { grid_alpha: 0, alpha: 0 });
    await set("PlotLine2d", {
      line_width: 0,
      marker_size: 0,
      interpolation: 0,
      fill_baseline: 1,
      fill_opacity: 0.35,
    });
    for (const scale_x of [0.001, -0.001]) {
      await set("CanvasStyle", { x: scale_x > 0 ? 8 : 10, scale_x });
      const frame = await capture(
        `area-32-subpixel-intervals-${scale_x}`,
        binding,
      );
      const background = linear(frame.pixels[2]!);
      // Thirty-two triangular peaks occupy a known fraction of each horizontal
      // scanline. Their total width is below one pixel, with more separate
      // intervals than the shader's local scratch capacity, also when mirrored.
      for (const row of [140, 150, 160]) {
        let alpha = 0;
        for (let x = 6; x <= 12; x++) {
          const blue = linear(frame.pixels[(row * frame.width + x) * 4 + 2]!);
          alpha += (blue - background) / (1 - background);
        }
        const dataY = (8 + 336 - (row + 0.5)) / 156;
        const expected = 0.35 * 548 * Math.abs(scale_x) * (1.5 - dataY);
        check(
          Math.abs(alpha - expected) < 0.008 + expected * 0.12,
          `Subpixel interval alpha ${alpha}, expected ${expected}, row ${row}, scale ${scale_x}`,
        );
      }
    }
  } finally {
    await host.clearRootOutput(binding).catch(() => {});
    await fixture.close();
  }
}

/** Narrow sectors cover their bisector without losing tips, holes or mirrored picking. */
async function exercisePieCoverage(
  host: HostClientBase<Client>,
  contract: ChartContract,
  font: Uint8Array<ArrayBuffer>,
  capture: (label: string, binding: RootBinding) => Promise<Frame>,
) {
  const fixture = await openWorkload(
    host,
    contract,
    font,
    {
      name: "narrow-pie",
      component: "PlotPie2d",
      rows: 96,
      bindings: 1,
      labels: 0,
    },
    "integration",
  );
  const { client, binding } = fixture,
    entity = fixture.entities[0]!;
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
  try {
    await fixture.editRows(
      Array.from({ length: 96 }, (_, i) =>
        [i / 95, 1, 0].map((value) => ({ kind: "f32" as const, value })),
      ),
    );
    await set("PlotFrame2d", {
      width: 300,
      height: 300,
      padding_left: 0,
      padding_top: 0,
      padding_right: 0,
      padding_bottom: 0,
    });
    // Asset loading and data projection are asynchronous. Two identical empty
    // frames can settle before the expressions/data become ready on a fast Host.
    const deadline = performance.now() + 30000;
    for (;;) {
      const view = await host.datasets.bindingView(client.session, entity, {
        limit: 1,
      });
      const value =
        view.rows[0]?.values[
          view.columns.findIndex((column) => column.name === "y")
        ];
      if (
        view.availability.reason === "Ready" &&
        !view.dirty &&
        value?.valid &&
        value.value.kind === "f32" &&
        value.value.value === 1
      )
        break;
      check(
        performance.now() < deadline,
        "Equal pie values never became ready",
      );
      await client.waitForFrame();
    }
    for (const mirror of [1, -1])
      for (const inner_radius of [0, 0.55]) {
        await set("CanvasStyle", {
          x: mirror === 1 ? 10 : 310,
          y: 20,
          scale_x: mirror,
        });
        await set("PlotPie2d", { inner_radius });
        const frame = await capture(
          `narrow-pie-${mirror}-${inner_radius}`,
          binding,
        );
        const pixel = (x: number, y: number) => [
          ...frame.pixels.slice(
            (Math.round(x) + Math.round(y) * frame.width) * 4,
            (Math.round(x) + Math.round(y) * frame.width) * 4 + 4,
          ),
        ];
        const point = (slice: number, radius: number) => {
          const angle = ((slice + 0.5) / 96) * Math.PI * 2;
          return [
            160 + mirror * 150 * radius * Math.sin(angle),
            170 - 150 * radius * Math.cos(angle),
          ] as const;
        };
        for (let slice = 0; slice < 96; slice++)
          for (const radius of [0.65, 0.9]) {
            const p = pixel(...point(slice, radius));
            check(
              p[0]! < 10 && p[2]! > 235,
              `Narrow sector clipped at ${slice}/${radius}: ${p}`,
            );
          }
        const background = pixel(0, 0);
        const quiet = (p: number[]) =>
          p.every((v, i) => Math.abs(v - background[i]!) <= 2);
        check(quiet(pixel(...point(7, 1.08))), "Pie escaped its circle");
        if (inner_radius > 0) check(quiet(pixel(160, 170)), "Pie hole filled");
        const [x, y] = point(7, 0.8);
        const picked = await (client as unknown as PickingWorldClient).query({
          type: "GeometryPickQuery",
          view: { kind: "bound", binding },
          x: x / 640,
          y: y / 400,
          includeViewPlane: false,
        });
        check(
          picked.ok && picked.hit?.row?.rowId === 8n,
          "Narrow sector lost mirrored source identity",
        );
      }
  } finally {
    await host.clearRootOutput(binding).catch(() => {});
    await fixture.close();
  }
}
