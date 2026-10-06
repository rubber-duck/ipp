import { check } from "../../harness/page/checks.js";
import {
  amberPixels,
  compositeTests,
  json,
} from "./support/composite-tests.js";
import type { prepare } from "./pages/kit-overlays.js";

/**
 * Host seconds a pointer hovers a hint's parent before the hint opens,
 * restated from `GUI_HINT_HOVER_DELAY` in the runtime's hints.
 */
const HINT_DELAY = 0.4;

compositeTests<typeof prepare>("kit-overlays", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const overlayAt = async (
    symbol: string,
    fraction: readonly [number, number] = [0.5, 0.5],
  ) => run.pagePoint(await step("overlayKitPoint", symbol, fraction));
  /** A kit control's box in the canvas, as its corners place it. */
  const overlayRect = async (symbol: string) => {
    const start = await step("overlayKitPoint", symbol, [0, 0]);
    const end = await step("overlayKitPoint", symbol, [1, 1]);
    return [...start, end[0] - start[0], end[1] - start[1]];
  };

  // Context menu: a click focuses its target; Shift+F10 requests the
  // target's context and the menu opens there. Down moves the active row to
  // Inspect and past the disabled row to Delete; Enter runs Delete once for
  // its target and closes the menu, and focus never left the target. A
  // secondary press opens it at the press point, painting Delete's row in
  // amber; Escape closes it without running anything, focus still on the
  // target.
  await run.case("kit context menu", async () => {
    await page.mouse.click(...(await overlayAt("ov-target")));
    await step("expectOverlayKitFocus", "ov-target", false);
    await step("expectOverlayKitReports", "targetPresses", ["target"]);
    await page.keyboard.press("Shift+F10");
    await step("expectOverlayKitOpen", "ov-menu/surface", true);
    await page.keyboard.press("ArrowDown");
    await step("expectOverlayKitActive", ["ov-menu/items/inspect"]);
    await page.keyboard.press("ArrowDown");
    const skippedDisabled = await step("expectOverlayKitActive", [
      "ov-menu/items/delete",
    ]);
    await page.keyboard.press("Enter");
    await step("expectOverlayKitOpen", "ov-menu/surface", false);
    await step("expectOverlayKitDeclared", [], ["ov-menu/items/delete"]);
    const commands = await step("overlayKitReports", "commands");
    check(
      json(commands) === json(["delete:target"]),
      `The menu ran ${json(commands)}, not Delete once`,
    );
    const contextFocus = await step("expectOverlayKitFocus", "ov-target", true);
    check(
      (await step("overlayKitReports", "targetPresses")).length === 1,
      "Enter on the menu pressed its target",
    );
    await page.mouse.click(...(await overlayAt("ov-target", [0.75, 0.5])), {
      button: "right",
    });
    await step("expectOverlayKitOpen", "ov-menu/surface", true);
    const menuImage = await run.capture("kit-context-menu");
    const menuAmber = amberPixels(
      menuImage,
      await overlayRect("ov-menu/items/delete"),
    );
    check(menuAmber > 10, `Delete's row drew ${menuAmber} amber pixels`);
    await page.keyboard.press("Escape");
    await step("expectOverlayKitOpen", "ov-menu/surface", false);
    await step("expectOverlayKitFocus", "ov-target");
    check(
      (await step("overlayKitReports", "commands")).length === 1,
      "Escape ran a command",
    );
    return { skippedDisabled, menuAmber, commands, contextFocus };
  });

  // Confirmation dialog: its opening moves focus to Cancel; Tab reaches
  // Delete, drawn in amber, and comes back to Cancel, the close button
  // taking none; a press outside it presses nothing and confirms nothing;
  // Escape cancels once and returns focus to the button that opened it.
  await run.case("kit confirmation dialog", async () => {
    await page.mouse.click(...(await overlayAt("ov-opener")));
    await step("expectOverlayKitOpen", "ov-dialog", true);
    const dialogFocus = await step("expectOverlayKitFocus", "ov-dialog/cancel");
    await page.keyboard.press("Tab");
    await step("expectOverlayKitFocus", "ov-dialog/action", true);
    await page.keyboard.press("Tab");
    const tabbedInside = await step(
      "expectOverlayKitFocus",
      "ov-dialog/cancel",
      true,
    );
    const dialogImage = await run.capture("kit-dialog");
    const dialogAmber = amberPixels(
      dialogImage,
      await overlayRect("ov-dialog/action"),
    );
    const cancelAmber = amberPixels(
      dialogImage,
      await overlayRect("ov-dialog/cancel"),
    );
    check(
      dialogAmber > 20 && cancelAmber === 0,
      `The dialog's action drew ${dialogAmber} amber pixels, Cancel ${cancelAmber}`,
    );
    await page.mouse.click(...(await overlayAt("ov-target")));
    await step("expectOverlayKitOpen", "ov-dialog", true);
    check(
      (await step("overlayKitReports", "targetPresses")).length === 1 &&
        (await step("overlayKitReports", "dialog")).length === 0,
      "A press outside the dialog reached its target or answered it",
    );
    await page.keyboard.press("Escape");
    const dialogAnswers = await step("expectOverlayKitReports", "dialog", [
      "cancel",
    ]);
    await step("expectOverlayKitOpen", "ov-dialog", false);
    const dialogReturn = await step("expectOverlayKitFocus", "ov-opener");
    return {
      dialogFocus,
      tabbedInside,
      dialogAmber,
      dialogAnswers,
      dialogReturn,
    };
  });

  // Popover: a click on its trigger opens it and focus enters its content; a
  // press outside, on the target, closes it without reaching the target, the
  // application hears that it closed, and focus returns to the trigger.
  await run.case("kit popover", async () => {
    await page.mouse.click(...(await overlayAt("ov-popover")));
    await step("expectOverlayKitOpen", "ov-popover/popover", true);
    await step("expectOverlayKitFocus", "ov-pop-ok");
    await page.mouse.click(...(await overlayAt("ov-target")));
    await step("expectOverlayKitOpen", "ov-popover/popover", false);
    const popoverReports = await step("expectOverlayKitReports", "popover", [
      true,
      false,
    ]);
    check(
      (await step("overlayKitReports", "targetPresses")).length === 1,
      "The press that closed the popover reached the target",
    );
    const popoverReturn = await step("expectOverlayKitFocus", "ov-popover");
    return { popoverReports, popoverReturn };
  });

  // Tooltip: it opens after its hover delay on the Host clock, measured from
  // the tick of the frame that ended with the help button hovered to the
  // tick of the one that ended with the tooltip open.
  await run.case("kit tooltip delay on the Host clock", async () => {
    await step("prepareKitHint", "ov-help", "ov-help/tip");
    await page.mouse.move(...(await overlayAt("ov-help")));
    const opened = await step("hintTiming", true);
    check(
      opened.hint > opened.hover && opened.most >= HINT_DELAY - 1e-5,
      `The kit tooltip opened without its delay: ${json(opened)}`,
    );
    return opened;
  });
});
