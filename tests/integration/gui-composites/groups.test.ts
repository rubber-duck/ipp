import { check, compositeTests, json } from "./harness.js";
import type { prepare } from "./groups.js";

compositeTests<typeof prepare>("groups", { budget: 60 }, async (run) => {
  const { page, step } = run;

  await run.case("segmented and tab groups", async () => {
    // Groups whose items take focus are one Tab stop each, entered at their
    // selected item from either side.
    await step("clientFocus", "after");
    await step("expectFocus", "after", true);
    await page.keyboard.press("Shift+Tab");
    const enteredTabs = await step("expectFocus", "tab-1", true);
    await page.keyboard.press("Shift+Tab");
    const enteredSegments = await step("expectFocus", "seg-y", true);

    // A segmented group: arrows move focus and select in the same frame, and
    // stop at the ends.
    const segments = ["seg-x", "seg-y", "seg-z"];
    let cut = await step("cut");
    await page.keyboard.press("ArrowRight");
    const segmentKey = await step("keyOutcome", "right", cut);
    check(
      segmentKey.disposition === "routed",
      `ArrowRight in the segmented group was ${segmentKey.disposition}`,
    );
    await step("expectFocus", "seg-z", true);
    const segmentRight = await step("expectSelected", segments, ["seg-z"]);
    await page.keyboard.press("ArrowRight");
    await step("expectFocus", "seg-z", true);
    await page.keyboard.press("Home");
    await step("expectFocus", "seg-x", true);
    const segmentHome = await step("expectSelected", segments, ["seg-x"]);

    // A tab group: Tab leaves the segmented group for the selected tab,
    // arrows move focus without selecting, Enter activates and selects, and
    // Tab leaves the group.
    const tabs = ["tab-1", "tab-2", "tab-3"];
    await page.keyboard.press("Tab");
    await step("expectFocus", "tab-1", true);
    await page.keyboard.press("ArrowRight");
    await step("expectFocus", "tab-2", true);
    await step("expectSelected", tabs, ["tab-1"]);

    // A key the group does not use reaches the client unhandled.
    cut = await step("cut");
    await page.keyboard.press("ArrowDown");
    const unusedKey = await step("keyOutcome", "down", cut);
    check(
      unusedKey.disposition === "unhandled",
      `ArrowDown on a tab was ${unusedKey.disposition}`,
    );
    const reported = await step("unhandledSince", cut);
    check(
      reported.some((input) => input.kind === "key" && input.key === "down"),
      `ArrowDown did not reach the client: ${json(reported)}`,
    );
    cut = await step("cut");
    await page.keyboard.press("Enter");
    const [pressedTab] = await step("expectEffects", "tab-2", "pressed", cut);
    const tabSelected = await step("expectSelected", tabs, ["tab-2"]);
    await page.keyboard.press("Tab");
    const leftTabs = await step("expectFocus", "after", true);
    await page.keyboard.press("Shift+Tab");
    const reentered = await step("expectFocus", "tab-2", true);
    await run.capture("focus-groups");
    return {
      enteredTabs,
      enteredSegments,
      segmentKey,
      segmentRight,
      segmentHome,
      unusedKey,
      pressedTab,
      tabSelected,
      leftTabs,
      reentered,
    };
  });

  // An option list whose rows do not take focus, opened one layer up and
  // driven from a focused text input: Up and Down move its active item,
  // typing reaches the field, Enter activates and selects the active row
  // instead of submitting, and hover moves the active item.
  await run.case("option list active item", async () => {
    const options = ["opt-1", "opt-2", "opt-3"];
    await step("clientFocus", "query");
    await step("setOverlay", "options", true);
    await step("expectNativeText", "ab", "query");
    await step("expectActive", null, "groups");
    const cut = await step("cut");
    await page.keyboard.press("ArrowDown");
    await step("expectActive", "opt-1");
    await page.keyboard.press("ArrowDown");
    const activeByKey = await step("expectActive", "opt-2");
    const optionKey = await step("keyOutcome", "down", cut);
    check(
      optionKey.disposition === "routed",
      `ArrowDown in the field was ${optionKey.disposition}`,
    );
    await page.keyboard.insertText("c");
    await step("expectNativeText", "abc", "query");
    await page.keyboard.press("Enter");
    const [pressedOption] = await step(
      "expectEffects",
      "opt-2",
      "pressed",
      cut,
    );
    const optionSelected = await step("expectSelected", options, ["opt-2"]);
    await step("expectNoEffects", "submitted", cut);
    const hoverAt = await run.at("opt-3", [0.5, 0.5]);
    await page.mouse.move(hoverAt.page[0], hoverAt.page[1]);
    const activeByHover = await step("expectActive", "opt-3");
    await page.keyboard.press("ArrowUp");
    const activeAfterHover = await step("expectActive", "opt-2");
    const fieldKept = await step("expectFocus", "query", true);
    await step("expectNativeText", "abc", "query");
    await run.capture("option-list");
    return {
      activeByKey,
      optionKey,
      pressedOption,
      optionSelected,
      activeByHover,
      activeAfterHover,
      fieldKept,
    };
  });

  // Escape in the field closes its light option list and leaves its text
  // and focus alone.
  await run.case("option list Escape", async () => {
    const cut = await step("cut");
    await page.keyboard.press("Escape");
    const escape = await step("keyOutcome", "escape", cut);
    check(
      escape.disposition === "routed",
      `Escape in the field was ${escape.disposition}`,
    );
    await step("expectOverlay", "options", false);
    const focus = await step("expectFocus", "query", true);
    await step("expectNativeText", "abc", "query");
    return { escape, focus };
  });
});
