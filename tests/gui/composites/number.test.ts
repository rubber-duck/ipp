import { check } from "../../harness/page/checks.js";
import { compositeTests, json } from "./support/composite-tests.js";
import type { prepare } from "./pages/number.js";

/**
 * Host seconds a step part is held before it repeats, restated from
 * `GUI_NUMBER_REPEAT_DELAY` in the runtime's numeric input.
 */
const REPEAT_DELAY = 0.4;

compositeTests<typeof prepare>("number", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const at = (x: number) => run.at("number", [x, 0.5]);

  // A numeric field with step parts at its ends: typing stays an edit until
  // Enter commits it clamped to the range, text that does not parse is
  // rejected and leaves the number, Escape discards the edit, restores the
  // formatted number and reports the discarded text, and Up and Down step.
  await run.case("numeric entry, rejection, Escape and keys", async () => {
    const field = await at(0.6);
    let cut = await step("cut");
    await page.mouse.click(field.page[0], field.page[1]);
    await step("expectFocus", "number", true);
    await step("expectEdit", "number", "1.25");
    await step("expectTyping", "number");
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("9");
    await step("expectEdit", "number", "9");
    const uncommitted = await step("scalar", "number");
    await page.keyboard.press("Enter");
    const entered = [
      await step("expectScalar", "number", 4),
      await step("expectEdit", "number", "4.00"),
    ];
    const [submitted] = await step("expectEffects", "number", "submitted", cut);
    check(
      uncommitted === 1.25 &&
        submitted?.effect.kind === "submitted" &&
        submitted.effect.text === "4.00",
      `Number entry ${uncommitted} then ${json(submitted?.effect)}`,
    );
    cut = await step("cut");
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("abc");
    await page.keyboard.press("Enter");
    const [rejected] = await step("expectEffects", "number", "rejected", cut);
    const kept = [
      await step("expectEdit", "number", "abc"),
      await step("scalar", "number"),
    ];
    check(
      rejected?.effect.kind === "rejected" &&
        rejected.effect.text === "abc" &&
        kept[1] === 4,
      `Rejected entry ${json(rejected?.effect)} left ${kept}`,
    );
    cut = await step("cut");
    await page.keyboard.press("Escape");
    const escape = await step("keyOutcome", "escape", cut);
    const restored = [
      await step("expectEdit", "number", "4.00"),
      await step("expectFocus", "number", true),
    ];
    const [discarded] = await step("expectEffects", "number", "discarded", cut);
    check(
      discarded?.effect.kind === "discarded" && discarded.effect.text === "abc",
      `Escape reported ${json(discarded?.effect)}`,
    );
    const keys = [];
    for (const [key, value] of [
      ["ArrowDown", 3.75],
      ["ArrowUp", 4],
    ] as const) {
      await page.keyboard.press(key);
      keys.push(await step("expectScalar", "number", value));
    }
    return {
      entered,
      submitted,
      rejected,
      kept,
      escape,
      restored,
      discarded,
      keys,
    };
  });

  // At the bound the increment part leaves the pending edit alone; the
  // decrement part commits it and steps from it. Blur commits an edit, which
  // reports no discard.
  await run.case("step parts at the bound and blur", async () => {
    const [minus, plus] = [await at(1 / 8), await at(7 / 8)];
    const cut = await step("cut");
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("2");
    await step("expectEdit", "number", "2");
    await page.mouse.click(plus.page[0], plus.page[1]);
    await step("expectNoEffects", "rejected", cut);
    const plusInert = [
      await step("expectEdit", "number", "2"),
      await step("scalar", "number"),
    ];
    check(plusInert[1] === 4, `Plus at the bound moved ${plusInert}`);
    await page.mouse.click(minus.page[0], minus.page[1]);
    const minusStepped = await step("expectScalar", "number", 1.75);
    await step("expectTyping", "number");
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText("3");
    await step("expectEdit", "number", "3");
    await page.keyboard.press("Tab");
    const blurCommitted = await step("expectScalar", "number", 3);
    await step("expectNoEffects", "discarded", cut);
    return { plusInert, minusStepped, blurCommitted };
  });

  // A held increment part steps at once, repeats after its delay on the Host
  // clock, one value record per change, and stops at the bound while still
  // held. The delay is measured between the ticks of the frames that ended
  // with the first step and with the first repeat.
  await run.case("held step part repeats on the Host clock", async () => {
    const plus = await at(7 / 8);
    const cut = await step("cut");
    const before = await step("numberRecordCount");
    await step("prepareNumberHold");
    await page.mouse.move(plus.page[0], plus.page[1]);
    await page.mouse.down();
    await step("expectScalar", "number", 4);
    await step("expectNoEffects", "rejected", cut);
    const stillHeld = await step("scalar", "number");
    await page.mouse.up();
    const records = await step("numberRecords", before);
    const timing = await step("numberHoldTiming", before);
    check(
      timing.repeat > timing.press &&
        timing.most >= REPEAT_DELAY - 1e-5 &&
        records[0] === 3.25 &&
        records.at(-1) === 4 &&
        records.length >= 2 &&
        records.every(
          (value, index) =>
            Number.isInteger(value * 4) &&
            (index === 0 || value > records[index - 1]!),
        ) &&
        stillHeld === 4,
      `Held part ${json({ timing, records, stillHeld })}`,
    );
    await run.capture("number");
    return { timing, records, stillHeld };
  });
});
