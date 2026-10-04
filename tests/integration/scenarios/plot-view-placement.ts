/** Real transported, completed-frame evidence for camera-only Plot presentation. */
import type {
  Client,
  HostClientBase,
  PickingWorldClient,
  RootBinding,
} from "@ipp/client";
import {
  openPlot3d,
  readyPlot3d,
  plot3dView,
  closePlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "../plot-3d-scene.js";
import {
  componentFields,
  createEntity,
  insertComponent,
  aliasId,
  successfulBatch,
} from "../../../examples/world-gallery/worlds/charts/shared/commands.js";
import { declarePlot } from "../../../examples/world-gallery/worlds/charts/shared/declare-plot.js";

interface Frame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}

type Capture = (label: string, binding: RootBinding) => Promise<Frame>;
type RecordResult = (label: string, value: unknown) => Promise<void>;
export interface PlotOffscreenCase {
  width: 427 | 1280;
  opposite: boolean;
}
const families = ["grid-bars", "height-surface", "variable-pie"] as const;
const ORTHO_HEIGHT = 17;

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function json(value: unknown): string {
  return JSON.stringify(value, (_, entry) =>
    typeof entry === "bigint" ? entry.toString() : entry,
  );
}

function angles(eye: readonly number[]) {
  const delta = eye.map((value, axis) => value - [5, 2, 5][axis]!);
  return {
    yaw: Math.atan2(delta[0]!, delta[2]!),
    pitch: -Math.atan2(delta[1]!, Math.hypot(delta[0]!, delta[2]!)),
  };
}

async function placeCamera(scene: Plot3dScene, eye: readonly number[]) {
  const { yaw, pitch } = angles(eye);
  const sy = Math.sin(yaw / 2),
    cy = Math.cos(yaw / 2);
  const sx = Math.sin(pitch / 2),
    cx = Math.cos(pitch / 2);
  for (const chart of scene.charts) {
    if (!families.includes(chart.name as (typeof families)[number])) continue;
    const pose = {
      x: eye[0]!,
      y: eye[1]!,
      z: eye[2]!,
      qx: cy * sx,
      qy: sy * cx,
      qz: -sy * sx,
      qw: cy * cx,
    };
    successfulBatch(
      await chart.client.batch(
        componentFields(chart.client, "Transform", pose).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id: chart.camera },
          component: chart.client.components.Transform!.id,
          field,
        })),
      ),
    );
  }
}

function colors(frame: Frame) {
  const count = { cyan: 0, amber: 0, magenta: 0 };
  for (let i = 0; i < frame.pixels.length; i += 4) {
    const r = frame.pixels[i]!,
      g = frame.pixels[i + 1]!,
      b = frame.pixels[i + 2]!;
    if (r < 100 && g > 100 && b > 100) count.cyan++;
    if (r > 150 && g > 80 && b < 120) count.amber++;
    if (r > 120 && b > 80 && g < 120) count.magenta++;
  }
  return count;
}

function difference(a: Frame, b: Frame) {
  check(a.width === b.width && a.height === b.height, "View extent changed");
  let count = 0;
  for (let i = 0; i < a.pixels.length; i += 4) {
    if (
      Math.abs(a.pixels[i]! - b.pixels[i]!) +
        Math.abs(a.pixels[i + 1]! - b.pixels[i + 1]!) +
        Math.abs(a.pixels[i + 2]! - b.pixels[i + 2]!) >
      30
    )
      count++;
  }
  return count;
}

