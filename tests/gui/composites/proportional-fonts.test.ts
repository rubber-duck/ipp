import { check } from "../../harness/page/checks.js";
import { accentPixels, compositeTests } from "./support/composite-tests.js";
import type { prepare } from "./pages/proportional-fonts.js";

compositeTests<typeof prepare>(
  "proportional-fonts",
  {
    budget: 90,
    font: "target/font-assets/noto-sans.ippf",
    iconFont: "target/font-assets/shure-tech-mono.ippf",
  },
  async (run) => {
    const { step } = run;
    // Independent values from the pinned Noto Sans regular source (1000 UPEM):
    // i=258, W=930, A=639, V=600, default Latin AV pair adjustment=-40.
    await run.case(
      "runtime advances, kerning and fitted controls",
      async () => {
        await step("expectWidth", "probe-i", ((4 * 258) / 1000) * 18);
        await step("expectWidth", "probe-w", ((4 * 930) / 1000) * 18);
        await step("expectWidth", "probe-av", ((639 + 600 - 40) / 1000) * 18);
        // Compact skin: 24-unit minimum content width plus two 12-unit pads, scaled 18/16.
        await step("expectWidth", "narrow", ((24 + 24) * 18) / 16);
        await step(
          "expectWidth",
          "wide",
          ((4 * 930) / 1000) * 18 + (24 * 18) / 16,
        );
        const narrow = await step("read", "narrow");
        const wide = await step("read", "wide");
        check(
          wide.bounds[2]! > narrow.bounds[2]! + 30,
          "Buttons must fit unequal glyph advances",
        );
        const image = await run.capture("noto-cybertron");
        check(
          accentPixels(image, [40, 32, 440, 40]) > 300,
          "Heading glyphs missing",
        );
        check(
          accentPixels(image, [40, 32, 16, 24]) > 10,
          "Separate icon font missing",
        );
        check(accentPixels(image, narrow.bounds) > 20, "Narrow label missing");
        check(accentPixels(image, wide.bounds) > 100, "Wide label missing");
        return { narrow, wide };
      },
    );
    await run.case(
      "font replacement retains controls and editable text",
      async () => {
        const before = await step("read", "workspace-name/input");
        const [x, y, width, height] = before.bounds;
        await run.page.mouse.click(
          ...run.pagePoint([x! + width! / 2, y! + height! / 2]),
        );
        await step("expectTyping", "font-input");
        await run.page.keyboard.press("ControlOrMeta+A");
        await run.page.keyboard.insertText("Willow AV iii");
        await step("expectNativeText", "Willow AV iii", "font-input");
        await step("render", "cybertron", true);
        await step("expectWidth", "probe-i", 4 * 0.54 * 18);
        await step("expectWidth", "probe-w", 4 * 0.54 * 18);
        await step("render", "glass");
        await step("expectWidth", "probe-i", ((4 * 258) / 1000) * 18);
        await step("expectWidth", "probe-w", ((4 * 930) / 1000) * 18);
        const after = await step("read", "workspace-name/input");
        check(
          before.entity === after.entity,
          "Font changes replaced the input entity",
        );
        await run.page.keyboard.press("End");
        await run.page.keyboard.insertText("!");
        await step("expectNativeText", "Willow AV iii!", "font-input");
        const image = await run.capture("noto-glass");
        let textPixels = 0;
        for (let row = 32; row < 70; row++)
          for (let col = 60; col < 440; col++) {
            const p = (row * image.width + col) * 4;
            if (
              image.pixels[p]! > 100 &&
              image.pixels[p + 1]! > 140 &&
              image.pixels[p + 2]! > 140
            )
              textPixels++;
          }
        check(textPixels > 300, "Reskinned proportional text missing");
        return { before, after, textPixels };
      },
    );
  },
);
