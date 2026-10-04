/** Real view-local axis stations around an edge-on support tie. */
import type { Client, HostClientBase, RootBinding } from "@ipp/client";
import {
  openPlot3d,
  readyPlot3d,
  closePlot3d,
  plot3dView,
  type Plot3dContract,
} from "../plot-3d-scene.js";
import {
  componentFields,
  successfulBatch,
} from "../../../examples/world-gallery/worlds/charts/shared/commands.js";

interface Frame {
  width: number;
  height: number;
  pixels: Uint8Array;
}

const HEIGHT = 17;
const EXTENT = { width: 960, height: 760, devicePixelRatio: 1 };

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function white(frame: Frame, x: number, y: number) {
  if (x < 0 || y < 0 || x >= frame.width || y >= frame.height) return false;
  const i = (y * frame.width + x) * 4;
  return (
    frame.pixels[i]! > 100 &&
    frame.pixels[i + 1]! > 150 &&
    frame.pixels[i + 2]! > 150
  );
}

/** Each case owns its World, camera, completion barriers and capture budget. */
export async function exercisePlotAxisSupport(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: (label: string, binding: RootBinding) => Promise<Frame>,
  record: (label: string, value: unknown) => Promise<void>,
  transformed: boolean,
) {
  const scene = await openPlot3d(host, contract, font, "plot-axis-support");
  try {
    const chart = scene.charts.find((item) => item.name === "height-surface")!;
    const angle = transformed ? 1.1 : 0;
    const scale = transformed ? [1.2, 0.8, 1.1] : [1, 1, 1];
    const translation = transformed ? [7, 1, -3] : [0, 0, 0];
    const place = (point: readonly number[]) => {
      const x = point[0]! * scale[0]!,
        z = point[2]! * scale[2]!;
      return [
        translation[0]! + Math.cos(angle) * x + Math.sin(angle) * z,
        translation[1]! + point[1]! * scale[1]!,
        translation[2]! - Math.sin(angle) * x + Math.cos(angle) * z,
      ];
    };
    const fields = async (
      entity: bigint,
      component: string,
      values: Record<string, number>,
    ) => {
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, component, values).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: entity },
            component: chart.client.components[component]!.id,
            field,
          })),
        ),
      );
    };
    await fields(chart.entity, "Transform", {
      x: translation[0]!,
      y: translation[1]!,
      z: translation[2]!,
      qy: Math.sin(angle / 2),
      qw: Math.cos(angle / 2),
      sx: scale[0]!,
      sy: scale[1]!,
      sz: scale[2]!,
    });
    await fields(chart.camera, "Camera", { ortho_height: HEIGHT });
    await readyPlot3d(scene);
    const original = await plot3dView(scene, chart.name);
    const binding = await host.setRootOutput(chart.binding.output, EXTENT);
    const center = place([5, 2.5, 5]);
    const pitch = -0.4;
    const render = async (label: string, turn: number) => {
      const yaw = angle + turn;
      const eye = [
        center[0]! + 25 * Math.sin(yaw) * Math.cos(pitch),
        center[1]! - 25 * Math.sin(pitch),
        center[2]! + 25 * Math.cos(yaw) * Math.cos(pitch),
      ];
      const sy = Math.sin(yaw / 2),
        cy = Math.cos(yaw / 2),
        sx = Math.sin(pitch / 2),
        cx = Math.cos(pitch / 2);
      await fields(chart.camera, "Transform", {
        x: eye[0]!,
        y: eye[1]!,
        z: eye[2]!,
        qx: cy * sx,
        qy: sy * cx,
        qz: -sy * sx,
        qw: cy * cx,
      });
      const frame = await capture(label, binding);
      // Independently project authored frame stations; do not read the renderer's
      // selected support edge, model matrices or label-layout metadata.
      const project = (point: readonly number[]) => {
        const delta = place(point).map((value, axis) => value - eye[axis]!);
        const right = Math.cos(yaw) * delta[0]! - Math.sin(yaw) * delta[2]!;
        const up =
          Math.sin(yaw) * Math.sin(pitch) * delta[0]! +
          Math.cos(pitch) * delta[1]! +
          Math.cos(yaw) * Math.sin(pitch) * delta[2]!;
        return [
          frame.width / 2 + (right * frame.height) / HEIGHT,
          frame.height / 2 - (up * frame.height) / HEIGHT,
        ];
      };
      return { frame, project, turn, label };
    };
    const stations = (view: Awaited<ReturnType<typeof render>>, z: number) => {
      return Array.from({ length: 5 }, (_, tick) => {
        const screen = view.project([0, (tick * 5) / 4, z]);
        const x = Math.round(screen[0]!),
          y = Math.round(screen[1]!);
        let ink = 0,
          first = Infinity,
          last = -Infinity,
          axisInk = 0;
        for (let py = y - 14; py <= y + 18; py++)
          for (let px = x - 54; px <= x - 5; px++)
            if (white(view.frame, px, py)) {
              ink++;
              first = Math.min(first, py);
              last = Math.max(last, py);
            }
        for (let py = y - 3; py <= y + 3; py++)
          for (let px = x - 2; px <= x + 2; px++)
            if (white(view.frame, px, py)) axisInk++;
        return {
          value: tick,
          screen,
          ink,
          axisInk,
          centerError: (first + last) / 2 - screen[1]!,
        };
      });
    };
    const tiny = [];
    for (const [name, turn] of [
      ["positive", 2e-6],
      ["negative", -2e-6],
      ["exact", 0],
    ] as const)
      tiny.push(await render(`axis-tie-${name}`, turn));
    for (const view of tiny) {
      // Both Z supports have effectively identical screen support at the tie;
      // the near edge keeps its upright and labels clear of the surface.
      const values = stations(view, 10);
      const right = Math.ceil(view.project([10, 0, 10])[0]!);
      const floorEnds = [0, 10].map((z) => view.project([10, 0, z])[1]!);
      // Isolate the outward title lane beside the floor: exclude the selected
      // annotation above it and the X endpoint tick at the shared corner.
      let floorTitleInk = 0;
      for (
        let y = Math.floor(Math.min(...floorEnds)) - 20;
        y <= Math.ceil(Math.max(...floorEnds)) + 20;
        y++
      )
        for (let x = right + 26; x < view.frame.width; x++)
          if (white(view.frame, x, y)) floorTitleInk++;
      await record(view.label, {
        transformed,
        turn: view.turn,
        z: 10,
        values,
        floorTitleInk,
      });
      check(
        floorTitleInk >= 50,
        `${view.label}: Z annotations jumped away from the right floor edge`,
      );
      for (const tick of values) {
        check(
          tick.ink >= 5 && Math.abs(tick.centerError) <= 3,
          `${view.label}: Y tick ${tick.value} left its near support station`,
        );
        check(
          tick.axisInk >= 2,
          `${view.label}: Y tick ${tick.value} is detached from the upright axis`,
        );
      }
    }
    for (const turn of [-0.12, 0.12]) {
      const view = await render(
        `axis-normal-${turn < 0 ? "negative" : "positive"}`,
        turn,
      );
      const z = turn < 0 ? 0 : 10;
      const values = stations(view, z);
      await record(view.label, { transformed, turn, z, values });
      for (const tick of values)
        check(
          tick.ink >= 5 && Math.abs(tick.centerError) <= 3 && tick.axisInk >= 2,
          `${view.label}: Y tick ${tick.value} did not follow its outer support edge`,
        );
    }
    const current = await plot3dView(scene, chart.name);
    check(
      current.evaluatedTick === original.evaluatedTick && !current.dirty,
      "Camera support selection reevaluated source data",
    );
    return { transformed, tinyViews: 3, normalViews: 2 };
  } finally {
    await closePlot3d(scene);
  }
}
