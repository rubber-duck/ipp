import { check } from "../../harness/page/checks.js";
import {
  accentPixels,
  compositeTests,
  json,
} from "./support/composite-tests.js";
import { TOAST_DURATION, type prepare } from "./pages/toast.js";

/** Seconds of the toast's fade before it ends, restated from the kit's toast. */
const FADE = 0.1;

/** The toast's whole time on the Host clock: shown, then fading. */
const TOAST_SECONDS = TOAST_DURATION / 1000 + FADE;

compositeTests<typeof prepare>("toast", { budget: 60 }, async (run) => {
  const { page, step } = run;
  /** A point over both the toast and `a`, in the panel's canvas. */
  let over: readonly [number, number] = [0, 0];
  /** The toast's check mark, over a's interior, a content inset in. */
  let mark: readonly number[] = [];
  const pageAt = async (point: readonly [number, number]) =>
    run.pagePoint(await step("panelPoint", "buttons", point));

  await run.case("kit toast over a focused panel", async () => {
    // The kit's toast stack, a top-level manual overlay of the panel, shows a
    // toast over button `a` while the text input holds focus.
    await step("clientFocus", "text");
    await step("expectFocus", "text", true);
    const toast = await step("showToast");
    const [ax, ay, aw, ah] = await step("bounds", "a");
    const [toastX, toastY, toastWidth, toastHeight] = toast;
    over = [
      toastX! + toastWidth! / 2,
      Math.max(toastY!, ay!) +
        (Math.min(toastY! + toastHeight!, ay! + ah!) - Math.max(toastY!, ay!)) /
          2,
    ];
    check(
      over[0] > ax! &&
        over[0] < ax! + aw! &&
        over[1] > ay! &&
        over[1] < ay! + ah!,
      `The toast ${json(toast)} does not lie over a`,
    );
    const overAt = await pageAt(over);
    // The pointer rests on it first, which pauses its time, so the capture
    // below cannot outlast it on a slow renderer.
    let cut = await step("cut");
    await page.mouse.move(overAt[0], overAt[1]);
    await step("toastEffects", "interactionChanged", cut, 1);

    mark = [toastX! + 6, toastY!, 6, toastHeight!];
    const shown = await run.capture("toast-shown");
    const markPixels = accentPixels(shown, mark);
    check(markPixels > 0, `No check mark painted over a at ${json(mark)}`);

    // A press there reaches the toast's body, not the button beneath it, and
    // leaves focus on the text input.
    cut = await step("cut");
    await page.mouse.click(overAt[0], overAt[1]);
    const toastPresses = await step("toastEffects", "pressed", cut, 1);
    check(
      toastPresses.length === 1,
      `Presses on the toast at ${json(over)} of ${json(toast)}: ${json(
        await step("panelEffects", "buttons", cut),
      )}`,
    );
    const beneath = await step("settledEffects", "a", "pressed", cut);
    check(beneath.length === 0, "A press on the toast reached a");
    const toastFocus = await step("expectFocus", "text");
    return { toast, over, markPixels, toastPresses, toastFocus };
  });

  // Its time, TOAST_DURATION of the Host clock and its fade, pauses while the
  // pointer hovers it: once the panel's World time has run past that whole
  // time since the toast was shown, the toast is still there and its
  // controller paused short of its end. A pointer's release ends its
  // interactions, hover included, until it moves again, so the pointer moves
  // within the toast.
  await run.case(
    "kit toast paused while hovered on the Host clock",
    async () => {
      const within = await pageAt([over[0] + 4, over[1]]);
      const cut = await step("cut");
      await page.mouse.move(within[0], within[1]);
      const held = await step("toastAfter", TOAST_SECONDS + 0.5);
      const [controller] = held.controllers;
      check(
        held.present &&
          held.dismissals.length === 0 &&
          held.controllers.length === 1 &&
          controller?.state === "paused" &&
          controller.time < TOAST_DURATION / 1000,
        `The toast did not pause while hovered: ${json({
          held,
          hover: await step("toastEffects", "interactionChanged", cut),
        })}`,
      );
      return held;
    },
  );

  // ...and dismisses itself once the pointer leaves, no sooner than the rest
  // of its time on the Host clock: from the World time read before the
  // pointer left to the first read without the toast.
  await run.case("kit toast dismissal on the Host clock", async () => {
    const before = await step("prepareToastLeave");
    const away = await pageAt([48, 90]);
    await page.mouse.move(away[0], away[1]);
    const dismissed = await step("expectToastDismissed");
    const remaining = TOAST_SECONDS - before.controllers[0]!.time;
    const elapsed = dismissed.removedAt - before.time;
    check(
      json(dismissed.dismissals) === json(["saved"]) &&
        elapsed >= remaining - 1e-3,
      `The toast was dismissed after ${elapsed} s of the Host clock with ${remaining} s left: ${json({ before, dismissed })}`,
    );
    const closed = await run.capture("toast-dismissed");
    check(
      accentPixels(closed, mark) === 0,
      "The dismissed toast still paints over a",
    );

    // The closed stack no longer covers a.
    const cut = await step("cut");
    const overAt = await pageAt(over);
    await page.mouse.click(overAt[0], overAt[1]);
    const reached = await step("expectEffects", "a", "pressed", cut);
    return { before, dismissed, remaining, elapsed, reached };
  });
});
