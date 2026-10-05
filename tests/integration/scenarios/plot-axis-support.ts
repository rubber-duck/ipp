/** Real view-local axis stations around an edge-on support tie. */
import type { Client, HostClientBase } from "@ipp/client";
import type { PlotCapture } from "../plot-capture.js";
import {
  openPlot3d,
  readyPlot3d,
  closePlot3d,
  plot3dView,
  type Plot3dContract,
} from "../plot-3d-scene.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../../examples/world-gallery/worlds/charts/shared/commands.js";
import { declarePlot } from "../../../examples/world-gallery/worlds/charts/shared/declare-plot.js";

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

/** These fixed 0..4 values are single glyphs beside the upright axis. */
function numericTickInk(
  frame: Frame,
  screen: readonly number[],
  fontSpan: number,
) {
  const x = Math.round(screen[0]!),
    y = Math.round(screen[1]!);
  const columns = new Map<number, number[]>();
  for (let py = y - 14; py <= y + 18; py++)
    // This projected font lane excludes the neighbouring Z endpoint at the
    // shared upper corner, without excluding or shifting the Y endpoint.
    for (
      let px = x - Math.ceil(fontSpan * 4.5);
      px <= x - Math.ceil(fontSpan * 2);
      px++
    )
      if (white(frame, px, py)) {
        const column = columns.get(px) ?? [];
        column.push(py);
        columns.set(px, column);
      }
  // Titles may enter this broad region after local layout. The numeric glyph
  // occupies the nearest column band to its upright. One empty sampling column
  // can split a glyph's strokes; join that hole while keeping separate title
  // letters outside the band. Retain every row so a real shift along the axis
  // still fails the same limit.
  const right = Math.max(...columns.keys());
  let left = right;
  while (columns.has(left - 1) || columns.has(left - 2))
    left -= columns.has(left - 1) ? 1 : 2;
  const bounds = [Infinity, Infinity, -Infinity, -Infinity];
  const excludedBounds = [Infinity, Infinity, -Infinity, -Infinity];
  let ink = 0,
    excludedInk = 0,
    axisInk = 0;
  for (const [px, rows] of columns)
    for (const py of rows) {
      const selected = px >= left;
      const target = selected ? bounds : excludedBounds;
      target[0] = Math.min(target[0]!, px);
      target[1] = Math.min(target[1]!, py);
      target[2] = Math.max(target[2]!, px);
      target[3] = Math.max(target[3]!, py);
      if (selected) ink++;
      else excludedInk++;
    }
  for (let py = y - 3; py <= y + 3; py++)
    for (let px = x - 2; px <= x + 2; px++) if (white(frame, px, py)) axisInk++;
  return {
    ink,
    bounds,
    excludedInk,
    excludedBounds,
    axisInk,
    centerError: (bounds[1]! + bounds[3]!) / 2 - screen[1]!,
  };
}

