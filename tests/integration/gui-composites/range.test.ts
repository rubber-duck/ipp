import { check, compositeTests, json } from "./harness.js";
import type { prepare } from "./range.js";

compositeTests<typeof prepare>("range", { budget: 60 }, async (run) => {
  const { page, step } = run;

  // A range slider's thumbs are focus parts of one control and Tab stops in
  // order: BackTab from the next panel's first control reaches the range at
  // its upper thumb, then its lower one; Tab returns through the upper thumb
  // to `c`. Focus feedback names the thumb. Enter on `c` shows the context
  // made the client's focus its keyboard target first.
  await run.case("range thumbs as Tab stops", async () => {
    await step("clientFocus", "c");
    const cut = await step("cut");
    await page.keyboard.press("Enter");
    await step("expectEffects", "c", "pressed", cut);
    const stops = [];
    for (const [key, name, part] of [
      ["Shift+Tab", "range", 1],
      ["Shift+Tab", "range", 0],
      ["Tab", "range", 1],
      ["Tab", "c", undefined],
    ] as const) {
      await page.keyboard.press(key);
      stops.push(await step("expectFocus", name, true, part));
    }
    const feedback = await step("focusFeedback", "range", cut);
    check(
      json(feedback) ===
        json([
          [true, 1],
          [true, 0],
          [true, 1],
          [false, 1],
        ]),
      `Range focus feedback ${json(feedback)}`,
    );
    return { stops, feedback };
  });

  // Arrows, Home and End move only the focused thumb, Home and End to its
  // legal bound: the range's end or the other thumb.
  await run.case("range keys move the focused thumb", async () => {
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Shift+Tab");
    await step("expectFocus", "range", true, 0);
    const keys = [];
    for (const [key, values] of [
      ["ArrowRight", [3, 8]],
      ["End", [8, 8]],
      ["Home", [0, 8]],
      ["Tab", [0, 8]],
      ["ArrowLeft", [0, 7]],
      ["Shift+ArrowLeft", [0, 6.5]],
      ["Home", [0, 0]],
      ["End", [0, 10]],
    ] as const) {
      await page.keyboard.press(key);
      keys.push([key, await step("expectRange", values)]);
    }
    await step("expectFocus", "range", true, 1);
    return keys;
  });

  await run.case("range drags, track press and writes", async () => {
    // A drag of the lower thumb past the upper one stops there, and dragged
    // back it is still the lower thumb: the thumbs never swap. Each value
    // record carries both values.
    const recordsBefore = await step("rangeRecordCount");
    const lower = await run.thumb("range", 0);
    await page.mouse.move(lower.page[0], lower.page[1]);
    await page.mouse.down();
    await step("expectFocus", "range", false, 0);
    const pastUpper = await run.thumb("range", 1);
    await page.mouse.move(pastUpper.page[0] + 4, pastUpper.page[1], {
      steps: 4,
    });
    const stopped = await step("expectRange", [10, 10]);
    const backAt = await run.thumb("range", 0.3);
    await page.mouse.move(backAt.page[0], backAt.page[1], { steps: 4 });
    const back = await step("expectRange", [3, 10]);
    await page.mouse.up();
    const records = await step("rangeRecords", recordsBefore);
    check(
      records.length > 0 &&
        records.every(([low, high]) => high === 10 && low >= 0 && low <= 10) &&
        json(records.at(-1)) === "[3,10]",
      `Range value records during the drag ${json(records)}`,
    );

    // A track press moves the nearer thumb to the pointer and takes it: 8 is
    // nearer the upper thumb at 10 than the lower at 3.
    const trackAt = await run.thumb("range", 0.8);
    await page.mouse.click(trackAt.page[0], trackAt.page[1]);
    const track = await step("expectRange", [3, 8]);
    await step("expectFocus", "range", false, 1);

    // A client write that would invert the values is refused and changes
    // nothing; both values change in one component write, reported as one
    // value record.
    const invertedLower = await step("writeRange", { value: 9 });
    const invertedUpper = await step("writeRange", { upper: 2 });
    check(
      invertedLower === "InvalidValue" && invertedUpper === "InvalidValue",
      `Inverting writes were ${invertedLower} and ${invertedUpper}`,
    );
    await step("expectRange", [3, 8]);
    const recordsBeforeBoth = await step("rangeRecordCount");
    const bothWritten = await step("writeRange", { value: 1, upper: 4 });
    check(bothWritten === null, `Writing both was ${bothWritten}`);
    const both = await step("expectRange", [1, 4]);
    const bothRecords = await step("rangeRecords", recordsBeforeBoth);
    check(
      json(bothRecords) === "[[1,4]]",
      `Writing both values reported ${json(bothRecords)}`,
    );
    await run.capture("range");
    return {
      drag: { stopped, back, records },
      track,
      refused: [invertedLower, invertedUpper],
      both: { values: both, records: bothRecords },
    };
  });
});
