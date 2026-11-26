import assert from "node:assert/strict";
import {
  DEMO_SKINS,
  isGallerySkin,
  type GallerySkin,
} from "../../../examples/demos/shared/skin-preference.js";

export const families = [
  "actions",
  "editing",
  "values",
  "selection",
  "navigation",
  "containers",
  "feedback",
  "dates-media",
  "overlays",
  "charts",
  "paint",
] as const;
export type Family = (typeof families)[number];
export const modals = [
  "command-open",
  "confirmation-open",
  "drawer-open",
  "dialog-open",
  "popover-open",
  "context-menu-open",
  "tooltip-open",
  "hover-card-open",
  "trigger-menu-open",
  "toast-stack",
];
export const spatialCases = [
  "3d-grid-bars",
  "3d-height-surface",
  "3d-point-plot",
  "3d-variable-pie",
  "3d-single-row",
  "3d-height-surface-1k",
  "3d-point-plot-1k",
];
export function chartCaseName(name: string) {
  if (!name.endsWith("-live")) return name;
  const base = name.slice(0, -5);
  assert.ok(
    ["all", "area", "band", "area-1k", "band-1k", ...spatialCases].includes(
      base,
    ),
    "Unknown live chart case " + name,
  );
  return base;
}

export function isSpatialCase(name: string) {
  return spatialCases.includes(chartCaseName(name));
}

export const projections = ["canvas", "flat", "cylinder", "sphere"] as const;
export type Projection = (typeof projections)[number];
export interface BoardCase {
  name: string;
  theme: GallerySkin;
  projection: Projection;
}
function unique(values: readonly string[]) {
  const keys = values.map((s) => s.trim());
  assert.ok(
    keys.length &&
      keys.every((s) => s.length) &&
      new Set(keys).size === keys.length,
    "Expected distinct, nonempty selections: " + values.join(","),
  );
  return keys;
}
export function guiCases(config: {
  cases?: readonly string[] | null;
  themes?: readonly string[] | null;
  projections?: readonly string[] | null;
}): BoardCase[] {
  const names = unique(
    config.cases ?? ["blank", "all", ...families, ...modals],
  );
  const selected = unique(config.projections ?? projections).map((s) => {
    const shape = projections.find((p) => p === s);
    assert.ok(shape, "Unknown GUI projection " + s);
    return shape;
  });
  const result: BoardCase[] = [];
  const explicit = config.cases || config.themes || config.projections;
  for (const theme of unique(config.themes ?? DEMO_SKINS.map((s) => s.key))) {
    assert.ok(isGallerySkin(theme), "Unknown GUI theme " + theme);
    for (const projection of selected) {
      for (const name of names) {
        if (isSpatialCase(name)) {
          if (projection === selected[0])
            result.push({ name, theme, projection: "canvas" });
        } else if (
          explicit ||
          name === "all" ||
          (theme === "glass" && projection === "canvas")
        )
          result.push({ name, theme, projection });
      }
    }
  }
  return result;
}
