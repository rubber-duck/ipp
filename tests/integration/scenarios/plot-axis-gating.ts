/** Optional adaptive axes share one selected-view lifetime across interaction changes. */
import type { Client, HostClientBase } from "@ipp/client";
import {
  closePlot3d,
  openPlot3d,
  readyPlot3d,
  type Plot3dContract,
} from "../plot-3d-scene.js";
import type { PlotCapture } from "../plot-capture.js";
import {
  axisInk,
  camera,
  check,
  motionNumericInk,
  perimeterInk,
  prepareAxisPoints,
} from "./plot-axis-fixture.js";

const EXTENT = { width: 480, height: 480, devicePixelRatio: 1 };

// This fixture travels from (0,z) through (0,0) to (1,0). A signed
// station makes progress across that adjacent corner directly comparable.
function resumeStation(cross: readonly number[]) {
  if (cross[1] === 0) return cross[0]!;
  if (cross[0] === 0) return -cross[1]!;
  return Number.NaN;
}

function easedProgress(seconds: number) {
  const t = Math.max(0, Math.min(1, seconds / 2));
  return t * t * (3 - 2 * t);
}

export async function exercisePlotAxisGating(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: PlotCapture,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const scene = await openPlot3d(host, contract, font, "plot-axis-gating");
  try {
    const { chart, fields, editHeight } = await prepareAxisPoints(scene);
    await readyPlot3d(scene);
    const binding = await host.setRootOutput(chart.binding.output, EXTENT);
    const gating = {
      initial: camera([5, 0, 10], 0, -0.4, 10),
      target: camera([5, 5, 0], 2.7, -0.4, 10),
    };
    return await capture.session(binding, async (viewCapture) => {
      const inspection = await chart.client.inspect();
      check(
        inspection.entities
          .find((entity) => entity.id === chart.entity)!
          .components.some(
            (component) => component.fields.adaptive_axes === false,
          ),
        "Adaptive axes must be disabled in a newly authored chart",
      );
      await fields(chart.camera, "Camera", {
        ortho_height: gating.initial.height,
      });
      await fields(chart.camera, "Transform", gating.initial.fields);
      const inactiveReference = await viewCapture.afterMotion(
        "axis-inactive-default",
        chart.client,
      );
      check(
        axisInk(inactiveReference, gating.initial, 0, [0, 1]).fraction >= 0.75,
        "Initially inactive axis did not use its standard perimeter edge",
      );
      const disabledCamera = await viewCapture.motion(
        "axis-inactive-camera",
        chart.client,
        () => fields(chart.camera, "Transform", gating.target.fields),
        0.5,
      );
      const disabledSource = await viewCapture.motion(
        "axis-inactive-source",
        chart.client,
        () => editHeight(50),
        0.5,
      );
      for (const samples of [disabledCamera, disabledSource])
        for (const sample of [samples.start, samples.middle, samples.end])
          check(
            axisInk(sample.frame, gating.target, 0, [0, 1]).fraction >= 0.7,
            "Disabled axis moved after a camera or source update",
          );
      await record("axis-inactive-updates", {
        defaultAdaptiveAxes: false,
        camera: axisInk(disabledCamera.end.frame, gating.target, 0, [0, 1]),
        source: axisInk(disabledSource.end.frame, gating.target, 0, [0, 1]),
      });
      await editHeight(100);
      await fields(chart.entity, "PlotFrame3d", { adaptive_axes: true });
      await readyPlot3d(scene);
      await fields(chart.camera, "Camera", {
        ortho_height: gating.initial.height,
      });
      await fields(chart.camera, "Transform", gating.initial.fields);
      await viewCapture.afterMotion("axis-freeze-origin", chart.client);
      const interrupted = await viewCapture.motion(
        "axis-freeze-during-motion",
        chart.client,
        () => fields(chart.camera, "Transform", gating.target.fields),
        0.5,
        () => fields(chart.entity, "PlotFrame3d", { adaptive_axes: false }),
      );
      check(
        interrupted.interrupted,
        "Axis freeze requires a completed inactive frame",
      );
      const frozen = perimeterInk(
        interrupted.interrupted.frame,
        gating.target,
        0,
      );
      check(
        frozen.fraction >= 0.7 &&
          axisInk(interrupted.end.frame, gating.target, 0, frozen.cross)
            .fraction >= 0.7,
        "Disabled axis failed to hold its intermediate perimeter station",
      );
      const resumed = await viewCapture.motion(
        "axis-resume-from-frozen",
        chart.client,
        () => fields(chart.entity, "PlotFrame3d", { adaptive_axes: true }),
        0.5,
      );
      const resumedStart = perimeterInk(resumed.start.frame, gating.target, 0),
        resumedMiddle = perimeterInk(resumed.middle.frame, gating.target, 0),
        resumedEnd = axisInk(resumed.end.frame, gating.target, 0, [1, 0]);
      const frozenStation = resumeStation(frozen.cross),
        startStation = resumeStation(resumedStart.cross),
        middleStation = resumeStation(resumedMiddle.cross);
      // A completed capture arrives after mutation and may already show motion.
      // The transition starts after origin.before and by the first resumed draw;
      // bound its eased progress with the surrounding Host-clock observations.
      const stationTolerance = 0.025;
      const bounds = (before: number, after: number) => ({
        min:
          frozenStation +
          (1 - frozenStation) *
            easedProgress(before - resumed.start.after.time) -
          stationTolerance,
        max:
          frozenStation +
          (1 - frozenStation) *
            easedProgress(after - resumed.origin.before.time) +
          stationTolerance,
      });
      const startBounds = bounds(
          resumed.start.before.time,
          resumed.start.after.time,
        ),
        middleBounds = bounds(
          resumed.middle.before.time,
          resumed.middle.after.time,
        );
      await record("axis-freeze-resume", {
        frozen,
        resumedStart,
        resumedMiddle,
        resumedEnd,
        stations: {
          frozen: frozenStation,
          start: startStation,
          middle: middleStation,
        },
        bounds: {
          start: startBounds,
          middle: middleBounds,
          tolerance: stationTolerance,
        },
        clocks: {
          origin: resumed.origin,
          start: { before: resumed.start.before, after: resumed.start.after },
          middle: {
            before: resumed.middle.before,
            after: resumed.middle.after,
          },
          end: { before: resumed.end.before, after: resumed.end.after },
        },
      });
      check(
        resumedStart.fraction >= 0.7 &&
          startStation >= startBounds.min &&
          startStation <= startBounds.max &&
          resumedMiddle.fraction >= 0.7 &&
          middleStation >= middleBounds.min &&
          middleStation <= middleBounds.max &&
          middleStation - frozenStation > 0.03 &&
          resumedEnd.fraction >= 0.75,
        "Reenabled axis did not resume smoothly from its frozen perimeter station",
      );
      const frozenGlyphs = motionNumericInk(
        interrupted.end.frame,
        gating.target,
        0,
        frozen.cross,
      );
      await record("axis-frozen-numeric-labels", frozenGlyphs);
      return {
        defaultAdaptiveAxes: false,
        inactiveUpdates: 2,
        interruptedResume: true,
      };
    });
  } finally {
    await closePlot3d(scene);
  }
}
