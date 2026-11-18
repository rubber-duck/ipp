import { check } from "../../harness/page/checks.js";
import { compositeTests } from "./support/composite-tests.js";
import type { prepare } from "./pages/kit-inventory.js";

compositeTests<typeof prepare>(
  "kit-inventory",
  { budget: 220 },
  async (run) => {
    const { step, page } = run;
    const failures: string[] = [];
    async function scenario(name: string, body: () => Promise<unknown>) {
      try {
        await run.case(name, body);
      } catch (error) {
        failures.push(String(error));
      }
    }
    async function point(name: string, x = 0.5, y = 0.5) {
      const [left, top, w, h] = await step("itemBounds", name);
      return run.pagePoint([left! + w! * x, top! + h! * y]);
    }
    async function click(name: string, x = 0.5, y = 0.5) {
      await page.mouse.click(...(await point(name, x, y)));
    }
    async function drag(name: string, dx: number, dy: number) {
      const [x, y] = await point(name);
      await page.mouse.move(x, y);
      await page.mouse.down();
      await page.mouse.move(x + dx, y + dy, { steps: 4 });
      await page.mouse.up();
    }
    async function replace(name: string, text: string) {
      await click(name + "/input");
      await step("native", name + "/input");
      await page.keyboard.press("ControlOrMeta+a");
      if (text) await page.keyboard.insertText(text);
      else await page.keyboard.press("Backspace");
    }
    await scenario(
      "Physical input changes selection, pagination and a passive measured value",
      async () => {
        await step(
          "field",
          "motion-preference",
          "CanvasText",
          "text",
          "Reduced",
        );
        await click("pin");
        await step("event", "pin/true");
        await page.keyboard.press("Space");
        await step("event", "pin/false");
        await click("tools/fast");
        await step("event", "tools/fast");
        await click("pages/next");
        await step("event", "page/2");
        await click("score/rating", 0.46);
        await step("event", "score/2.5");
        await page.keyboard.press("ArrowRight");
        await step("event", "score/3");
        await page.mouse.move(...(await point("score/rating", 0.99)));
        await step("field", "score/mark/4", "CanvasText", "text", "★");
        await page.mouse.move(...(await point("pin")));
        await step("field", "score/mark/4", "CanvasText", "text", "☆");
        const before = await run.capture("meter-quarter");
        const small = await step("itemBounds", "load/fill");
        await click("sample");
        await step("exists", "load/remainder", false);
        const full = await step("itemBounds", "load/fill");
        check(
          full[2]! > 3.8 * small[2]!,
          "The fill tracks the clamped measurement",
        );
        const [meter] = await step("meter");
        check(
          meter?.state.value === 100 && meter.actions.length === 0,
          "Agent inspection exposes a passive 100% measurement",
        );
        const after = await run.capture("meter-full");
        const [l, t, w, h] = full;
        let changed = 0;
        for (let y = Math.floor(t!); y < t! + h!; y++)
          for (let x = Math.floor(l!); x < l! + w!; x++) {
            const i = 4 * (x + y * before.width);
            if (
              Math.abs(before.pixels[i]! - after.pixels[i]!) +
                Math.abs(before.pixels[i + 1]! - after.pixels[i + 1]!) >
              25
            )
              changed++;
          }
        check(changed > 50, "Completed pixels change inside the measured bar");
        await step("externalMotion", false);
        await step("field", "motion-preference", "CanvasText", "text", "Full");
        await step("skin");
        await step("field", "pin", "GuiButton", "selected", false);
        await run.capture("glass-selection");
        await page.emulateMedia({ reducedMotion: "no-preference" });
        await step("followPlatform");
        await step("field", "motion-preference", "CanvasText", "text", "Full");
        await page.emulateMedia({ reducedMotion: "reduce" });
        await step(
          "field",
          "motion-preference",
          "CanvasText",
          "text",
          "Reduced",
        );
        return { changed };
      },
    );
    await scenario("A leap-day range crosses the month boundary", async () => {
      await step("draw", "calendar");
      await click("month/2024-02-29");
      await step("event", "2024-02-29/pending");
      await click("month/next");
      await click("month/2024-03-02");
      await step("event", "2024-02-29/2024-03-02");
      await step("field", "month/2024-03-01", "GuiButton", "selected", true);
      return {};
    });
    await scenario(
      "Date and time segments keep invalid drafts local",
      async () => {
        await step("draw", "editing");
        await replace("date/day", "31");
        await click("date/apply");
        await step("exists", "date/error", true);
        await click("external-date");
        await step("field", "date/day/input", "GuiTextInput", "text", "01");
        await click("external-date");
        await step("field", "date/day/input", "GuiTextInput", "text", "28");
        await step("exists", "date/error", false);
        await replace("date/day", "29");
        await click("date/apply");
        await step("event", "date/2024-02-29");
        await replace("time/dayPeriod", "PM");
        await click("time/apply");
        await step("event", "time/12:15");
        for (const part of ["hour", "minute", "dayPeriod"])
          await replace("time/" + part, "");
        await click("time/apply");
        await step("event", "time/null");
        await step("exists", "time/error", false);
        await click("time/clear");
        await step("event", "time/null");
        await click("seven/minute/increase");
        await step("event", "seven/00:01:03");
        return {};
      },
    );
    await scenario(
      "Picker selection updates its segmented field and closes the popup",
      async () => {
        await step("draw", "picker");
        await click("pick/choose");
        await step(
          "field",
          "pick/choose/popover",
          "GuiBehavior",
          "visible",
          true,
        );
        await click("pick/calendar/2024-02-29");
        await step("event", "pick/2024-02-29");
        await step(
          "field",
          "pick/date/day/input",
          "GuiTextInput",
          "text",
          "29",
        );
        await step(
          "field",
          "pick/choose/popover",
          "GuiBehavior",
          "visible",
          false,
        );
        return {};
      },
    );
    await scenario(
      "Pane handles, retained regions and slides react to physical controls",
      async () => {
        await step("fullMotion");
        await step("draw", "layout");
        const old = await step("itemBounds", "split/pane/left");
        await click("split/handle/left");
        await step("native", "split/handle/left");
        await page.keyboard.press("ArrowRight");
        await step("event", "resize/" + (old[2]! + 10));
        const id = await step("identity", "retained");
        await click("more/header");
        await step("event", "more/false");
        await step("field", "more/region", "CanvasBounds", "height", 0);
        await click("more/header");
        await step("event", "more/true");
        await step("itemBounds", "more/region");
        check(
          id === (await step("identity", "retained")),
          "Retained editor keeps its entity",
        );
        await click("sections/a/header");
        await step("event", "expanded/a");
        await click("sections/b/header");
        await step("event", "expanded/b");
        await drag("slides/grip", -70, 0);
        await step("event", "slide/two");
        await step("field", "slides/strip", "CanvasStyle", "x", -240);
        await click("detail");
        await step("event", "inspect");
        await click("remove-last");
        await step("field", "slides/strip", "CanvasStyle", "x", 0);
        await step("exists", "slides/slide/two", false);
        return {};
      },
    );
    await scenario(
      "Trace rows retain the time scale and timeline actions remain reachable",
      async () => {
        await step("draw", "feed");
        await step("exists", "history/open/connector", true);
        await step("exists", "history/done/connector", false);
        await click("result");
        await step("event", "result");
        await click("trace/request/toggle");
        await click("trace/db/select");
        await step("event", "span/db");
        const root = await step("itemBounds", "trace/request/bar/1"),
          child = await step("itemBounds", "trace/db/bar/1");
        check(
          Math.abs(root[2]! / 3 - child[2]!) < 1,
          "Durations share one time scale",
        );
        return {};
      },
    );
    await scenario(
      "Avatar and crop preview track the bitmap lifetime and original pixel coordinates",
      async () => {
        await step("draw", "assets");
        await step("exists", "person/fallback", true);
        const avatar = await step("identity", "person");
        await click("photo-toggle");
        await step("exists", "person/image", true);
        await step("itemBounds", "crop/image");
        const photo = await run.capture("bitmap-and-crop");
        const orange = photo.pixels.filter(
          (r, i) =>
            i % 4 === 0 &&
            r > 220 &&
            photo.pixels[i + 1]! > 65 &&
            photo.pixels[i + 1]! < 155 &&
            photo.pixels[i + 2]! < 70,
        ).length;
        check(
          orange > 150,
          "The ready bitmap paints the avatar and crop preview",
        );
        await click("crop/preview");
        await step("native", "crop/preview");
        await page.keyboard.press("ArrowRight");
        await step("event", "crop/5/8");
        await click("crop/reset");
        await step("event", "crop/4/8");
        await click("crop/preview");
        await step("native", "crop/preview");
        await page.keyboard.press("Shift+ArrowDown");
        await step("event", "crop/4/9");
        await click("crop/reset");
        await drag("crop/preview", 11, 0);
        await step("event", "crop/6/8");
        await click("photo-toggle");
        await step("exists", "crop/image", false);
        check(
          avatar === (await step("identity", "person")),
          "Avatar identity survives source changes",
        );
        return { orange };
      },
    );
    await scenario("A canvas drawer opens, snaps and dismisses", async () => {
      await step("skin");
      await step("draw", "drawer");
      await click("show-drawer");
      await step("field", "drawer", "GuiBehavior", "visible", true);
      await click("drawer/grip");
      await page.keyboard.press("ArrowUp");
      await step("event", "snap/1");
      await step("field", "drawer", "GuiLayout", "height", 160);
      await drag("drawer/grip", 0, 72);
      await step("event", "snap/0");
      await step("field", "drawer", "GuiLayout", "height", 80);
      await click("drawer/close");
      await step("field", "drawer", "GuiBehavior", "visible", false);
      return {};
    });
    if (failures.length) throw new Error(failures.join("\n"));
  },
);