/** Each case owns its World, camera, completion barriers and capture budget. */
export async function exercisePlotAxisSupport(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: PlotCapture,
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
      values: Record<string, number | string>,
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
    const render = async (label: string, turn: number, height = HEIGHT) => {
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
      const frame = await capture.afterMotion(label, binding, chart.client);
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
          frame.width / 2 + (right * frame.height) / height,
          frame.height / 2 - (up * frame.height) / height,
        ];
      };
      return { frame, project, turn, label, eye, height };
    };
    const stations = (view: Awaited<ReturnType<typeof render>>, z: number) => {
      return Array.from({ length: 5 }, (_, tick) => {
        const screen = view.project([0, (tick * 5) / 4, z]);
        const fontSpan = Math.abs(view.project([0.27, 0, z])[0]! - screen[0]!);
        return {
          value: tick,
          screen,
          ...numericTickInk(view.frame, screen, fontSpan),
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
      const left = Math.floor(view.project([0, 5, 10])[0]!);
      const upperEnds = [0, 10].map((z) => view.project([0, 5, z])[1]!);
      // Z now owns the upper-left enclosure edge. Its outward label lane stays
      // beside that edge while Y retains the front-left upright.
      let upperTitleInk = 0;
      for (
        let y = Math.floor(Math.min(...upperEnds)) - 20;
        y <= Math.ceil(Math.max(...upperEnds)) + 20;
        y++
      )
        for (let x = 0; x < left - 26; x++)
          if (white(view.frame, x, y)) upperTitleInk++;
      await record(view.label, {
        transformed,
        turn: view.turn,
        z: 10,
        values,
        upperTitleInk,
      });
      check(
        upperTitleInk >= 50,
        `${view.label}: Z annotations left the upper-left enclosure edge`,
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
      const z = 10;
      const values = stations(view, z);
      await record(view.label, { transformed, turn, z, values });
      for (const tick of values)
        check(
          tick.ink >= 5 && Math.abs(tick.centerError) <= 3 && tick.axisInk >= 2,
          `${view.label}: Y tick ${tick.value} did not follow its outer support edge`,
        );
    }
    // Use distinct authored colours to attribute actual pixels to their owner.
    // Short titles keep glyph length separate from distance to the owning axis.
    const labels = chart.client.components[chart.component]!.fields.labels!;
    check(labels.rows, "Plot labels Rows layout is unavailable");
    successfulBatch(
      await chart.client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: chart.entity },
          component: chart.client.components[chart.component]!.id,
          field: {
            offset: labels.offset,
            value: {
              kind: "rows",
              value: contract.encodeRowsTable(labels.rows, {
                nextSlot: 0,
                rows: new Map(),
              }),
            },
          },
        },
      ]),
    );
    await fields(chart.entity, "PlotFrame3d", {
      red: 1,
      green: 1,
      blue: 0,
      x_title: "X",
      y_title: "Y",
      z_title: "Z",
    });
    const neighbourName = "visible-axis-neighbour";
    const neighbourRef = { kind: "alias" as const, alias: 91 };
    const neighbour = aliasId(
      await chart.client.batch([
        createEntity(91, neighbourName),
        insertComponent(chart.client, "Transform", neighbourRef, { x: 10000 }),
        insertComponent(chart.client, "PlotFrame3d", neighbourRef, {
          width: 0.5,
          height: 5,
          depth: 0.5,
          automatic_x: false,
          automatic_y: false,
          automatic_z: false,
          min_y: 0,
          max_y: 4,
          ticks: 16,
          source: chart.font,
          font_size: 0.27,
          red: 1,
          green: 0,
          blue: 1,
          x_title: "N",
          y_title: "N",
          z_title: "N",
        }),
      ]),
      91,
    );
    const builder = new contract.ExpressionBuilder();
    const neighbourRoot = await declarePlot(
      chart.client,
      contract,
      neighbourName,
      "PlotPoints3d",
      chart.source,
      { x: builder.encode(builder.input("column:x", "f32")) },
      [],
      [],
      {},
    );
    try {
      const deadline = performance.now() + 30_000;
      for (;;) {
        const view = await host.datasets.bindingView(
          chart.client.session,
          neighbour,
        );
        if (view.availability.reason === "Ready" && !view.dirty) break;
        check(
          performance.now() < deadline,
          "Visible neighbour Plot did not prepare",
        );
        await chart.client.waitForFrame();
      }
      const moveNeighbour = async (visible: boolean) => {
        const position = visible ? place([-3, 0, 10]) : [10000, 0, 0];
        await fields(neighbour, "Transform", {
          x: position[0]!,
          y: position[1]!,
          z: position[2]!,
          qy: Math.sin(angle / 2),
          qw: Math.cos(angle / 2),
          sx: scale[0]!,
          sy: scale[1]!,
          sz: scale[2]!,
        });
      };
      const axisInk = (frame: Frame, x: number, y: number) => {
        if (x < 0 || y < 0 || x >= frame.width || y >= frame.height)
          return false;
        const i = (y * frame.width + x) * 4;
        return (
          frame.pixels[i]! > 90 &&
          frame.pixels[i + 1]! > 90 &&
          frame.pixels[i + 2]! < 90
        );
      };
      const neighbourInk = (frame: Frame) => {
        let count = 0;
        for (let i = 0; i < frame.pixels.length; i += 4)
          if (
            frame.pixels[i]! > 90 &&
            frame.pixels[i + 1]! < 90 &&
            frame.pixels[i + 2]! > 90
          )
            count++;
        return count;
      };
      const segmentDistance = (
        point: readonly number[],
        a: readonly number[],
        b: readonly number[],
      ) => {
        const dx = b[0]! - a[0]!,
          dy = b[1]! - a[1]!;
        const squared = dx * dx + dy * dy;
        const t =
          squared === 0
            ? 0
            : Math.max(
                0,
                Math.min(
                  1,
                  ((point[0]! - a[0]!) * dx + (point[1]! - a[1]!) * dy) /
                    squared,
                ),
              );
        return Math.hypot(
          point[0]! - a[0]! - t * dx,
          point[1]! - a[1]! - t * dy,
        );
      };
      const isolatedNeighbourInk = new Map<number, number>();
      for (const [name, height, visible] of [
        ["near-isolated", 17, false],
        ["far-isolated", 68, false],
        ["distant-isolated", 136, false],
        ["overview-isolated", 170, false],
        ["far-neighbour", 68, true],
        ["near-neighbour", 17, true],
        ["overview-return", 170, true],
        ["far-return", 68, true],
      ] as const) {
        await moveNeighbour(visible);
        await fields(chart.camera, "Camera", { ortho_height: height });
        const view = await render(`axis-association-${name}`, 0, height);
        const fontHeight = (0.27 * scale[1]! * view.frame.height) / height;
        const fontExtent =
          (0.27 * Math.max(scale[0]!, scale[1]!) * view.frame.height) / height;
        const values = Array.from({ length: 5 }, (_, tick) => {
          const screen = view.project([0, (tick * 5) / 4, 10]);
          const band = Math.max(2, fontHeight * 0.65);
          const axisExclusion = Math.max(1.5, fontExtent * 0.25);
          const bounds = [Infinity, Infinity, -Infinity, -Infinity];
          let ink = 0;
          for (
            let y = Math.floor(screen[1]! - band);
            y <= Math.ceil(screen[1]! + band);
            y++
          )
            for (
              let x = Math.floor(screen[0]! - fontExtent * 10);
              x <= Math.floor(screen[0]! - axisExclusion);
              x++
            )
              if (axisInk(view.frame, x, y)) {
                ink++;
                bounds[0] = Math.min(bounds[0]!, x);
                bounds[1] = Math.min(bounds[1]!, y);
                bounds[2] = Math.max(bounds[2]!, x);
                bounds[3] = Math.max(bounds[3]!, y);
              }
          return {
            value: tick,
            screen,
            ink,
            bounds,
            normalGap: screen[0]! - bounds[2]!,
            tangentError: (bounds[1]! + bounds[3]!) / 2 - screen[1]!,
          };
        });
        const corners = Array.from({ length: 8 }, (_, index) =>
          view.project([
            index & 1 ? 10 : 0,
            index & 2 ? 5 : 0,
            index & 4 ? 10 : 0,
          ]),
        );
        const edges = corners.flatMap((point, index) =>
          [1, 2, 4]
            .filter((axis) => !(index & axis))
            .map((axis) => [point, corners[index | axis]!]),
        );
        let ownerInk = 0,
          maximumAxisDistance = 0;
        for (let y = 0; y < view.frame.height; y++)
          for (let x = 0; x < view.frame.width; x++)
            if (axisInk(view.frame, x, y)) {
              ownerInk++;
              maximumAxisDistance = Math.max(
                maximumAxisDistance,
                Math.min(
                  ...edges.map(([a, b]) => segmentDistance([x, y], a!, b!)),
                ),
              );
            }
        const visibleNeighbourInk = neighbourInk(view.frame);
        if (!visible) isolatedNeighbourInk.set(height, visibleNeighbourInk);
        await record(view.label, {
          transformed,
          transform: { angle, scale, translation },
          eye: view.eye,
          extent: EXTENT,
          orthoHeight: height,
          fontHeight,
          fontExtent,
          neighbourVisible: visible,
          neighbourInk: visibleNeighbourInk,
          ownerInk,
          maximumAxisDistance,
          values,
        });
        // Association is measured in authored font units, with two pixels for
        // sampling. No renderer-selected bounds, offsets or search reach enter
        // the expectation. The same viewport retains near-view layout history.
        check(
          ownerInk >= (height <= 68 ? 10 : 3),
          `${view.label}: owner axis labels disappeared`,
        );
        check(
          maximumAxisDistance <= fontExtent * 8 + 2,
          `${view.label}: axis title ink floated away from its own chart`,
        );
        // At the overview scale some subpixel glyphs fall between pixel centres.
        // Keep the owner-ink envelope assertion there; individual numeric values
        // must remain visible and aligned in the near and intermediate views.
        if (height <= 68)
          for (const tick of values) {
            check(
              tick.ink >= 1,
              `${view.label}: Y tick ${tick.value} disappeared`,
            );
            if (tick.value > 0 && tick.value < 4)
              check(
                Math.abs(tick.tangentError) <= 3 &&
                  tick.normalGap <= fontHeight * 3 + 2,
                `${view.label}: Y tick ${tick.value} floated away from its axis station`,
              );
          }
        if (visible && height >= 68)
          check(
            visibleNeighbourInk >
              isolatedNeighbourInk.get(height)! + (height <= 68 ? 10 : 3),
            `${view.label}: visible neighbour labels did not render`,
          );
      }
    } finally {
      await neighbourRoot.unmount();
      successfulBatch(
        await chart.client.batch([
          { kind: "delete", entity: { kind: "handle", id: neighbour } },
        ]),
      );
    }
    const current = await plot3dView(scene, chart.name);
    check(
      current.sourceIncarnation === original.sourceIncarnation &&
        current.bindingIncarnation === original.bindingIncarnation &&
        current.evaluatedTick === original.evaluatedTick &&
        !current.dirty,
      "Camera support selection reevaluated source data",
    );
    return { transformed, tinyViews: 3, normalViews: 2, associationViews: 8 };
  } finally {
    await closePlot3d(scene);
  }
}
