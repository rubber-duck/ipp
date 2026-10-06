import { check } from "../../harness/page/checks.js";
import { compositeTests, json } from "./support/composite-tests.js";
import type { prepare } from "./pages/overlays.js";

/**
 * Host seconds a pointer hovers a hint's parent before the hint opens, and
 * that the hint stays open once it leaves, restated from
 * `GUI_HINT_HOVER_DELAY` and `GUI_HINT_GRACE` in the runtime's hints.
 */
const HINT_DELAY = 0.4;
const HINT_GRACE = 0.1;

compositeTests<typeof prepare>("overlays", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const outsideAt = await run.at("a", [0.5, 0.5]);
  const beneathAt = await run.at("beneath", [0.2, 0.5]);

  await run.case("dropdown list: keys, outside press and Escape", async () => {
    // A press on its trigger opens its light list as a kit does on the
    // press, leaving focus on the trigger; arrows from the trigger move the
    // active row and Enter selects it.
    const items = ["item-1", "item-2", "item-3"];
    await step("watchOverlay", "menu-list");
    let cut = await step("cut");
    const menuAt = await run.at("menu", [0.5, 0.5]);
    await page.mouse.click(menuAt.page[0], menuAt.page[1]);
    await step("expectEffects", "menu", "pressed", cut);
    await step("setOverlay", "menu-list", true);
    const menuFocus = await step("expectFocus", "menu", false);
    await page.keyboard.press("ArrowDown");
    await step("expectActive", "item-1");
    await page.keyboard.press("ArrowDown");
    const menuActive = await step("expectActive", "item-2");
    cut = await step("cut");
    await page.keyboard.press("Enter");
    await step("expectEffects", "item-2", "pressed", cut);
    const menuSelected = await step("expectSelected", items, ["item-2"]);
    await run.capture("dropdown-open");

    // A press outside it, here on another panel's button, closes it and is
    // swallowed: nothing is pressed and focus stays.
    cut = await step("cut");
    await page.mouse.click(outsideAt.page[0], outsideAt.page[1]);
    const outsidePress = await step("pressOutcome", cut);
    check(
      outsidePress.disposition === "blocked",
      `The outside press was ${outsidePress.disposition}`,
    );
    await step("expectOverlay", "menu-list", false);
    await step("expectNoEffects", "pressed", cut);
    const outsideFocus = await step("expectFocus", "menu");
    // A client following the field observes the runtime's write.
    const closeObserved = await step("expectOverlayChanges", "menu-list", 2);

    // Escape closes it and leaves focus on the trigger, with the ring as
    // after any key.
    await step("setOverlay", "menu-list", true);
    cut = await step("cut");
    await page.keyboard.press("Escape");
    const menuEscape = await step("keyOutcome", "escape", cut);
    check(
      menuEscape.disposition === "routed",
      `Escape over the dropdown was ${menuEscape.disposition}`,
    );
    await step("expectOverlay", "menu-list", false);
    const escapeFocus = await step("expectFocus", "menu", true);
    return {
      menuFocus,
      menuActive,
      menuSelected,
      outsidePress,
      outsideFocus,
      closeObserved,
      menuEscape,
      escapeFocus,
    };
  });

  await run.case("popover focus entry and return", async () => {
    // A popover opened from the keyboard takes focus to its first control
    // with the ring; Tab moves inside it and Escape closes it, returning
    // focus to its opener.
    await step("clientFocus", "opener");
    let cut = await step("cut");
    await page.keyboard.press("Enter");
    await step("expectEffects", "opener", "pressed", cut);
    await step("setOverlay", "pop", true);
    const popEntered = await step("expectFocus", "pop-ok", true);
    // Focus feedback reports both sides of the runtime's move.
    const openerBlurred = await step(
      "expectFocusFeedback",
      "opener",
      cut,
      false,
    );
    await step("expectFocusFeedback", "pop-ok", cut, true);
    await page.keyboard.press("Tab");
    const popTabbed = await step("expectFocus", "pop-cancel", true);
    await run.capture("popover-open");
    cut = await step("cut");
    await page.keyboard.press("Escape");
    await step("expectOverlay", "pop", false);
    const popReturned = await step("expectFocus", "opener", true);
    await step("expectFocusFeedback", "pop-cancel", cut, false);
    await step("expectFocusFeedback", "opener", cut, true);

    // Opened by a pointer press, focus enters without the ring; an outside
    // press closes it, is swallowed and returns focus.
    cut = await step("cut");
    const openerAt = await run.at("opener", [0.5, 0.5]);
    await page.mouse.click(openerAt.page[0], openerAt.page[1]);
    await step("expectEffects", "opener", "pressed", cut);
    await step("setOverlay", "pop", true);
    const popPointer = await step("expectFocus", "pop-ok", false);
    cut = await step("cut");
    await page.mouse.click(beneathAt.page[0], beneathAt.page[1]);
    await step("expectOverlay", "pop", false);
    await step("expectNoEffects", "pressed", cut);
    const popOutside = await step("expectFocus", "opener", false);
    return {
      popEntered,
      openerBlurred,
      popTabbed,
      popReturned,
      popPointer,
      popOutside,
    };
  });

  await run.case("modal dialog", async () => {
    // A modal dialog takes focus from the control under it and keeps Tab
    // inside; a press on its canvas outside it reaches nothing and leaves it
    // open, while another panel stays usable.
    await step("clientFocus", "beneath");
    await step("setOverlay", "dialog", true);
    const dialogEntered = await step("expectFocus", "dialog-no", true);
    const dialogTabs = [];
    for (const [key, expected] of [
      ["Tab", "dialog-yes"],
      ["Tab", "dialog-no"],
      ["Shift+Tab", "dialog-yes"],
    ] as const) {
      await page.keyboard.press(key);
      dialogTabs.push(await step("expectFocus", expected, true));
    }
    await run.capture("dialog-open");
    let cut = await step("cut");
    await page.mouse.click(beneathAt.page[0], beneathAt.page[1]);
    const dialogBeneath = await step("pressOutcome", cut);
    check(
      dialogBeneath.disposition === "blocked",
      `A press under the dialog was ${dialogBeneath.disposition}`,
    );
    await step("expectNoEffects", "pressed", cut);
    await step("expectOverlay", "dialog", true);
    cut = await step("cut");
    await page.mouse.click(outsideAt.page[0], outsideAt.page[1]);
    const otherPanel = await step("expectEffects", "a", "pressed", cut);
    await step("expectOverlay", "dialog", true);

    // Escape from inside closes it and returns focus to the control it took
    // focus from.
    await step("clientFocus", "dialog-yes");
    await page.keyboard.press("Escape");
    await step("expectOverlay", "dialog", false);
    const dialogReturned = await step("expectFocus", "beneath", true);
    return {
      dialogEntered,
      dialogTabs,
      dialogBeneath,
      otherPanel,
      dialogReturned,
    };
  });

  // A tooltip opens after its hover delay on the Host clock and closes after
  // its grace once the pointer leaves. Each is measured from the tick of the
  // frame that ended with the hover change to the tick of the one that ended
  // with the hint so; the most that interval can be bounds the runtime's
  // wait from above whatever the machine's load, so it must reach the delay.
  await run.case("tooltip delay and grace on the Host clock", async () => {
    await step("prepareHint", "help", "tip");
    const helpAt = await run.at("help", [0.5, 0.5]);
    await page.mouse.move(helpAt.page[0], helpAt.page[1]);
    const opened = await step("hintTiming", true);
    check(
      opened.hint > opened.hover && opened.most >= HINT_DELAY - 1e-5,
      `The tooltip opened without its delay: ${json(opened)}`,
    );
    await run.capture("tooltip-open");
    await step("prepareHint", "help", "tip");
    const empty = await step("panelPoint", "overlays", [144, 12]);
    const emptyAt = run.pagePoint(empty);
    await page.mouse.move(emptyAt[0], emptyAt[1]);
    const closed = await step("hintTiming", false);
    check(
      closed.hint > closed.hover && closed.most >= HINT_GRACE - 1e-5,
      `The tooltip closed without its grace: ${json(closed)}`,
    );
    return { opened, closed };
  });
});
