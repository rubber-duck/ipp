import { check } from "../../harness/page/checks.js";
import {
  accentPixels,
  compositeTests,
  json,
} from "./support/composite-tests.js";
import type { prepare } from "./pages/kit-select.js";

compositeTests<typeof prepare>("kit-select", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const selectAt = async (
    symbol: string,
    fraction: readonly [number, number] = [0.5, 0.5],
  ) => run.pagePoint(await step("selectPoint", symbol, fraction));

  await run.case("kit dropdown", async () => {
    // A press opens the list and focuses the trigger; Escape closes it
    // without a change and keeps focus there.
    const list = ["sel-dropdown/list"];
    await page.mouse.click(...(await selectAt("sel-dropdown")));
    await step("expectSelect", { open: list, focus: "sel-dropdown" });
    await page.keyboard.press("Escape");
    const dropdownEscape = await step("expectSelect", {
      open: [],
      focus: "sel-dropdown",
      dropdown: [],
      dropdownText: "Aurora",
    });
    // Enter on the trigger opens it; Down starts at the selected Aurora and
    // moves past the disabled Ember to Neon; Enter picks Neon once, the list
    // closes and focus stays on the trigger.
    await page.keyboard.press("Enter");
    await step("expectSelect", { open: list });
    await page.keyboard.press("ArrowDown");
    await step("expectSelect", { active: "sel-dropdown/options/aurora" });
    await page.keyboard.press("ArrowDown");
    const dropdownActive = await step("expectSelect", {
      active: "sel-dropdown/options/neon",
      focus: "sel-dropdown",
    });
    await page.keyboard.press("Enter");
    const dropdownPicked = await step("expectSelect", {
      open: [],
      focus: "sel-dropdown",
      dropdown: ["neon"],
      dropdownText: "Neon",
    });
    // An outside press closes the list and is swallowed: the button beside
    // it hears nothing until a press with the list closed.
    await page.mouse.click(...(await selectAt("sel-dropdown")));
    await step("expectSelect", { open: list });
    const besideAt = await selectAt("sel-beneath");
    await page.mouse.click(...besideAt);
    const dropdownOutside = await step("expectSelect", {
      open: [],
      beneath: 0,
    });
    await page.mouse.click(...besideAt);
    await step("expectSelect", { beneath: 1, dropdown: ["neon"] });
    return { dropdownEscape, dropdownActive, dropdownPicked, dropdownOutside };
  });

  // Searchable dropdown: opening moves focus into its search field; typing,
  // once the field's native buffer takes the keys, filters the options while
  // the field keeps focus; Enter picks the first match and focus returns to
  // the trigger, the search cleared.
  await run.case("kit searchable dropdown", async () => {
    await page.mouse.click(...(await selectAt("sel-search")));
    await step("expectSelect", {
      open: ["sel-search/list"],
      focus: "sel-search/search/field",
    });
    await step("expectSelectField", "sel-search/search/field");
    await page.keyboard.insertText("pu");
    const searchFiltered = await step("expectSelect", {
      searchText: "pu",
      searchRows: ["sel-search/options/pulse"],
      focus: "sel-search/search/field",
    });
    await page.keyboard.press("Enter");
    const searchPicked = await step("expectSelect", {
      open: [],
      focus: "sel-search",
      searchable: ["pulse"],
      searchText: "",
    });
    return { searchFiltered, searchPicked };
  });

  // Multi-select: two picks toggle two options and the list stays open; their
  // check marks are painted.
  await run.case("kit multi-select", async () => {
    await page.mouse.click(...(await selectAt("sel-multi")));
    await step("expectSelect", { open: ["sel-multi/list"] });
    await page.mouse.click(...(await selectAt("sel-multi/options/render")));
    await step("expectSelect", { multi: [["render"]] });
    await page.mouse.click(...(await selectAt("sel-multi/options/physics")));
    const multiToggled = await step("expectSelect", {
      open: ["sel-multi/list"],
      multi: [["render"], ["render", "physics"]],
      multiText: "Render, Physics",
      focus: "sel-multi",
    });
    const image = await run.capture("kit-multi-select");
    // A row's check mark is its Button's icon: the square of 0.55 its height
    // centred in the row's leading square, clear of the active row's lit
    // edge next to it.
    const leading = async (row: string) => {
      const [x, y, , height] = await step(
        "selectBounds",
        `sel-multi/options/${row}`,
      );
      const inset = height * 0.225;
      return accentPixels(image, [
        x + inset,
        y + inset,
        height * 0.55,
        height * 0.55,
      ]);
    };
    const checks = {
      render: await leading("render"),
      network: await leading("network"),
    };
    check(
      checks.render > 0 && checks.network === 0,
      `Check marks: ${json(checks)}`,
    );
    await page.keyboard.press("Escape");
    await step("expectSelect", { open: [] });
    return { multiToggled, checks };
  });

  // Autocomplete: typing opens the application's suggestions; Escape closes
  // them and keeps the text; typing reopens them, Down makes the first
  // active while focus and caret stay in the field, and Enter accepts it.
  // Text without a suggestion commits on Enter.
  await run.case("kit autocomplete", async () => {
    await page.mouse.click(...(await selectAt("sel-auto")));
    await step("expectSelect", { focus: "sel-auto" });
    await step("expectSelectField", "sel-auto");
    await page.keyboard.insertText("al");
    await step("expectSelect", { open: ["sel-auto/list"], input: ["al"] });
    await page.keyboard.press("Escape");
    const autoEscape = await step("expectSelect", {
      open: [],
      autoText: "al",
      focus: "sel-auto",
    });
    await page.keyboard.insertText("p");
    await step("expectSelect", {
      open: ["sel-auto/list"],
      input: ["al", "alp"],
    });
    await page.keyboard.press("ArrowDown");
    await step("expectSelect", {
      active: "sel-auto/suggestions/alpha",
      focus: "sel-auto",
    });
    await page.keyboard.press("Enter");
    const autoAccepted = await step("expectSelect", {
      open: [],
      select: ["alpha"],
      autoText: "Alpha Station",
      focus: "sel-auto",
    });
    await page.keyboard.press("End");
    await page.keyboard.insertText("!");
    await step("expectSelect", { autoText: "Alpha Station!", open: [] });
    await page.keyboard.press("Enter");
    const autoCommitted = await step("expectSelect", {
      commit: ["Alpha Station!"],
      select: ["alpha"],
      input: ["al", "alp", "Alpha Station", "Alpha Station!"],
    });
    await run.capture("kit-select");
    return { autoEscape, autoAccepted, autoCommitted };
  });
});
