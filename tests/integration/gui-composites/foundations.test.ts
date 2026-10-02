import { check, compositeTests, contextPoint, json, near } from "./harness.js";
import type { prepare } from "./foundations.js";

compositeTests<typeof prepare>("foundations", { budget: 60 }, async (run) => {
  const { page, step } = run;

  // A client focuses the text input: the presenting context makes it the
  // keyboard target and its native buffer takes DOM focus without a
  // gesture, so typing reaches it.
  await run.case("client focus then typing", async () => {
    await step("clientFocus", "text");
    await step("expectNativeText", "ab", "text");
    await page.keyboard.insertText("c");
    const outcome = {
      native: await step("expectNativeText", "abc", "text"),
      focus: await step("expectFocus", "text", true),
    };
    await run.capture("client-focus-typing");
    return outcome;
  });

  await run.case("client focus traversal across Worlds", async () => {
    // A client focus elsewhere in the same World ends the native buffer;
    // the next key reaches the new control, and Tab and BackTab continue
    // from it.
    await step("clientFocus", "a");
    await step("expectNoNativeText");
    let cut = await step("cut");
    await page.keyboard.press("Enter");
    const pressedA = await step("expectEffects", "a", "pressed", cut);
    await page.keyboard.press("Tab");
    const tabbed = await step("expectFocus", "b", true);
    await page.keyboard.press("Shift+Tab");
    const backTabbed = await step("expectFocus", "a", true);

    // A client focus in the other panel's World moves the target and blurs
    // the previous one, so one World holds focus.
    await step("clientFocus", "c");
    const crossed = await step("expectFocus", "c", true);
    cut = await step("cut");
    await page.keyboard.press("Enter");
    const pressedC = await step("expectEffects", "c", "pressed", cut);
    // Nothing outside `c`'s panel was pressed.
    await step("expectNoEffects", "pressed", {
      ...cut,
      effects: Object.fromEntries(
        Object.entries(cut.effects).map(([name, from]) => [
          name,
          name === "sliders" ? Number.MAX_SAFE_INTEGER : from,
        ]),
      ),
    });
    // Tab continues through the sliders panel's sliders, past the disabled
    // `d`, and into the groups panel, whose segmented group is one stop
    // entered at its selected item.
    const continued = [];
    for (const name of ["vertical", "slider", "seg-y"]) {
      await page.keyboard.press("Tab");
      continued.push(await step("expectFocus", name, true));
    }
    await run.capture("client-focus-traversal");
    return { pressedA, tabbed, backTabbed, crossed, pressedC, continued };
  });

  // A client blur ends the keyboard target and its native buffer: keys
  // find no target and typing changes nothing.
  await run.case("client blur", async () => {
    await step("clientFocus", "text");
    await step("expectNativeText", "abc", "text");
    await step("clientBlur", "text");
    await step("expectNoNativeText");
    await step("expectFocus", null);
    const cut = await step("cut");
    await page.keyboard.press("Enter");
    await page.keyboard.insertText("x");
    const key = await step("keyOutcome", "enter", cut);
    check(
      key.disposition === "unhandled",
      `Enter after a client blur was ${key.disposition}`,
    );
    await step("expectNoEffects", "pressed", cut);
    await step("expectNoEffects", "submitted", cut);
    const text = await step("textValue", "text");
    check(
      text === "abc",
      `Typing after a client blur edited the text: ${text}`,
    );
    return { key, text };
  });

  await run.case("context requests", async () => {
    // A secondary press requests a context at its canvas point and focuses
    // the control as a press does, without the ring; the browser's own
    // menu stays closed.
    let cut = await step("cut");
    const pressAt = await run.at("b", [0.25, 0.5]);
    await page.mouse.click(pressAt.page[0], pressAt.page[1], {
      button: "right",
    });
    const [rightClicked] = await step(
      "expectEffects",
      "b",
      "contextRequested",
      cut,
    );
    check(
      near(contextPoint(rightClicked), pressAt.panel, 1),
      `Right-click context ${json(contextPoint(rightClicked))}, expected ${pressAt.panel}`,
    );
    const pointerFocus = await step("expectFocus", "b", false);
    const menus = await step("browserMenus");
    check(
      menus.length > 0 && menus.every((prevented) => prevented),
      `Browser context menus were not suppressed: ${menus}`,
    );

    // The Menu key and Shift+F10 request the focused control's context at
    // the bottom-left corner of its box; F10 alone does not, so Shift
    // reaches the control's key handling.
    const [bx, by, , bh] = await step("bounds", "b");
    const corner = [bx!, by! + bh!];
    const keyed = [];
    for (const key of ["ContextMenu", "Shift+F10"]) {
      cut = await step("cut");
      await page.keyboard.press(key);
      const [requested] = await step(
        "expectEffects",
        "b",
        "contextRequested",
        cut,
      );
      check(
        near(contextPoint(requested), corner),
        `${key} context ${json(contextPoint(requested))}, expected ${corner}`,
      );
      keyed.push(requested);
    }
    cut = await step("cut");
    await page.keyboard.press("F10");
    await step("expectNoEffects", "contextRequested", cut);

    // Shift+F10 in the text input's native buffer requests its context
    // without editing it.
    await step("clientFocus", "text");
    await step("expectNativeText", "abc", "text");
    const [tx, ty, , th] = await step("bounds", "text");
    cut = await step("cut");
    await page.keyboard.press("Shift+F10");
    const [textContext] = await step(
      "expectEffects",
      "text",
      "contextRequested",
      cut,
    );
    check(
      near(contextPoint(textContext), [tx!, ty! + th!]),
      `Text input context ${json(contextPoint(textContext))}`,
    );
    await step("expectNativeText", "abc", "text");

    // A secondary press on a disabled control requests nothing and leaves
    // focus where it was.
    cut = await step("cut");
    const disabledAt = await run.at("d", [0.5, 0.5]);
    await page.mouse.click(disabledAt.page[0], disabledAt.page[1], {
      button: "right",
    });
    const disabled = await step("secondaryOutcome", cut);
    check(
      disabled.disposition !== "routed",
      `Disabled context was ${disabled.disposition}`,
    );
    await step("expectNoEffects", "contextRequested", cut);
    const kept = await step("expectFocus", "text", true);
    await run.capture("context-requests");
    return {
      rightClicked,
      pointerFocus,
      menus,
      keyed,
      textContext,
      disabled,
      kept,
    };
  });
});
