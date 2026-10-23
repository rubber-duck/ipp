import {
  assertChartImageChanged,
  chartCheck,
  focusChart,
  chartTransform,
  baselineBarPointer,
  placePoint,
  projectChartPoint,
  waitForCharts,
  type GalleryChartsDriver,
} from "./gallery-charts.js";

/** The real app's providers cover attached Canvas Worlds and root spatial charts. */
export async function exerciseChartSkins(driver: GalleryChartsDriver) {
  await driver.action("skin", "cybertron");
  await focusChart(driver, "grid-bars");
  const before = await driver.inspect();
  const cybertron = await driver.capture("skin-cybertron-grid");
  await driver.action("font", "noto-sans");
  const proportional = await driver.inspect();
  for (const chart of proportional.charts) {
    const previous = before.charts.find((item) => item.id === chart.id)!;
    const source = (item: typeof chart) =>
      item.inspection.entities
        .find((entity) => entity.id === item.entity)!
        .components.find((component) => "grid_alpha" in component.fields)!
        .fields.source;
    chartCheck(
      chart.entity === previous.entity && chart.world.id === previous.world.id,
      `${chart.id}: font switching preserves chart identity`,
    );
    chartCheck(
      source(chart) !== source(previous),
      `${chart.id}: plot labels receive the selected font`,
    );
    chartCheck(
      chart.binding.evaluatedTick === previous.binding.evaluatedTick,
      `${chart.id}: font switching preserves evaluated data`,
    );
    for (const text of chart.legend.inspection.entities) {
      const font = text.components.find(
        (component) =>
          "text" in component.fields && "source" in component.fields,
      )?.fields.source;
      if (!font) continue;
      const original = previous.legend.inspection.entities.find(
        (entity) => entity.id === text.id,
      );
      chartCheck(
        original,
        `${chart.id}: font switching preserves legend identity`,
      );
      const oldFont = original.components.find(
        (component) =>
          "text" in component.fields && "source" in component.fields,
      )?.fields.source;
      chartCheck(
        font !== oldFont,
        `${chart.id}: legend text receives the selected font`,
      );
    }
  }
  assertChartImageChanged(
    cybertron,
    await driver.capture("font-noto-grid"),
    "Proportional chart labels and legends",
  );
  await driver.action("skin", "glass");
  const glass = await driver.inspect();
  for (const chart of glass.charts) {
    const previous = before.charts.find((item) => item.id === chart.id)!;
    chartCheck(
      previous.entity === chart.entity && previous.world.id === chart.world.id,
      `${chart.id}: a skin change replaced native identity`,
    );
    chartCheck(
      previous.binding.evaluatedTick === chart.binding.evaluatedTick,
      `${chart.id}: a skin change reevaluated source data`,
    );
    const entity = chart.inspection.entities.find(
      (item) => item.id === chart.entity,
    )!;
    const material = entity.components.find(
      (component) =>
        "finish" in (component.properties ?? {}) &&
        !("body" in (component.properties ?? {})),
    );
    chartCheck(
      material,
      `${chart.id}: provider did not attach the glass material`,
    );
    chartCheck(
      entity.components.some((component) => "padding_x" in component.fields) &&
        entity.components.some(
          (component) => "body" in (component.properties ?? {}),
        ),
      `${chart.id}: native callouts did not receive shared glass paint`,
    );
    if (chart.component.endsWith("3d"))
      chartCheck(
        material.fields.alpha_mode === 2,
        `${chart.id}: spatial glass did not enable alpha blending`,
      );
  }
  assertChartImageChanged(
    cybertron,
    await driver.capture("skin-glass-grid"),
    "Glass spatial appearance",
  );
  await focusChart(driver, "variable-pie");
  const pieState = await driver.inspect();
  const pie = pieState.charts.find((chart) => chart.id === "variable-pie")!;
  const pieEntity = pie.inspection.entities.find(
    (entity) => entity.id === pie.entity,
  )!;
  chartCheck(
    pieEntity.components.some(
      (component) => component.fields.mark_outlines === false,
    ),
    "Glass pie suppresses wire geometry including highlighted edges",
  );
  const sheet = pie.legend.inspection.entities.find(
    (entity) => entity.metadata?.symbolicId === `${pie.legend.id}/sheet`,
  );
  chartCheck(
    sheet?.components.some(
      (component) => "body" in (component.properties ?? {}),
    ),
    "Spatial legend uses the shared GUI glass sheet",
  );
  const facing = pieState.world.entities.find(
    (entity) => entity.id === pie.legend.facing,
  );
  chartCheck(
    facing?.components.some(
      (component) =>
        "target" in component.fields && component.fields.enabled === true,
    ),
    "Glass legend faces the camera through the native aim constraint",
  );
  const pieFrame = await driver.capture("skin-glass-pie-and-legend");
  const angle = Math.PI / 2 + Math.PI * 0.4;
  await driver.action(
    "select",
    projectChartPoint(
      pieState,
      placePoint(chartTransform(pieState, pie), [
        5 + Math.sin(angle) * 1.8,
        2.4,
        5 + Math.cos(angle) * 1.8,
      ]),
      pieFrame.width / pieFrame.height,
    ),
  );
  await waitForCharts(
    driver,
    (state) =>
      state.selection?.chart === "variable-pie" &&
      String(state.selection.rowId) === "1",
    "Glass pie selection retains the real source row",
  );
  assertChartImageChanged(
    pieFrame,
    await driver.capture("skin-glass-pie-selected"),
    "Native Glass pie selection remains visible without a wire outline",
  );
  await driver.action("clearSelection");
  await focusChart(driver, "bars");
  const planar = await driver.capture("skin-glass-bars");
  await driver.action(
    "select",
    baselineBarPointer(
      await driver.inspect(),
      planar.width / planar.height,
      1,
      8.75,
    ),
  );
  await waitForCharts(
    driver,
    (state) =>
      state.selection?.chart === "bars" &&
      String(state.selection.rowId) === "1",
    "Planar glass callout retains source-row picking",
  );
  assertChartImageChanged(
    planar,
    await driver.capture("skin-glass-bars-selected"),
    "Planar shared glass callout",
  );
  await driver.action("clearSelection");
  await driver.action("skin", "cybertron");
  assertChartImageChanged(
    planar,
    await driver.capture("skin-cybertron-bars"),
    "Glass planar appearance",
  );
  const restored = await driver.inspect();
  for (const chart of restored.charts) {
    if (chart.legend.facing !== null) {
      const facing = restored.world.entities.find(
        (entity) => entity.id === chart.legend.facing,
      );
      chartCheck(
        facing?.components.some(
          (component) =>
            "target" in component.fields && component.fields.enabled === false,
        ),
        "Cybertron restores fixed legend placement",
      );
    }
    const entity = chart.inspection.entities.find(
      (item) => item.id === chart.entity,
    )!;
    chartCheck(
      !entity.components.some(
        (component) => "finish" in (component.properties ?? {}),
      ),
      `${chart.id}: leaving Glass retained its material`,
    );
    const frame = entity.components.find(
      (component) => "grid_alpha" in component.fields,
    )!.fields;
    const previous = before.charts.find((item) => item.id === chart.id)!;
    const original = previous.inspection.entities
      .find((item) => item.id === previous.entity)!
      .components.find((component) => "grid_alpha" in component.fields)!.fields;
    for (const field of [
      "red",
      "green",
      "blue",
      "alpha",
      "grid_red",
      "grid_green",
      "grid_blue",
      "grid_alpha",
      "mark_outlines",
    ])
      chartCheck(
        frame[field] === original[field],
        `${chart.id}: Cybertron did not restore ${field}`,
      );
  }
  await driver.record("chart-skin-switching", {
    worlds: glass.charts.map((chart) => chart.world),
    preservesIdentity: true,
    preservesData: true,
  });
}
