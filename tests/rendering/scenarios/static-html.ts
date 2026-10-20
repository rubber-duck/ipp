import assert from "node:assert/strict";

interface HtmlFrame {
  width: number;
  height: number;
  pixels: number[];
  errors: string[];
}
export interface StaticHtmlDriver {
  call<T>(operation: string): Promise<T>;
  save(label: string, frame: HtmlFrame): Promise<void>;
}

function pixel(frame: HtmlFrame, x: number, y: number) {
  return frame.pixels.slice(
    (y * frame.width + x) * 4,
    (y * frame.width + x) * 4 + 3,
  );
}

function near(actual: number[], expected: number[]) {
  return actual.every((value, i) => Math.abs(value - expected[i]!) < 5);
}

async function capture(
  driver: StaticHtmlDriver,
  label: string,
  marker: number[],
  middle: number[],
) {
  const deadline = performance.now() + 10_000;
  for (;;) {
    const frame = await driver.call<HtmlFrame>("capture");
    if (frame.errors.length) await driver.save(`${label}-failed`, frame);
    assert.deepEqual(frame.errors, []);
    if (
      near(pixel(frame, 4, 4), marker) &&
      near(pixel(frame, 40, 24), middle)
    ) {
      await driver.save(label, frame);
      return frame;
    }
    if (performance.now() > deadline) {
      await driver.save(`${label}-failed`, frame);
      assert.fail(
        `${label}: marker=${pixel(frame, 4, 4)} middle=${pixel(frame, 40, 24)}`,
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

export async function staticHtmlScenario(driver: StaticHtmlDriver) {
  await capture(driver, "html-alpha", [0, 255, 0], [188, 0, 188]);
  await driver.call("replace");
  await capture(driver, "html-replaced", [255, 0, 0], [0, 188, 188]);
  await driver.call("resize");
  const resized = await capture(
    driver,
    "html-resized",
    [255, 0, 0],
    [0, 188, 188],
  );
  assert.ok(
    near(pixel(resized, 80, 48), [0, 0, 255]),
    "resizing discards old extent pixels",
  );
  await driver.call("recover");
  await capture(driver, "html-recovered", [255, 0, 0], [0, 188, 188]);
  await driver.call("withdraw");
  await capture(driver, "html-withdrawn", [0, 0, 255], [0, 0, 255]);
}
