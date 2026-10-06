/** Completed gallery data changes, pointer reconciliation and deliberate presentation options. */
import {
  assertChartImageChanged,
  assertBoundedChartInput,
  baselineBarPointer,
  baselineGridPointer,
  baselineSingleRowPointer,
  chartCheck,
  focusChart,
  waitForCharts,
  type GalleryChartsDriver,
  type GalleryChartsState,
} from "./gallery-charts.js";

function sample(state: GalleryChartsState, id: string, row = 1n) {
  const chart = state.charts.find((chart) => chart.id === id)!;
  const value = chart.binding.rows.find(
    (item) => String(item.id) === String(row),
  )?.values[chart.binding.columns.findIndex((column) => column.name === "y")];
  chartCheck(
    value?.valid && value.value.kind === "f32",
    `${id}: displayed sample exists`,
  );
  return value.value.value;
}

export function assertChartFeedback(state: GalleryChartsState, settled = true) {
  for (const chart of state.charts) {
    const marks = [state.selection, state.hover].filter(
      (mark) => mark?.chart === chart.id,
    );
    const expected = marks.filter(
      (mark, index) =>
        marks.findIndex(
          (other) =>
            String(other!.rowId) === String(mark!.rowId) &&
            other!.series === mark!.series,
        ) === index,
    );
    chartCheck(
      chart.labels.length === expected.length,
      `${chart.id}: actual label count matches current feedback`,
    );
    for (const mark of expected) {
      chartCheck(mark, "Feedback mark exists");
      chartCheck(
        mark.source === chart.source &&
          String(mark.sourceIncarnation) ===
            String(chart.binding.sourceIncarnation) &&
          String(mark.bindingIncarnation) ===
            String(chart.binding.bindingIncarnation),
        `${chart.id}: feedback belongs to the current source and binding lifetime`,
      );
      chartCheck(
        chart.labels.some(
          (label) =>
            label.series === mark.series &&
            String(label.row_id) === String(mark.rowId) &&
            label.highlighted &&
            label.text ===
              `${mark === state.selection ? "SELECTED" : "HOVER"} / ROW ${mark.rowId}`,
        ),
        `${chart.id}: current row has a real highlighted label`,
      );
    }
  }
  assertBoundedChartInput(state, settled);
}

function assertAdaptiveScope(state: GalleryChartsState) {
  for (const chart of state.charts.filter((chart) =>
    chart.component.endsWith("3d"),
  )) {
    const fields = chart.inspection.entities
      .find((entity) => String(entity.id) === String(chart.entity))!
      .components.find(
        (component) => "adaptive_axes" in component.fields,
      )!.fields;
    const expected =
      state.adaptiveAxes &&
      [state.hover, state.selection].some((mark) => mark?.chart === chart.id);
    chartCheck(
      fields.adaptive_axes === expected,
      `${chart.id}: adaptive axes follow only enabled mark interaction`,
    );
  }
}

export async function exerciseChartRegeneration(
  driver: GalleryChartsDriver,
  aspect: number,
) {
  await driver.action("smoothChanges", false);
  await driver.action("streamPlayback", false);
  const focused = await focusChart(driver, "bars");
  const point = baselineBarPointer(focused, aspect, 1, 8.75);
  await driver.action("hover", point);
  await driver.action("select", point);
  const original = await driver.inspect();
  chartCheck(
    String(original.hover?.rowId) === "1",
    "Original pointer picks row one",
  );
  assertChartFeedback(original);
  for (const mode of ["streaming", "buffer"]) {
    const previous = await driver.inspect();
    await driver.action("dataSource", mode);
    const current = await driver.inspect();
    chartCheck(
      current.hover?.chart === "bars" && String(current.hover.rowId) === "1",
      "A stationary pointer repicks replacement data without another input event",
    );
    chartCheck(
      current.selection === null,
      "Replacement sources invalidate selection",
    );
    chartCheck(
      String(current.hover.sourceIncarnation) !==
        String(previous.hover?.sourceIncarnation),
      "Reused row numbers never reuse source identity",
    );
    assertChartFeedback(current);
    const highlighted = await driver.capture(
      `chart-regenerated-${mode}-highlight`,
    );
    await driver.action("hover", null);
    const cleared = await driver.capture(`chart-regenerated-${mode}-clear`);
    assertChartImageChanged(
      highlighted,
      cleared,
      "Actual replacement-row highlight",
    );
    await driver.action("hover", point);
    await driver.record(`chart-regenerated-${mode}`, current);
  }
  await driver.action("select", point);
  // Row one's old height is 35; the edited height is 17.5. This point leaves it.
  await driver.action(
    "hover",
    baselineBarPointer(await driver.inspect(), aspect, 1, 26.25),
  );
  await driver.action("changeSamples", true);
  const shrunk = await driver.inspect();
  chartCheck(
    shrunk.hover === null && String(shrunk.selection?.rowId) === "1",
    "A stationary pointer leaves a shrunk bar while selection keeps its row",
  );
  assertChartFeedback(shrunk);
  await driver.action("changeSamples", false);
  const restored = await driver.inspect();
  chartCheck(
    String(restored.hover?.rowId) === "1",
    "Restored geometry returns beneath the stationary pointer",
  );
  assertChartFeedback(restored);
  await driver.record("chart-stationary-pointer-edits", { shrunk, restored });
}