interface Panel {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** Detect actual thin, closed callout borders, without renderer layout metadata. */
function calloutPanels(frame: Frame): Panel[] {
  const ink = (x: number, y: number) => {
    if (x < 0 || y < 0 || x >= frame.width || y >= frame.height) return false;
    const i = (y * frame.width + x) * 4;
    const r = frame.pixels[i]!,
      g = frame.pixels[i + 1]!,
      b = frame.pixels[i + 2]!;
    // Thin analytic borders have partial pixel coverage; the magenta edge can
    // fall below 140 while remaining clearly separate from the dim grid.
    return Math.max(r, g, b) > 100 && r + g + b > 180;
  };
  const runs: { left: number; right: number; y: number }[] = [];
  for (let y = 2; y < frame.height - 2; y++) {
    for (let x = 0; x < frame.width; x++) {
      if (!ink(x, y)) continue;
      const left = x;
      while (x + 1 < frame.width && ink(x + 1, y)) x++;
      if (x - left < 50) continue;
      runs.push({ left, right: x, y });
    }
  }
  const panels: Panel[] = [];
  for (const top of runs) {
    for (const bottom of runs) {
      const height = bottom.y - top.y;
      if (
        height < 12 ||
        height > 60 ||
        (Math.abs(top.left - bottom.left) > 2 &&
          Math.abs(top.right - bottom.right) > 2)
      )
        continue;
      // Adjacent geometry can extend one bright run past a real corner. Match
      // the shared corner and verify the closed border inside their intersection.
      const left = Math.max(top.left, bottom.left);
      const right = Math.min(top.right, bottom.right);
      if (right - left < 50) continue;
      const thin = (y: number) => {
        let count = 0;
        for (let sample = 1; sample <= 8; sample++) {
          const x = Math.round(left + ((right - left) * sample) / 9);
          if (!ink(x, y - 2) && !ink(x, y + 2)) count++;
        }
        return count >= 6;
      };
      // Filled slice faces have ink throughout one side of the segment.
      if (!thin(top.y) || !thin(bottom.y)) continue;
      let sides = 0;
      for (let y = top.y + 1; y < bottom.y; y++) {
        const edge = (x: number) => ink(x - 1, y) || ink(x, y) || ink(x + 1, y);
        if (edge(left) && edge(right)) sides++;
      }
      if (sides < (height - 1) * 0.8) continue;
      const panel = {
        left,
        right,
        top: top.y,
        bottom: bottom.y,
      };
      if (
        panels.some(
          (old) =>
            !(
              panel.right < old.left ||
              panel.left > old.right ||
              panel.bottom < old.top ||
              panel.top > old.bottom
            ),
        )
      )
        continue;
      panels.push(panel);
    }
  }
  return panels;
}

function projected(point: readonly number[], eye: readonly number[]) {
  const p = point.map((value, axis) => value - eye[axis]!);
  const { yaw, pitch } = angles(eye);
  const right = Math.cos(yaw) * p[0]! - Math.sin(yaw) * p[2]!;
  const up =
    Math.sin(yaw) * Math.sin(pitch) * p[0]! +
    Math.cos(pitch) * p[1]! +
    Math.cos(yaw) * Math.sin(pitch) * p[2]!;
  return {
    x: 0.5 + right / ((ORTHO_HEIGHT * 960) / 760),
    y: 0.5 - up / ORTHO_HEIGHT,
  };
}

function farGridPixels(frame: Frame, eye: readonly number[]) {
  // The upper rim stays above the fixture's tallest bar (4.25m). Its Z
  // station is independently selected from the authored camera side, not
  // obtained from renderer metadata or inferred from image differences.
  const station = [5.125, 5, eye[2]! > 5 ? 0 : 10];
  const screen = projected(station, eye);
  const x = Math.round(screen.x * frame.width);
  const y = Math.round(screen.y * frame.height);
  let contrast = 0;
  for (let dy = -3; dy <= 3; dy++) {
    for (let dx = -3; dx <= 3; dx++) {
      const px = x + dx,
        py = y + dy;
      if (px < 0 || py < 0 || px >= frame.width || py >= frame.height) continue;
      const i = (py * frame.width + px) * 4;
      const change = [0, 1, 2].reduce(
        (sum, channel) =>
          sum + Math.abs(frame.pixels[i + channel]! - frame.pixels[channel]!),
        0,
      );
      if (change > 20) contrast++;
    }
  }
  check(
    contrast >= 2,
    "Expected far-side grid rim is absent from the completed image",
  );
  return { station, screen, contrast };
}

/** Numeric ink must stay beside the actual visible vertical tick station. */
function verticalTickPixels(frame: Frame, eye: readonly number[]) {
  // Pick the left outer edge from independently projected fixture corners.
  // The fixed grid-bar frame is 10 x 5 x 10, with five 0..100 tick labels.
  const corners = [0, 10].flatMap((x) =>
    [0, 10].map((z) => ({ x, z, screen: projected([x, 0, z], eye) })),
  );
  corners.sort((a, b) => a.screen.x - b.screen.x);
  const edge = corners[0]!;
  const ticks = Array.from({ length: 5 }, (_, tick) => {
    const station = [edge.x, (tick * 5) / 4, edge.z];
    const screen = projected(station, eye);
    const x = Math.round(screen.x * frame.width);
    const y = Math.round(screen.y * frame.height);
    let ink = 0;
    let firstInk = Infinity,
      lastInk = -Infinity;
    // Physical font size 0.27 maps to 12px here. Exclude the axis line itself
    // and cyan geometry; count pale numeric ink in the short outward lane.
    for (let py = y - 14; py <= y + 18; py++) {
      for (let px = x - 54; px <= x - 5; px++) {
        if (px < 0 || py < 0 || px >= frame.width || py >= frame.height)
          continue;
        const i = (py * frame.width + px) * 4;
        if (
          frame.pixels[i]! > 100 &&
          frame.pixels[i + 1]! > 150 &&
          frame.pixels[i + 2]! > 150
        ) {
          ink++;
          firstInk = Math.min(firstInk, py);
          lastInk = Math.max(lastInk, py);
        }
      }
    }
    check(ink >= 5, `Vertical tick ${tick * 25} has no adjacent numeric ink`);
    const centerError = (firstInk + lastInk) * 0.5 - screen.y * frame.height;
    check(
      Math.abs(centerError) <= 3,
      `Vertical tick ${tick * 25} numeric center is offset by ${centerError}px`,
    );
    return { value: tick * 25, station, screen, ink, centerError };
  });
  return ticks;
}

async function pickBar(scene: Plot3dScene, eye: readonly number[]) {
  const chart = scene.charts.find((entry) => entry.name === "grid-bars")!;
  // Row6's top centre follows the fixed linear fixture axes, independently of
  // renderer-selected grid planes and label placement. The camera is orthographic.
  const result = await (chart.client as unknown as PickingWorldClient).query({
    type: "GeometryPickQuery",
    view: { kind: "bound", binding: chart.binding },
    ...projected([3.75, 4.25, 5], eye),
    includeViewPlane: false,
  });
  check(result.ok && result.hit, "Camera-only view lost the highlighted bar");
  check(
    result.hit.entity === chart.entity &&
      result.hit.component === chart.client.components.PlotGridBars3d!.id &&
      result.hit.row?.series === 0 &&
      result.hit.row.rowId === 6n,
    "View-local grid/labels changed exact source-row picking",
  );
  return result;
}

/** A nearby offscreen chart must produce the same pixels as a distant one.
 * This compares real glyphs, grids and highlighted callout/connector placement,
 * without relying on colour thresholds to overlook stray pale title text. */
async function offscreenLabels(
  scene: Plot3dScene,
  capture: Capture,
  record: RecordResult,
  selection: PlotOffscreenCase,
) {
  const chart = scene.charts.find((entry) => entry.name === "grid-bars")!;
  const client = chart.client;
  const ref = { kind: "alias" as const, alias: 91 };
  const id = aliasId(
    await client.batch([
      createEntity(91, "offscreen-label-neighbour"),
      insertComponent(client, "Transform", ref, { x: 10000 }),
      insertComponent(client, "PlotFrame3d", ref, {
        width: 0.5,
        height: 0.5,
        depth: 0.5,
        automatic_x: false,
        automatic_y: false,
        automatic_z: false,
        ticks: 1,
        source: chart.font,
        font_size: 0.3,
        x_title: "OFFSCREEN X",
        y_title: "OFFSCREEN Y",
        z_title: "OFFSCREEN Z",
      }),
    ]),
    91,
  );
  const builder = new scene.contract.ExpressionBuilder();
  const root = await declarePlot(
    client,
    scene.contract,
    "offscreen-label-neighbour",
    "PlotPoints3d",
    chart.source,
    { x: builder.encode(builder.input("column:x", "f32")) },
    [],
    [],
    {},
  );
  const move = async (position: readonly number[]) => {
    successfulBatch(
      await client.batch(
        componentFields(client, "Transform", {
          x: position[0]!,
          y: position[1]!,
          z: position[2]!,
          qy: Math.sin(0.4),
          qw: Math.cos(0.4),
        }).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id },
          component: client.components.Transform!.id,
          field,
        })),
      ),
    );
  };
  try {
    const deadline = performance.now() + 30_000;
    for (;;) {
      const binding = await scene.host.datasets.bindingView(client.session, id);
      if (binding.availability.reason === "Ready" && !binding.dirty) break;
      check(performance.now() < deadline, "Neighbour Plot did not prepare");
    }
    const extent = {
      width: selection.width,
      height: selection.width === 427 ? 600 : 960,
    };
    const binding = await scene.host.setRootOutput(chart.binding.output, {
      ...extent,
      devicePixelRatio: 1,
    });
    const eye = selection.opposite ? [-10, 8, -14] : [10, 8, 14];
    const index = selection.opposite ? 1 : 0;
    await placeCamera(scene, eye);
    const name = `offscreen-${extent.width}-${index}`;
    await move([10000, 0, 0]);
    const baseline = await capture(`${name}-reference`, binding);
    // Positive control proves the new label fixture actually renders.
    await move([5, 7, 5]);
    const visible = await capture(`${name}-visible-control`, binding);
    check(
      difference(baseline, visible) > 50,
      `${name}: neighbour labels absent`,
    );
    const { yaw } = angles(eye);
    const halfWidth = (ORTHO_HEIGHT * extent.width) / (2 * extent.height);
    for (const side of [-1, 1]) {
      // More than two metres clear of the viewport, even after the rotated
      // half-metre frame and its retained text extents. Still within the old
      // fitter's 336px reach on both sizes. The chart itself never moves in
      // the camera's depth direction.
      const right = side * (halfWidth + 3);
      await move([5 + Math.cos(yaw) * right, 2, 5 - Math.sin(yaw) * right]);
      const frame = await capture(`${name}-${side}`, binding);
      const changed = difference(baseline, frame);
      await record(`${name}-${side}`, { eye, extent, changed });
      check(
        changed === 0,
        `${name}/${side}: offscreen labels changed ${changed} visible pixels`,
      );
    }
  } finally {
    await root.unmount();
    successfulBatch(
      await client.batch([{ kind: "delete", entity: { kind: "handle", id } }]),
    );
    await scene.host.setRootOutput(chart.binding.output, {
      width: 960,
      height: 760,
      devicePixelRatio: 1,
    });
  }
}

