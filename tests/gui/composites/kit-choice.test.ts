import { check } from "../../harness/page/checks.js";
import { compositeTests } from "./support/composite-tests.js";
import type { prepare } from "./pages/kit-choice.js";

compositeTests<typeof prepare>("kit-choice", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const kitAt = async (
    symbol: string,
    fraction: readonly [number, number] = [0.5, 0.5],
  ) => run.pagePoint(await step("kitPoint", symbol, fraction));
  const marks = ["x", "y", "z"].map((value) => `kit-radio/${value}/mark`);
  const tabs = ["one", "two", "three"].map((value) => `kit-tabs/tab/${value}`);
  const rows = ["a", "a1", "a2", "b"].map((key) => `kit-tree/row/${key}`);

  // Radio group: a press focuses X; Right moves focus and selection to Y in
  // one frame and the application hears of it; Right then stops, the
  // disabled Z being skipped. A press on X's label selects X through its
  // mark, a client round trip later, and leaves focus on Y.
  await run.case("radio group", async () => {
    await page.mouse.click(...(await kitAt(marks[0]!)));
    await step("expectKitFocus", marks[0]!, false);
    const cut = await step("cut");
    await page.keyboard.press("ArrowRight");
    const radioKey = await step("keyOutcome", "right", cut);
    check(
      radioKey.disposition === "routed",
      `ArrowRight in the radio group was ${radioKey.disposition}`,
    );
    await step("expectKitFocus", marks[1]!, true);
    const radioSelected = await step("expectKitSelected", marks, [marks[1]!]);
    await step("expectKitReports", "radio", ["y"]);
    await page.keyboard.press("ArrowRight");
    await step("expectKitFocus", marks[1]!, true);
    await page.mouse.click(...(await kitAt("kit-radio/x/label")));
    const labelSelected = await step("expectKitSelected", marks, [marks[0]!]);
    const radioReports = await step("expectKitReports", "radio", ["y", "x"]);
    await step("expectKitFocus", marks[1]!, true);
    return { radioKey, radioSelected, labelSelected, radioReports };
  });

  // Tab strip: one Tab stop entered at its selected tab, also when Shift+Tab
  // returns to the radio group's selected X. Arrows move focus without
  // selecting; Enter activates the focused tab, which selects it and
  // switches the content.
  await run.case("tab strip", async () => {
    await page.keyboard.press("Tab");
    await step("expectKitFocus", tabs[0]!, true);
    await page.keyboard.press("Shift+Tab");
    const radioEntered = await step("expectKitFocus", marks[0]!, true);
    await page.keyboard.press("Tab");
    await step("expectKitFocus", tabs[0]!, true);
    await page.keyboard.press("ArrowRight");
    await step("expectKitFocus", tabs[1]!, true);
    await step("expectKitSelected", tabs, [tabs[0]!]);
    await step("expectKitDeclared", ["kit-content-one"]);
    await page.keyboard.press("Enter");
    const tabSelected = await step("expectKitSelected", tabs, [tabs[1]!]);
    const tabReports = await step("expectKitReports", "tabs", ["two"]);
    await step("expectKitDeclared", ["kit-content-two"], ["kit-content-one"]);
    return { radioEntered, tabSelected, tabReports };
  });

  // Tree: Tab leaves the strip for the tree's first row. Right returns
  // unhandled and expands A; again, it moves focus to A's first row through
  // a client focus action, which the keyboard follows: Down continues from
  // A1. Left moves to the parent, then collapses it. Collapsing A by its
  // chevron while A1 holds focus moves focus to A and selects nothing; Enter
  // selects the focused row.
  await run.case("tree view", async () => {
    await page.keyboard.press("Tab");
    await step("expectKitFocus", rows[0]!, true);
    const cut = await step("cut");
    await page.keyboard.press("ArrowRight");
    const treeKey = await step("keyOutcome", "right", cut);
    check(
      treeKey.disposition === "unhandled",
      `ArrowRight on a tree row was ${treeKey.disposition}`,
    );
    await step("expectKitDeclared", [rows[1]!, rows[2]!]);
    await step("expectKitReports", "expanded", [["a"]]);
    await step("expectKitFocus", rows[0]!, true);
    await page.keyboard.press("ArrowRight");
    const childFocused = await step("expectKitFocus", rows[1]!, true);
    await page.keyboard.press("ArrowDown");
    const keyboardFollowed = await step("expectKitFocus", rows[2]!, true);
    await page.keyboard.press("ArrowLeft");
    const parentFocused = await step("expectKitFocus", rows[0]!, true);
    await page.keyboard.press("ArrowLeft");
    await step("expectKitDeclared", [rows[0]!], [rows[1]!]);
    await step("expectKitReports", "expanded", [["a"], []]);
    await page.keyboard.press("ArrowRight");
    await step("expectKitDeclared", [rows[1]!]);
    await page.keyboard.press("ArrowRight");
    await step("expectKitFocus", rows[1]!, true);
    await page.mouse.click(...(await kitAt("kit-tree/row/a/chevron")));
    await step("expectKitDeclared", [rows[0]!], [rows[1]!]);
    const collapsedFocus = await step("expectKitFocus", rows[0]!);
    await step("expectKitSelected", [rows[0]!, rows[3]!], []);
    await page.keyboard.press("Enter");
    const rowSelected = await step(
      "expectKitSelected",
      [rows[0]!, rows[3]!],
      [rows[0]!],
    );
    const treeReports = await step("expectKitReports", "tree", ["a"]);
    await run.capture("kit-choice");
    return {
      treeKey,
      childFocused,
      keyboardFollowed,
      parentFocused,
      collapsedFocus,
      rowSelected,
      treeReports,
    };
  });
});
