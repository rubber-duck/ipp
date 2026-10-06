import { check } from "../../harness/page/checks.js";
import { compositeTests, json, near } from "./support/composite-tests.js";
import type { prepare } from "./pages/kit-values.js";

compositeTests<typeof prepare>("kit-values", { budget: 60 }, async (run) => {
  const { page, step } = run;
  /** The page position of a slider thumb's centre at a fraction of its range. */
  const thumbAt = async (symbol: string, fraction: number) => {
    const [x, y, width, height] = await step("valuesBounds", symbol);
    const edge = 0.75 * Math.min(width, height);
    return run.pagePoint([
      x + edge / 2 + fraction * (width - edge),
      y + height / 2,
    ]);
  };
  /** Press at `from` and move to `to`, still held. */
  const drag = async (
    from: readonly [number, number],
    to: readonly [number, number],
  ) => {
    await page.mouse.move(from[0], from[1]);
    await page.mouse.down();
    await page.mouse.move(to[0], to[1], { steps: 4 });
  };

  // The kit's range slider: the lower thumb from 20 to 40 moves its readout
  // and leaves the upper one; the upper thumb from 80 to 60 likewise. The
  // application hears both values once per change, never inverted.
  await run.case("kit range slider drags", async () => {
    await drag(
      await thumbAt("val-range/slider", 0.2),
      await thumbAt("val-range/slider", 0.4),
    );
    const lowerDragged = await step("expectValues", {
      rangeValue: [40, 80],
      rangeReadouts: ["40 m", "80 m"],
    });
    await page.mouse.up();
    await drag(
      await thumbAt("val-range/slider", 0.8),
      await thumbAt("val-range/slider", 0.6),
    );
    const upperDragged = await step("expectValues", {
      rangeValue: [40, 60],
      rangeReadouts: ["40 m", "60 m"],
    });
    await page.mouse.up();
    const reported = upperDragged.range;
    check(
      reported.every(
        (value, index) =>
          value.length === 2 &&
          value[0]! <= value[1]! &&
          (index === 0 || json(value) !== json(reported[index - 1])),
      ) &&
        json(reported.at(-1)) === "[40,60]" &&
        reported.some((value) => json(value) === "[40,80]") &&
        near(upperDragged.rangeMarks, [-0.2, 0.2], 1e-6),
      `Range reports ${json(reported)}, marks ${json(upperDragged.rangeMarks)}`,
    );
    return { lowerDragged, upperDragged };
  });

  // The kit's knob turns relative to the press: a quarter of the upward
  // travel that crosses its range, two and a half dial sides, adds 25.
  await run.case("kit knob drag", async () => {
    const [x, y, width] = await step("valuesBounds", "val-knob/dial");
    const dial = run.pagePoint([x + width / 2, y + width / 2]);
    await drag(dial, [dial[0], dial[1] - 0.25 * 2.5 * width]);
    const knobTurned = await step("expectValues", {
      knobValue: 75,
      knobReadout: "75%",
    });
    await page.mouse.up();
    check(
      knobTurned.knob.at(-1) === 75 &&
        knobTurned.knob.every((value) => value > 50 && value <= 75),
      `Knob reports ${json(knobTurned.knob)}`,
    );
    await run.capture("kit-values");
    return knobTurned;
  });

  // The kit's numeric stepper: an entry that does not parse keeps the number
  // and shows the error line under the field, which does not move; Escape
  // discards the entry, clears the error and shows the formatted number; a
  // valid entry commits and clears a later error; the unit stays beside it.
  await run.case("kit numeric stepper entry", async () => {
    const before = await step("valuesBounds", "val-step/field");
    const [x, y, width, height] = before;
    await page.mouse.click(...run.pagePoint([x + width * 0.6, y + height / 2]));
    await step("expectValuesEdit", "1.25");
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("abc");
    await step("expectValuesEdit", "abc");
    await page.keyboard.press("Enter");
    const rejected = await step("expectValues", {
      stepperValue: 1.25,
      stepperUnits: "EV",
      stepperError: "Not a number",
    });
    const withError = await step("valuesBounds", "val-step/field");
    await page.keyboard.press("Escape");
    await step("expectValuesEdit", "1.25");
    const discarded = await step("expectValues", {
      stepperValue: 1.25,
      stepperError: "",
      stepper: [],
    });
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("x");
    await page.keyboard.press("Enter");
    await step("expectValues", { stepperError: "Not a number" });
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("2.5");
    await step("expectValuesEdit", "2.5");
    await page.keyboard.press("Enter");
    const committed = await step("expectValues", {
      stepperValue: 2.5,
      stepperUnits: "EV",
      stepperError: "",
      stepper: [2.5],
    });
    check(
      json(withError) === json(before),
      `The stepper's field moved: ${json(before)} -> ${json(withError)}`,
    );
    await run.capture("kit-stepper");
    return { rejected, discarded, committed };
  });
});
