import { check } from "../../harness/page/checks.js";
import {
  compositeTests,
  type CompositeImage,
} from "./support/composite-tests.js";
import type { prepare } from "./pages/kit-collections.js";

function differentPixels(a: CompositeImage, b: CompositeImage) {
  let changed = 0;
  for (let i = 0; i < a.pixels.length; i += 4) {
    if (
      Math.abs(a.pixels[i]! - b.pixels[i]!) +
        Math.abs(a.pixels[i + 1]! - b.pixels[i + 1]!) +
        Math.abs(a.pixels[i + 2]! - b.pixels[i + 2]!) >
      30
    )
      changed++;
  }
  return changed;
}

compositeTests<typeof prepare>(
  "kit-collections",
  { budget: 120 },
  async (run) => {
    const { page, step } = run;
    const point = async (id: string) =>
      run.pagePoint(await step("collectionPoint", id));
    const click = async (id: string) => page.mouse.click(...(await point(id)));
    await run.case(
      "Command pages preserve search focus and dispatch once",
      async () => {
        const before = await run.capture("cybertron-collections");
        await click("palette/trigger");
        await step("shown", "palette", true);
        await step("native", "palette/search/input");
        await page.keyboard.insertText("Pro");
        await step("nativeText", "Pro");
        await step("absent", "palette/choices/save");
        await page.keyboard.press("ArrowDown");
        await step("activeRow", "palette/choices/projects");
        await page.keyboard.press("Enter");
        await click("palette/back");
        await step("native", "palette/search/input");
        await click("palette/choices/projects");
        await step("native", "palette/search/input");
        const opened = await run.capture("cybertron-command");
        check(
          differentPixels(before, opened) > 100,
          "The open command palette changes the completed image",
        );
        await page.keyboard.press("ArrowDown");
        await step("activeRow", "palette/pages/projects/choices/create");
        await page.keyboard.press("Enter");
        await step("event", "projects/create");
        await step("shown", "palette", false);
        await step("focused", "palette/trigger");
        return { changed: differentPixels(before, opened) };
      },
    );
    await run.case(
      "Combobox filters committed choices, removes and clears them",
      async () => {
        await click("combo/field");
        await step("native", "combo/field");
        await page.keyboard.insertText("Al");
        await step("shown", "combo/field/list", true);
        await page.keyboard.press("ArrowDown");
        await step("activeRow", "combo/field/suggestions/alpine");
        await page.keyboard.press("Enter");
        await step("event", "selection:alpine");
        await step("shown", "combo/field/list", false);
        await step("choice", "limited");
        await step("field", "combo/selected/alpine", "label", "Alpine");
        await run.capture("combo-selected-remote-page");
        await click("combo/selected/alpine");
        await step("event", "selection:");
        await click("combo/field");
        await step("native", "combo/field");
        await page.keyboard.insertText("Unknown");
        await step("nativeText", "Unknown");
        await page.keyboard.press("Enter");
        await step("absent", "combo/selected/Unknown");
        await step("choice", "controlled");
        await click("combo/clear");
        await step("event", "selection:", 2);
        await step("field", "combo/selected/bay", "label", "Bay");
        await step("choice", "free");
        await click("combo/field");
        await step("native", "combo/field");
        await page.keyboard.insertText("Custom");
        await step("nativeText", "Custom");
        await page.keyboard.press("Enter");
        await step("event", "selection:Custom");
        await click("combo/clear");
        await step("event", "selection:", 3);
        return {};
      },
    );
    await run.case(
      "Menubar switches eligible triggers and navigation retains its active destination",
      async () => {
        await click("bar/file");
        await step("shown", "bar/file/popup", true);
        await page.keyboard.press("Escape");
        await step("shown", "bar/file/popup", false);
        await page.keyboard.press("ArrowDown");
        await step("shown", "bar/file/popup", true);
        await page.keyboard.press("ArrowRight");
        await step("focused", "bar/edit");
        await step("shown", "bar/edit/popup", true);
        await page.keyboard.press("ArrowDown");
        await step("activeRow", "bar/edit/commands/copy");
        await page.keyboard.press("Enter");
        await step("event", "edit/copy");
        await step("shown", "bar/edit/popup", false);
        await click("nav/places");
        await step("shown", "nav/places/popup", true);
        await step("field", "nav/places/commands/home", "selected", true);
        await run.capture("navigation-current-and-disabled");
        await page.keyboard.press("ArrowRight");
        await step("shown", "nav/library/popup", true);
        await page.keyboard.press("ArrowLeft");
        await step("shown", "nav/places/popup", true);
        await click("nav/places/commands/map");
        await step("event", "route/map");
        return {};
      },
    );
    await run.case(
      "Interactive preview stays open between trigger and content",
      async () => {
        await page.mouse.move(...(await point("preview-trigger")));
        await step("shown", "preview", true);
        await page.mouse.move(...(await point("preview/detail")));
        await click("preview/detail");
        await step("event", "details");
        await run.capture("interactive-hover-card");
        await step("shown", "preview", true, "pressed interior");
        await page.keyboard.press("Escape");
        await step("shown", "preview", false);
        await step("focused", "preview-trigger");
        await click("mention/input");
        await step("native", "mention/input");
        await page.keyboard.press("Shift+Tab");
        await step("focused", "preview-trigger");
        await step("shown", "preview", true, "visible keyboard focus");
        await page.keyboard.press("Tab");
        await step("focused", "preview/detail");
        await page.keyboard.press("Escape");
        await step("shown", "preview", false);
        await step("focused", "preview-trigger");
        return {};
      },
    );
    await run.case(
      "Trigger candidates replace only the live range",
      async () => {
        await click("mention/input");
        await step("native", "mention/input");
        await page.keyboard.insertText("🙂 Meet\n@a");
        await step("nativeText", "🙂 Meet\n@a");
        await page.keyboard.insertText("li");
        await step("nativeText", "🙂 Meet\n@ali");
        await step("shown", "mention-menu/popup", true);
        const row = await step("collectionPoint", "mention-menu/options/alice");
        const caret = await step("caretPoint");
        await run.capture("caret-trigger-candidates");
        check(
          Math.abs(row[1]! - caret[1]!) < 40,
          "Trigger candidate opens beside the measured caret",
        );
        await page.keyboard.press("ArrowDown");
        await step("activeRow", "mention-menu/options/alice");
        await page.keyboard.press("Enter");
        await step("event", "insert/alice");
        await step("nativeText", "🙂 Meet\n@alice ");
        await page.keyboard.insertText("@al");
        await step("shown", "mention-menu/popup", true);
        await page.keyboard.press("Escape");
        await step("shown", "mention-menu/popup", false);
        await step("nativeText", "🙂 Meet\n@alice @al");
        await page.keyboard.insertText("i");
        await step("shown", "mention-menu/popup", true);
        await step("compose", "日");
        await step("shown", "mention-menu/popup", false);
        await step("finishComposition", "日");
        return { row, caret };
      },
    );
    await run.case(
      "Glass renders the same command and collection behavior",
      async () => {
        const cybertron = await run.capture("cybertron-rest");
        await step("skin", true);
        const rest = await run.capture("glass-rest");
        check(
          differentPixels(rest, cybertron) > 100,
          "Switching skins changes the resting controls",
        );
        await click("palette/trigger");
        await step("native", "palette/search/input");
        const glass = await run.capture("glass-command");
        await page.keyboard.type("Projects");
        await step("nativeText", "Projects");
        check(
          differentPixels(glass, rest) > 100,
          "Glass command surface is visible",
        );
        await page.keyboard.press("Escape");
        await step("shown", "palette", false);
        return { changed: differentPixels(glass, rest) };
      },
    );
    await run.case(
      "Transformed controls use local layout and input coordinates",
      async () => {
        await step("scaled", 1);
        const token = await step(
          "expectScaleBox",
          "scaled-token/input/0",
          60,
          1,
        );
        const at = async (name: string, x = 0.5) => {
          const r = await step("scaleBox", name);
          return run.pagePoint([r.x + r.width * x, r.y + r.height / 2]);
        };
        await page.mouse.click(...(await at("scaled-token/input/0", 0.2)));
        await step("native", "scaled-token/input/0");
        await page.keyboard.press("End");
        await step("event", "token/24/24");
        const margin = await step("tokenMargin");
        const before = await run.capture("one-times-layout");
        await step("scaled", 1.5);
        await step(
          "expectScaleBox",
          "scaled-token/input/0",
          token.localWidth,
          1.5,
        );
        await step("tokenMargin", margin);
        await step("expectScaleBox", "scaled-split/pane/a", 65, 1.5);
        await step("expectScaleBox", "scaled-split/pane/b", 65, 1.5);
        const after = await run.capture("transformed-layout");
        check(
          differentPixels(before, after) > 100,
          "Scaled controls change the presented frame",
        );
        await page.mouse.click(...(await at("scaled-rating/rating", 0.75)));
        await step("event", "rating/4");
        await page.mouse.click(...(await at("scaled-split/handle/a")));
        await step("native", "scaled-split/handle/a");
        await page.keyboard.press("ArrowRight");
        await step("event", "pane/75");
        await step("expectScaleBox", "scaled-split/pane/a", 75, 1.5);
        const trigger = await step("scaleBox", "scaled-menus/edit");
        await page.mouse.click(...(await at("scaled-menus/edit")));
        await step("shown", "scaled-menus/edit/popup", true);
        const popup = await step(
          "collectionPoint",
          "scaled-menus/edit/commands/copy",
        );
        const row = await step("scaleBox", "scaled-menus/edit/commands/copy");
        check(
          Math.abs(row.x - trigger.x) < 8,
          "Popup aligns with its transformed trigger: " +
            JSON.stringify({ trigger, row }),
        );
        await page.keyboard.press("Escape");
        await step("scaled", 1.5, true);
        await step("shown", "scaled-drawer", true);
        await step("layoutLength", "scaled-drawer", "width", 20);
        const grip = await at("scaled-drawer/grip");
        await page.mouse.move(...grip);
        await page.mouse.down();
        // A 39-canvas-unit drag is 26 local units. From width 20 the
        // nearest snap is 40; applying the raw canvas delta would pick 60.
        await page.mouse.move(grip[0] - 39, grip[1], { steps: 4 });
        await page.mouse.up();
        await step("event", "snap/1");
        await step("layoutLength", "scaled-drawer", "width", 40);
        return {
          token,
          margin,
          trigger,
          popup,
          changed: differentPixels(before, after),
        };
      },
    );
  },
);