export async function exerciseChartAdaptiveScope(
  driver: GalleryChartsDriver,
  aspect: number,
) {
  const grid = await focusChart(driver, "grid-bars");
  chartCheck(!grid.adaptiveAxes, "Adaptive axes start disabled");
  assertAdaptiveScope(grid);
  await driver.action("hover", baselineGridPointer(grid, aspect));
  await driver.action("adaptiveAxes", true);
  assertAdaptiveScope(await driver.inspect());
  await driver.action(
    "select",
    baselineGridPointer(await driver.inspect(), aspect),
  );
  const single = await focusChart(driver, "single-row");
  await driver.action("hover", baselineSingleRowPointer(single, aspect));
  const simultaneous = await driver.inspect();
  chartCheck(
    simultaneous.hover?.chart === "single-row" &&
      simultaneous.selection?.chart === "grid-bars",
    "Hover and selection can authorize distinct charts",
  );
  assertAdaptiveScope(simultaneous);
  await driver.action("clearSelection");
  assertAdaptiveScope(await driver.inspect());
  await driver.action("hover", null);
  const inactive = await driver.inspect();
  assertAdaptiveScope(inactive);
  await driver.action("adaptiveAxes", false);
  assertAdaptiveScope(await driver.inspect());
  await driver.record("chart-adaptive-axis-interaction-scope", {
    simultaneous,
    inactive,
  });
  await driver.capture("chart-adaptive-axis-inactive");
}

export async function exerciseChartAutomaticRange(
  driver: GalleryChartsDriver,
  aspect: number,
) {
  await driver.action("smoothChanges", false);
  const baseline = await focusChart(driver, "bars");
  chartCheck(
    !baseline.automaticRange && !baseline.expandedSamples,
    "Range and growth options start fixed",
  );
  await driver.action("expandedSamples", true);
  const expanded = await driver.inspect();
  chartCheck(
    sample(expanded, "bars") === 150 &&
      sample(expanded, "height-surface") === 6,
    "Growth samples exceed the original numeric ranges",
  );
  const high = baselineBarPointer(expanded, aspect, 2, 60);
  await driver.action("hover", high);
  chartCheck(
    String((await driver.inspect()).hover?.rowId) === "2",
    "Fixed axes preserve the original numeric mapping",
  );
  await driver.action("hover", null);
  const fixed = await driver.capture("chart-expanded-fixed-range");
  await driver.action("automaticRange", true);
  const fitted = await driver.inspect();
  const fitFrame = await driver.capture("chart-expanded-automatic-range");
  assertChartImageChanged(fixed, fitFrame, "Automatic numeric-range fitting");
  await driver.action("hover", high);
  chartCheck(
    (await driver.inspect()).hover === null,
    "Automatic 0–150 range lowers the unchanged 65-value bar below the old pointer",
  );
  await driver.action("hover", baselineBarPointer(fitted, aspect, 2, 30));
  chartCheck(
    String((await driver.inspect()).hover?.rowId) === "2",
    "Picking uses the same fitted numeric range as rendering",
  );
  for (const chart of fitted.charts) {
    const original = baseline.charts.find((item) => item.id === chart.id)!;
    for (const dimension of ["width", "height", "depth", "min_z", "max_z"])
      chartCheck(
        chart.frame[dimension] === original.frame[dimension],
        `${chart.id}: range growth preserves ${dimension}`,
      );
  }
  const height = fitted.charts.find((chart) => chart.id === "height-surface")!;
  chartCheck(
    height.legend.scale?.min === 0 && height.legend.scale.max === 4,
    "Expanded height retains the clamped 0–4 color scale",
  );
  const color =
    height.binding.rows[0]!.values[
      height.binding.columns.findIndex((column) => column.name === "color")
    ];
  chartCheck(
    color?.valid &&
      color.value.kind === "vec4" &&
      color.value.value.every(
        (value, index) =>
          Math.abs(value - height.legend.scale!.colors.at(-1)![index]!) < 1e-5,
      ),
    "Out-of-range height uses the legend's top endpoint color",
  );
  await driver.record("chart-automatic-range-growth", {
    baseline,
    expanded,
    fitted,
  });
}