/** Each driver owns a fresh Host/worker and capture budget for this view case. */
export async function exercisePlotViewPlacement(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: Capture,
  record: RecordResult = async () => {},
  offscreen?: PlotOffscreenCase,
) {
  const scene = await openPlot3d(host, contract, font, "plot-view-placement");
  try {
    for (const chart of scene.charts)
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "Camera", {
            ortho_height: ORTHO_HEIGHT,
          }).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: chart.camera },
            component: chart.client.components.Camera!.id,
            field,
          })),
        ),
      );
    await readyPlot3d(scene);
    if (offscreen) {
      await offscreenLabels(scene, capture, record, offscreen);
      return { views: 1, offscreen, comparisons: 2 };
    }
    const before = new Map(
      await Promise.all(
        families.map(
          async (name) => [name, await plot3dView(scene, name)] as const,
        ),
      ),
    );
    const baseline = new Map<string, Frame>();
    for (const view of [
      { name: "baseline", eye: [10, 8, 14] },
      { name: "opposite", eye: [-10, 8, -14] },
    ]) {
      await placeCamera(scene, view.eye);
      await readyPlot3d(scene);
      for (const name of families) {
        const chart = scene.charts.find((entry) => entry.name === name)!;
        const frame = await capture(`${view.name}-${name}`, chart.binding);
        const count = colors(frame);
        check(count.cyan > 100, `${view.name}/${name}: no data geometry`);
        if (name === "grid-bars") {
          await record(`far-grid-${view.name}`, farGridPixels(frame, view.eye));
          await record(
            `vertical-ticks-${view.name}`,
            verticalTickPixels(frame, view.eye),
          );
        }
        let panels: Panel[] | undefined;
        if (name === "variable-pie") {
          check(
            count.amber > 200 && count.magenta > 200,
            `${view.name}: pie slice colours absent`,
          );
          panels = calloutPanels(frame);
          await record(`panels-${view.name}`, panels);
          check(
            panels.length >= 4,
            `${view.name}: four distinct boxed pie callouts are not visible`,
          );
        }
        if (view.name === "baseline") baseline.set(name, frame);
        else
          check(
            difference(baseline.get(name)!, frame) > 500,
            `${name}: opposite-side camera did not change the completed image`,
          );
        const current = await plot3dView(scene, name);
        const original = before.get(name)!;
        check(
          current.sourceIncarnation === original.sourceIncarnation &&
            current.bindingIncarnation === original.bindingIncarnation &&
            current.evaluatedTick === original.evaluatedTick &&
            !current.dirty &&
            json(current.rows) === json(original.rows) &&
            json(current.columns) === json(original.columns),
          `${view.name}/${name}: camera-only presentation rebuilt binding results`,
        );
        await record(`view-${view.name}-${name}`, {
          eye: view.eye,
          colors: count,
          panels,
          source: current.sourceIncarnation,
          binding: current.bindingIncarnation,
          evaluatedTick: current.evaluatedTick,
        });
      }
      await record(`pick-${view.name}`, await pickBar(scene, view.eye));
    }
    return {
      views: 2,
      families: families.length,
      sourceRetained: true,
      rowPicks: 2,
    };
  } finally {
    await closePlot3d(scene);
  }
}
