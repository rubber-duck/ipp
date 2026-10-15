/** Independent image observations for native Plot finishes and data geometry. */
import { check } from "../../harness/page/checks.js";
import type { PlotFrame } from "./plot-capture.js";

export type PlotRegion = readonly [
  left: number,
  top: number,
  right: number,
  bottom: number,
];

export function plotPixelDifference(
  before: PlotFrame,
  after: PlotFrame,
  region: PlotRegion = [0, 0, before.width, before.height],
) {
  check(
    before.width === after.width && before.height === after.height,
    "Finish changed capture extent",
  );
  const [left, top, right, bottom] = region;
  let changed = 0;
  let sum = 0;
  for (let y = top; y < bottom; y++) {
    for (let x = left; x < right; x++) {
      const offset = (y * before.width + x) * 4;
      let delta = 0;
      for (let channel = 0; channel < 3; channel++)
        delta += Math.abs(
          before.pixels[offset + channel]! - after.pixels[offset + channel]!,
        );
      sum += delta;
      if (delta > 24) changed++;
    }
  }
  return { changed, mean: sum / ((right - left) * (bottom - top) * 3) };
}

export function assertPlotFinishResponse(
  before: PlotFrame,
  glass: PlotFrame,
  movedLight: PlotFrame,
  region: PlotRegion,
  minimumPixels: number,
) {
  const finish = plotPixelDifference(before, glass, region);
  const light = plotPixelDifference(glass, movedLight, region);
  check(
    finish.changed > minimumPixels,
    `Plot material did not change enough data pixels: ${finish.changed}`,
  );
  check(
    light.changed > minimumPixels / 10,
    `Plot material ignored light movement: ${light.changed}`,
  );
  return { finish, light };
}

/** Header/tick regions lie outside mark geometry; procedural paint must not wash over them. */
export function assertPlotRegionUnchanged(
  before: PlotFrame,
  glass: PlotFrame,
  region: PlotRegion,
) {
  const difference = plotPixelDifference(before, glass, region);
  check(
    difference.changed === 0,
    `Plot paint escaped its data marks: ${difference.changed}`,
  );
  return difference;
}

/** Solid surfaces have no authored interior mesh edges; wireframe opts them in. */
export function assertPlotSurfaceEdges(solid: PlotFrame, wire: PlotFrame) {
  const region: PlotRegion = [300, 360, 600, 500];
  const difference = plotPixelDifference(solid, wire, region);
  let paleInterior = 0;
  for (let y = region[1]; y < region[3]; y++)
    for (let x = region[0]; x < region[2]; x++) {
      const offset = (y * solid.width + x) * 4;
      if (
        solid.pixels[offset]! > 180 &&
        solid.pixels[offset + 1]! > 210 &&
        solid.pixels[offset + 2]! > 210
      )
        paleInterior++;
    }
  check(
    difference.changed > 100,
    "Surface wireframe did not expose native interior edges",
  );
  check(
    paleInterior < 200,
    `Solid surface contains pale triangulation seams: ${paleInterior}`,
  );
  return { difference, paleInterior };
}

/** Inspect an interior patch of the tallest authored bar, away from its rim/callout. */
export function assertPlotSurfaceDetail(frame: PlotFrame) {
  let maximumStep = 0;
  let crispSteps = 0;
  for (let y = 675; y < 697; y++)
    for (let x = 315; x < 339; x++) {
      const offset = (y * frame.width + x) * 4;
      let step = 0;
      for (let channel = 0; channel < 3; channel++)
        step += Math.abs(
          frame.pixels[offset + channel]! - frame.pixels[offset + 4 + channel]!,
        );
      step /= 3;
      maximumStep = Math.max(maximumStep, step);
      if (step > 5) crispSteps++;
    }
  check(
    maximumStep > 0.3 && maximumStep < 50,
    `Glass surface lost reflected detail or gained a hard facet: ${JSON.stringify({ maximumStep, crispSteps })}`,
  );
  return { maximumStep, crispSteps };
}

/** The selected bar's broad face must change, independently of the callout/rim. */
export function assertPlotSelection(idle: PlotFrame, selected: PlotFrame) {
  const face = plotPixelDifference(idle, selected, [399, 313, 418, 349]);
  check(
    face.changed > 500 && face.mean > 18,
    `Selected glass face remains too faint: ${JSON.stringify(face)}`,
  );
  const unrelated = assertPlotRegionUnchanged(
    idle,
    selected,
    [236, 408, 256, 450],
  );
  return { face, unrelated };
}
