import type { PlotColorScale, PlotLegendEntry } from "@ipp/react";

/** Linear RGBA colors shared by source rows, series and their legends. */
export const CHART_COLORS = [
  [0.03, 0.35, 0.88, 1],
  [1, 0.36, 0.04, 1],
  [0.08, 0.68, 0.23, 1],
  [0.7, 0.1, 0.5, 1],
] as const;

export const CHART_HEIGHT_SCALE: PlotColorScale = {
  min: 0,
  max: 4,
  colors: [
    [0.02, 0.06, 0.25, 1],
    [0.03, 0.48, 0.4, 1],
    [0.96, 0.68, 0.12, 1],
  ],
  format: (value) => `${value.toFixed(1)} M`,
};

export const chartCategoryColor = (index: number) =>
  CHART_COLORS[index % CHART_COLORS.length]!;

export function chartLegendEntries(
  labels: readonly string[],
): readonly PlotLegendEntry[] {
  return labels.map((label, index) => ({
    id: `category-${index}`,
    label,
    color: chartCategoryColor(index),
  }));
}
