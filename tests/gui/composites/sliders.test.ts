import { check } from "../../harness/page/checks.js";
import { compositeTests, json } from "./support/composite-tests.js";
import type { prepare } from "./pages/sliders.js";

compositeTests<typeof prepare>("sliders", { budget: 60 }, async (run) => {
  const { page, step } = run;

  // A vertical slider keeps its minimum at the bottom: a held drag from the
  // thumb up to 80% commits 8, and a track press at 20% jumps to 2. The press
  // focuses it, so arrows step it, Up and Right increasing, and Shift takes
  // the fine quarter.
  await run.case("vertical slider drag, track press and keys", async () => {
    const vertical = await run.thumb("vertical", 0.5);
    await page.mouse.move(vertical.page[0], vertical.page[1]);
    await page.mouse.down();
    const raised = await run.thumb("vertical", 0.8);
    await page.mouse.move(raised.page[0], raised.page[1], { steps: 4 });
    const dragged = await step("expectScalar", "vertical", 8);
    await page.mouse.up();
    const track = await run.thumb("vertical", 0.2);
    await page.mouse.click(track.page[0], track.page[1]);
    const tracked = await step("expectScalar", "vertical", 2);
    await step("expectFocus", "vertical", false);
    await step("expectNoNativeText");
    const stepped: Record<string, number> = {};
    for (const [key, value] of [
      ["ArrowUp", 3],
      ["ArrowDown", 2],
      ["Shift+ArrowUp", 2.25],
      ["Shift+ArrowDown", 2],
      ["ArrowRight", 3],
      ["ArrowLeft", 2],
      ["End", 10],
      ["Home", 0],
    ] as const) {
      await page.keyboard.press(key);
      stepped[key] = await step("expectScalar", "vertical", value);
    }
    return { dragged, tracked, stepped };
  });

  await run.case("slider wheel in a scroll view", async () => {
    // The wheel over a slider that does not hold focus scrolls the scroll
    // view around it and leaves the value.
    let cut = await step("cut");
    const unfocusedAt = await run.at("slider", [0.25, 0.5]);
    await page.mouse.move(unfocusedAt.page[0], unfocusedAt.page[1]);
    await page.mouse.wheel(0, 100);
    const passed = await step("wheelOutcome", cut);
    check(
      passed.disposition === "routed" && passed.remaining !== undefined,
      `A wheel over an unfocused slider was ${json(passed)}`,
    );
    const scrolled = await step("expectScrolledPast", "scroll", 0);
    const unfocusedValue = await step("scalar", "slider");
    check(
      unfocusedValue === 5,
      `A wheel over an unfocused slider moved it to ${unfocusedValue}`,
    );

    // Once a press on its thumb focuses it, the wheel over it steps it, down
    // decreasing and Shift taking the fine half, and the scroll view stays;
    // off the slider it scrolls again.
    const grabbed = await run.thumb("slider", 0.5);
    await page.mouse.click(grabbed.page[0], grabbed.page[1]);
    await step("expectFocus", "slider", false);
    await step("expectScalar", "slider", 5);
    const focusedOffset = await step("scrollOffset", "scroll");
    const on = await run.at("slider", [0.25, 0.5]);
    await page.mouse.move(on.page[0], on.page[1]);
    cut = await step("cut");
    await page.mouse.wheel(0, 100);
    const wheeled = await step("wheelOutcome", cut);
    const lowered = await step("expectScalar", "slider", 4);
    await page.keyboard.down("Shift");
    await page.mouse.wheel(0, -100);
    await page.keyboard.up("Shift");
    const raisedFinely = await step("expectScalar", "slider", 4.5);
    const keptOffset = await step("scrollOffset", "scroll");
    check(
      wheeled.disposition === "routed" &&
        wheeled.remaining === undefined &&
        keptOffset === focusedOffset,
      `A wheel over the focused slider scrolled: ${json(wheeled)}, offset ${focusedOffset} -> ${keptOffset}`,
    );
    const below = await run.at("scroll", [0.25, 0.9]);
    await page.mouse.move(below.page[0], below.page[1]);
    await page.mouse.wheel(0, 100);
    const scrolledOff = await step(
      "expectScrolledPast",
      "scroll",
      focusedOffset,
    );
    const kept = await step("scalar", "slider");
    check(kept === 4.5, `A wheel off the focused slider moved it to ${kept}`);
    await run.capture("sliders");
    return {
      unfocused: { outcome: passed, scrolled, value: unfocusedValue },
      focused: {
        outcome: wheeled,
        lowered,
        raisedFinely,
        offset: keptOffset,
      },
      offSlider: { scrolled: scrolledOff, value: kept },
    };
  });

  await run.case("dial wheel, press, relative drags and keys", async () => {
    // A dial in its own scroll view. The wheel over it while it does not hold
    // focus scrolls the view and leaves the value.
    let cut = await step("cut");
    const centre = await run.at("dial", [0.5, 0.5]);
    await page.mouse.move(centre.page[0], centre.page[1]);
    await page.mouse.wheel(0, 100);
    const passed = await step("wheelOutcome", cut);
    check(
      passed.disposition === "routed" && passed.remaining !== undefined,
      `A wheel over an unfocused dial was ${json(passed)}`,
    );
    const scrolled = await step("expectScrolledPast", "dial-scroll", 0);
    const unwheeled = await step("scalar", "dial");
    check(
      unwheeled === 5,
      `A wheel over an unfocused dial moved it to ${unwheeled}`,
    );

    // A press alone, even towards the minimum's end of the sweep, leaves the
    // value and focuses the dial without its ring.
    const press = await run.at("dial", [0.25, 0.75]);
    await page.mouse.click(press.page[0], press.page[1]);
    await step("expectFocus", "dial", false);
    const pressed = await step("scalar", "dial");
    check(pressed === 5, `A press on the dial moved it to ${pressed}`);

    // A vertical drag turns it relative to the press: upward travel of two and
    // a half dial sides crosses the range, so a fifth of that is two steps up,
    // and as far below the press two steps down. Sideways travel leaves it.
    const [, , width, height] = await step("bounds", "dial");
    const fifth = 0.2 * 2.5 * Math.min(width!, height!);
    const held = await run.at("dial", [0.5, 0.5]);
    await page.mouse.move(held.page[0], held.page[1]);
    await page.mouse.down();
    await page.mouse.move(held.page[0], held.page[1] - fifth, { steps: 4 });
    const turnedUp = await step("expectScalar", "dial", 7);
    await page.mouse.move(held.page[0], held.page[1] + fifth, { steps: 4 });
    const turnedDown = await step("expectScalar", "dial", 3);
    await page.mouse.up();
    cut = await step("cut");
    await page.mouse.move(held.page[0], held.page[1]);
    await page.mouse.down();
    await page.mouse.move(held.page[0] + fifth, held.page[1], { steps: 4 });
    await page.mouse.up();
    await step("pressOutcome", cut);
    await step("expectNoEffects", "pressed", cut);
    const sideways = await step("scalar", "dial");
    check(sideways === 3, `A sideways drag turned the dial to ${sideways}`);

    // Arrows, Shift's fine half, End and Home step it as any slider.
    const stepped: Record<string, number> = {};
    for (const [key, value] of [
      ["ArrowUp", 4],
      ["ArrowLeft", 3],
      ["Shift+ArrowUp", 3.5],
      ["Shift+ArrowDown", 3],
      ["End", 10],
      ["Home", 0],
    ] as const) {
      await page.keyboard.press(key);
      stepped[key] = await step("expectScalar", "dial", value);
    }

    // The wheel over the focused dial steps it, up increasing and Shift
    // taking the fine half, and the view stays; beside the dial it scrolls
    // the view again.
    const offset = await step("scrollOffset", "dial-scroll");
    const on = await run.at("dial", [0.5, 0.5]);
    await page.mouse.move(on.page[0], on.page[1]);
    cut = await step("cut");
    await page.mouse.wheel(0, -100);
    const wheeled = await step("wheelOutcome", cut);
    const raised = await step("expectScalar", "dial", 1);
    await page.keyboard.down("Shift");
    await page.mouse.wheel(0, -100);
    await page.keyboard.up("Shift");
    const fine = await step("expectScalar", "dial", 1.5);
    const kept = await step("scrollOffset", "dial-scroll");
    check(
      wheeled.disposition === "routed" &&
        wheeled.remaining === undefined &&
        kept === offset,
      `A wheel over the focused dial scrolled: ${json(wheeled)}, offset ${offset} -> ${kept}`,
    );
    const beside = await run.at("dial-scroll", [0.8, 0.5]);
    await page.mouse.move(beside.page[0], beside.page[1]);
    await page.mouse.wheel(0, -100);
    const scrolledOff = await step("expectScrollMoved", "dial-scroll", offset);
    const keptValue = await step("scalar", "dial");
    check(
      keptValue === 1.5,
      `A wheel beside the focused dial moved it to ${keptValue}`,
    );
    await run.capture("dial");
    return {
      unfocused: { outcome: passed, scrolled, value: unwheeled },
      pressed,
      turnedUp,
      turnedDown,
      sideways,
      stepped,
      focused: { outcome: wheeled, raised, fine, offset: kept },
      beside: { scrolled: scrolledOff, value: keptValue },
    };
  });
});
