import { check } from "../../../harness/page/checks.js";

const assert = {
  equal(actual: unknown, expected: unknown) {
    check(actual === expected, `Expected ${expected}, received ${actual}`);
  },
  deepEqual(actual: readonly number[], expected: readonly number[]) {
    check(
      actual.length === expected.length &&
        actual.every((value, index) => value === expected[index]),
      `Expected [${expected}], received [${actual}]`,
    );
  },
};

/** Operations implemented by native and browser glass drivers. */
export interface GlassValuesDriver {
  click(id: string): Promise<void>;
  drag(id: string, from: number, to: number, thumb?: number): Promise<void>;
  focus(id: string, thumb?: number): Promise<void>;
  key(key: string): Promise<void>;
  checked(id: string): Promise<boolean>;
  values(id: string): Promise<readonly [number, number]>;
  focusedPart(id: string): Promise<number | undefined>;
  capture(name: string): Promise<void>;
}

/** Real input asserts committed native values before collecting visual states. */
export async function glassValuesScenario(driver: GlassValuesDriver) {
  await driver.capture("values-idle");

  for (const kind of ["checkbox", "switch"] as const) {
    await driver.click(`${kind}-off`);
    assert.equal(await driver.checked(`${kind}-off`), true);
    await driver.focus(`${kind}-off`);
    await driver.capture(`${kind}-checked-focus`);
    await driver.key("Space");
    assert.equal(await driver.checked(`${kind}-off`), false);
    await driver.click(`${kind}-disabled`);
    assert.equal(await driver.checked(`${kind}-disabled`), true);
  }
  await driver.capture("selection-values");

  for (const id of ["horizontal", "vertical"] as const) {
    await driver.drag(id, 0.5, 0.8);
    assert.equal((await driver.values(id))[0], 8);
    await driver.focus(id);
    await driver.key("Home");
    assert.equal((await driver.values(id))[0], 0);
    await driver.key("End");
    assert.equal((await driver.values(id))[0], 10);
    await driver.key("Shift+ArrowLeft");
    assert.equal((await driver.values(id))[0], 9.75);
  }

  for (const id of ["range", "vertical-range"] as const) {
    await driver.focus(id, 0);
    assert.equal(await driver.focusedPart(id), 0);
    await driver.capture(`${id}-lower-focus`);
    await driver.key("End");
    assert.deepEqual(await driver.values(id), [8, 8]);
    await driver.focus(id, 1);
    assert.equal(await driver.focusedPart(id), 1);
    await driver.capture(`${id}-upper-focus`);
    await driver.key("Home");
    assert.deepEqual(await driver.values(id), [8, 8]);
    await driver.key("End");
    assert.deepEqual(await driver.values(id), [8, 10]);
    await driver.drag(id, 0.8, 0.3, 0);
    assert.deepEqual(await driver.values(id), [3, 10]);
  }

  await driver.drag("slider-disabled", 0.5, 0.8);
  assert.equal((await driver.values("slider-disabled"))[0], 5);
  await driver.drag("range-disabled", 0.2, 0.5, 0);
  assert.deepEqual(await driver.values("range-disabled"), [2, 8]);
  await driver.capture("slider-values");
}