export async function exerciseChartSmoothChanges(
  driver: GalleryChartsDriver,
  aspect: number,
) {
  const baseline = await focusChart(driver, "bars");
  chartCheck(
    baseline.smoothChanges,
    "Existing-sample smoothing starts enabled",
  );
  chartCheck(driver.setSampleInterpolation, "Fixture can author binding rates");
  // Populate the expanded row immediately, then descend through the visible range.
  await driver.action("smoothChanges", false);
  await driver.action("expandedSamples", true);
  await driver.action("smoothChanges", true);
  const expanded = await driver.inspect();
  await driver.setSampleInterpolation(
    expanded.charts.find((chart) => chart.id === "bars")!,
    5,
  );
  const pointer = baselineBarPointer(baseline, aspect, 1, 75);
  await driver.action("hover", pointer);
  await driver.action("select", pointer);
  chartCheck(
    String((await driver.inspect()).hover?.rowId) === "1",
    "Descent pointer begins inside the expanded bar",
  );
  const initial = await driver.capture("chart-smooth-expanded");
  await driver.action("expandedSamples", false);
  const moving = await driver.inspect();
  const picked =
    moving.hover?.values[
      moving.hover.columns.findIndex((column) => column === "y")
    ];
  chartCheck(
    sample(moving, "bars") > 75 &&
      sample(moving, "bars") < 150 &&
      picked?.valid &&
      picked.value.kind === "f32" &&
      picked.value.value > 75 &&
      picked.value.value < 150,
    "Picking reports the displayed intermediate value rather than the 35-value target",
  );
  assertChartFeedback(moving, false);
  const middle = await waitForCharts(
    driver,
    (state) =>
      sample(state, "bars") > 55 &&
      sample(state, "bars") < 70 &&
      state.hover === null,
    "chart-smooth-visible-interior",
    30_000,
  );
  const value = sample(middle, "bars");
  const selected =
    middle.selection?.values[
      middle.selection.columns.findIndex((column) => column === "y")
    ];
  chartCheck(
    value > 35 &&
      value < 100 &&
      selected?.valid &&
      selected.value.kind === "f32" &&
      selected.value.value > 35 &&
      selected.value.value < 100,
    "Visible intermediate geometry leaves the stationary pointer and refreshes selection",
  );
  assertChartFeedback(middle, false);
  const shown = await driver.capture("chart-smooth-intermediate");
  assertChartImageChanged(
    initial,
    shown,
    "Intermediate rendered sample motion",
  );
  const settled = await waitForCharts(
    driver,
    (state) => {
      const selected =
        state.selection?.values[state.selection.columns.indexOf("y")];
      return (
        sample(state, "bars") === 35 &&
        sample(state, "height-surface") ===
          sample(baseline, "height-surface") &&
        state.hover === null &&
        selected?.valid === true &&
        selected.value.kind === "f32" &&
        selected.value.value === 35 &&
        Object.values(state.input).every(
          (lane) => lane.inFlight === 0 && lane.pending === 0,
        )
      );
    },
    "chart-smooth-existing-row-settled",
  );
  assertChartFeedback(settled);
  const final = await driver.capture("chart-smooth-settled");
  assertChartImageChanged(shown, final, "Settled rendered sample motion");
  // A fast real transition can finish before a feedback interval is needed.
  await Promise.all(
    settled.charts.map((chart) =>
      driver.setSampleInterpolation!(chart, 1_000_000),
    ),
  );
  await driver.action("changeSamples", true);
  const fast = await driver.inspect();
  const fastSelection =
    fast.selection?.values[fast.selection.columns.indexOf("y")];
  chartCheck(
    sample(fast, "bars") === 17.5 &&
      fastSelection?.valid === true &&
      fastSelection.value.kind === "f32" &&
      fastSelection.value.value === 17.5,
    "A transition completed before feedback polling still publishes final selected values",
  );
  assertChartFeedback(fast);
  await driver.record("chart-smooth-displayed-values-and-pointer", {
    baseline,
    expanded,
    moving,
    middle,
    settled,
    fast,
  });
}
