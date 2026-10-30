import assert from "node:assert/strict";

/** Pixel expectations are independent of producer/import implementation. */
export async function nativeCefScenario(driver) {
  const near = (a, b) => a.every((v, i) => Math.abs(v - b[i]) <= 5);
  const pixel = (frame, x, y) =>
    Array.from(
      frame.pixels.slice(
        (y * frame.width + x) * 4,
        (y * frame.width + x) * 4 + 4,
      ),
    );
  const expect = async (label, marker, center) => {
    const deadline = Date.now() + 15_000;
    for (;;) {
      const frame = await driver.capture();
      if (
        near(pixel(frame, 4, 4), marker) &&
        near(pixel(frame, 32, 24), center)
      ) {
        await driver.save(label, frame);
        return frame;
      }
      if (Date.now() > deadline) {
        await driver.save(`${label}-failed`, frame);
        assert.fail(
          `${label}: marker ${pixel(frame, 4, 4)}, center ${pixel(frame, 32, 24)}, errors ${await driver.errors()}`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, 30));
    }
  };
  const initial = await expect(
    "initial-alpha",
    [0, 255, 0, 255],
    [188, 0, 188, 255],
  );
  assert.ok(
    near(pixel(initial, 100, 110), [255, 128, 0, 255]),
    "nested canvas is included in the accelerated frame",
  );
  const allocations = await driver.allocations();
  assert.equal(allocations.availability.allocations, "available");
  assert.ok(
    BigInt(allocations.window.lastBoundary) -
      BigInt(allocations.window.firstBoundary) >=
      10n,
    "measure several completed native frames",
  );
  const recurring = allocations.categories.filter(
    (category) => category.name !== "unattributed",
  );
  assert.ok(
    allocations.stages.some((stage) => BigInt(stage.calls) >= 10n),
    "Host frame processing was measured",
  );
  assert.deepEqual(
    recurring.filter((category) => BigInt(category.allocationCalls) !== 0n),
    [],
    "unchanged native evaluation and rendering reuse their storage",
  );
  assert.deepEqual(
    allocations.stages.filter((stage) => BigInt(stage.allocationCalls) !== 0n),
    [],
    "unchanged World System passes reuse their storage",
  );
  await driver.click(64, 44);
  await expect("clicked", [255, 0, 0, 255], [0, 188, 188, 255]);
  await driver.click(110, 44);
  await driver.text("CEF");
  await expect("typed", [255, 255, 0, 255], [0, 188, 188, 255]);
  await driver.key("Backspace");
  await expect("backspace", [255, 255, 255, 255], [0, 188, 188, 255]);
  await driver.text("F");
  await expect("retyped", [255, 255, 0, 255], [0, 188, 188, 255]);
  await driver.click(16, 88);
  const navigationDeadline = Date.now() + 5000;
  while (
    !(await driver.navigations()).length &&
    Date.now() < navigationDeadline
  )
    await new Promise((resolve) => setTimeout(resolve, 20));
  assert.match(
    (await driver.navigations())[0] ?? "",
    /replacement\.html$/,
    "links return navigation to the client",
  );
  await expect(
    "navigation-awaits-client",
    [255, 255, 0, 255],
    [0, 188, 188, 255],
  );
  await driver.replace("form.html");
  await expect("form-ready", [255, 0, 0, 255], [0, 0, 255, 255]);
  await driver.click(64, 44);
  await expect("form-submitted", [22, 128, 75, 255], [0, 0, 255, 255]);
  await driver.replace("replacement.html");
  await expect("replaced", [255, 0, 0, 255], [0, 188, 188, 255]);
  await driver.resize(63, 31);
  const resized = await expect("resized", [255, 0, 0, 255], [0, 188, 188, 255]);
  assert.ok(
    near(pixel(resized, 80, 48), [0, 0, 255, 255]),
    "old extent is no longer visible",
  );
  await driver.recover();
  await expect("recovered", [255, 0, 0, 255], [0, 188, 188, 255]);
  await driver.withdraw();
  await expect("withdrawn", [0, 0, 255, 255], [0, 0, 255, 255]);
  await driver.reconnect();
  await expect("reconnected", [0, 255, 0, 255], [188, 0, 188, 255]);
  assert.deepEqual(await driver.errors(), []);
}
