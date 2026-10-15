import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { check } from "../../harness/page/checks.js";
import { GLASS_VALUE_RECTS, GLASS_VALUES_EXTENT } from "./fixtures/values.js";

/** Paint margins must not shift scaled switch blocks away from their native centres. */
export function checkGlassSwitchPlacement(image: RgbaImage) {
  const offMark = region(image, 45, 45, 15, 16);
  const onMark = region(image, 133, 45, 15, 16);
  check(
    offMark.max < 170 && onMark.max > 200,
    `Checkbox glyph must follow checked state: ${offMark.max}/${onMark.max}`,
  );
  for (const x of [330, 490]) {
    const rim = region(image, x, 39.5, 12, 1.5);
    check(
      rim.max > 110,
      `Scaled switch moved off its native top edge: ${rim.max}`,
    );
  }
  const offBody = region(image, 332, 48, 8, 8);
  const offTrack = region(image, 370, 48, 8, 8);
  const onTrack = region(image, 450, 48, 8, 8);
  const offCorner = region(image, 348.5, 39.5, 0.5, 0.5);
  check(offBody.mean > 60, "Off switch lost its visible glass lens");
  check(
    onTrack.mean > offTrack.mean + 8,
    "On switch lost its selected track tint",
  );
  check(offCorner.max < 100, "Switch lens extends beyond its round contour");
  const empty = region(image, 140, 312, 20, 4);
  const complete = region(image, 140, 376, 20, 4);
  check(
    complete.mean > empty.mean + 15,
    "Progress fill does not show completion",
  );
  const panelFace = region(image, 410, 330, 40, 8);
  check(panelFace.max < 80, "Matte panel backing became a bright glass face");
}

/** Clear and frosted bodies transmit the page, including their disabled states. */
export function checkGlassTransmission(dark: RgbaImage, bright: RgbaImage) {
  check(
    dark.width === bright.width && dark.height === bright.height,
    "Transmission frame sizes differ",
  );
  const regions = [
    [328, 47, 12, 10],
    [488, 47, 12, 10],
    [608, 47, 12, 10],
    [144, 139, 14, 18],
    [144, 227, 14, 18],
    [100, 146, 12, 4],
    [100, 234, 12, 4],
    [693, 238, 14, 18],
    [789, 175, 14, 18],
    [40, 44, 8, 12],
    [128, 44, 8, 8],
    [216, 44, 8, 8],
    [100, 344, 12, 4],
  ];
  const scale = dark.width / GLASS_VALUES_EXTENT[0];
  let maximum = 0;
  for (const [x, y, width, height] of regions)
    for (
      let py = Math.floor(y! * scale);
      py < Math.floor((y! + height!) * scale);
      py++
    )
      for (
        let px = Math.floor(x! * scale);
        px < Math.floor((x! + width!) * scale);
        px++
      )
        for (let channel = 0; channel < 3; channel++) {
          const index = (py * dark.width + px) * 4 + channel;
          maximum = Math.max(
            maximum,
            Math.abs(dark.pixels[index]! - bright.pixels[index]!),
          );
        }
  check(
    maximum > 20 && maximum < 230,
    `Glass transmission missing or unbounded: channel difference ${maximum}`,
  );
  return maximum;
}

/** Moving the same source must change each material family without moving its bounds. */
export function checkGlassValueLightResponse(
  left: RgbaImage,
  right: RgbaImage,
) {
  const regions = {
    checkbox: GLASS_VALUE_RECTS["checkbox-off"]!,
    switch: GLASS_VALUE_RECTS["switch-off"]!,
    slider: GLASS_VALUE_RECTS.horizontal!,
  };
  const scale = left.width / GLASS_VALUES_EXTENT[0];
  const differences: Record<string, number> = {};
  for (const [name, [x, y, width, height]] of Object.entries(regions)) {
    let total = 0,
      count = 0;
    for (
      let py = Math.floor(y! * scale);
      py < Math.floor((y! + height!) * scale);
      py++
    )
      for (
        let px = Math.floor(x! * scale);
        px < Math.floor((x! + width!) * scale);
        px++
      )
        for (let channel = 0; channel < 3; channel++) {
          const index = (py * left.width + px) * 4 + channel;
          total += Math.abs(left.pixels[index]! - right.pixels[index]!);
          count++;
        }
    differences[name] = total / count;
    check(
      differences[name]! > 0.02,
      `${name} did not respond to moving light: ${differences[name]}`,
    );
  }
  return differences;
}

/** Image geometry is calculated independently of themes and shader inputs. */
export function checkGlassSliderThumb(
  image: RgbaImage,
  id: string,
  fraction: number,
  vertical = false,
) {
  const rect = GLASS_VALUE_RECTS[id];
  check(rect, `No slider oracle rectangle for ${id}`);
  const [x, y, width, height] = rect;
  const edge = Math.min(width, height) * 0.75;
  const inset = edge / 2;
  const cx = vertical ? x + width / 2 : x + inset + fraction * (width - edge);
  const cy = vertical
    ? y + height - inset - fraction * (height - edge)
    : y + height / 2;
  const centre = region(
    image,
    cx - edge * 0.1,
    cy - edge * 0.1,
    edge * 0.2,
    edge * 0.2,
  );
  const rim = region(image, cx - edge * 0.35, cy - edge / 2 - 1, edge * 0.7, 3);
  check(
    rim.max > centre.mean + 18,
    `${id} thumb at ${fraction} lost its glass rim: rim ${rim.max}, centre ${centre.mean}`,
  );
  check(
    centre.mean < 175,
    `${id} idle thumb centre is washed out: ${centre.mean}`,
  );
}

function region(
  image: RgbaImage,
  x: number,
  y: number,
  width: number,
  height: number,
) {
  const sx = image.width / GLASS_VALUES_EXTENT[0];
  const sy = image.height / GLASS_VALUES_EXTENT[1];
  let sum = 0;
  let max = 0;
  let count = 0;
  for (let py = Math.floor(y * sy); py < Math.ceil((y + height) * sy); py++) {
    for (let px = Math.floor(x * sx); px < Math.ceil((x + width) * sx); px++) {
      check(
        px >= 0 && py >= 0 && px < image.width && py < image.height,
        "Glass oracle region outside capture",
      );
      const index = (py * image.width + px) * 4;
      const value = Math.max(
        image.pixels[index]!,
        image.pixels[index + 1]!,
        image.pixels[index + 2]!,
      );
      sum += value;
      max = Math.max(max, value);
      count++;
    }
  }
  check(count > 0, "Empty glass oracle region");
  return { mean: sum / count, max };
}
