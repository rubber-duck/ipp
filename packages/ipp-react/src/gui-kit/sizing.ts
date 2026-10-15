import type { MetricsProps } from "../gui/theme.js";
import type { GuiKitContract, GuiKitRow } from "./kit.js";

/**
 * Shared layout at a 16-unit body size. Spacing follows Tailwind's 4-unit
 * scale: label gap-2, form gap-6, sections gap-8 and panel p-6.
 * https://tailwindcss.com/docs/padding · https://headlessui.com/react/fieldset
 * Input inset remains px-3, independent of container padding. Skins change material and colour,
 * while these lengths keep forms steady when their appearance changes.
 * Choice targets remain larger than their visible marks; paint insets never
 * reduce the native hit region. Applications scale everything with fontSize.
 */
export const compactSizing = {
  em: 16,
  controlHeight: 36,
  smallHeight: 24,
  choiceTarget: 28,
  choiceIndicator: 18,
  switchWidth: 40,
  switchHeight: 22,
  dial: 80,
  dialReadoutInside: true,
  dialHousingScale: 1,
  dockedHeight: 20,
  dockedWidth: 28,
  progressInset: 0.5,
  progressHeight: 8,
  bar: 6,
  inset: 12,
  spacing: { compact: 4, label: 8, field: 24, section: 32, inset: 24 },
  row: 28,
  denseRow: 20,
  selectionGutter: 4,
  textSmall: 13,
  textBody: 16,
  textDisplay: 24,
  icon: 20,
} as const;

/** Native measurement and part geometry use the same layout for every peer skin. */
export const compactMetrics = {
  em: compactSizing.em,
  min_width: compactSizing.smallHeight,
  min_height: compactSizing.controlHeight,
  label_inset: compactSizing.inset,
  label_gap: compactSizing.spacing.label,
  rail_thickness: compactSizing.bar,
  thumb_ratio: 0.75,
  checkbox_ratio: 0.5,
  icon_ratio: 0.55,
  number_step_ratio: 0.875,
  dial_inset: 6,
  dial_gap: 4,
  caret_width: 1,
  step_mark: 10,
  color_inset: 8,
  color_gap: 16,
  color_rail: 24,
  color_swatch: 24,
  color_field: 144,
  color_marker: 12,
  color_thumb: 8,
  scroll_thickness: 6,
  scroll_inset: 6,
  scroll_end_inset: 4,
  scroll_tip_ratio: 0.5,
  scroll_min_thumb_ratio: 2,
} satisfies MetricsProps;

export function compactControlMetrics(name: string): MetricsProps {
  const t = compactSizing;
  const sizes: Readonly<Record<string, readonly [number, number]>> = {
    checkbox: [t.choiceTarget, t.choiceTarget],
    switch: [t.switchWidth, t.switchHeight],
    slider: [128, 20],
    dial: [t.dial, t.dial],
    textInput: [128, t.controlHeight],
    scroll: [0, 0],
    color: [0, 0],
  };
  const [min_width, min_height] = sizes[name] ?? [
    t.smallHeight,
    t.controlHeight,
  ];
  return {
    ...compactMetrics,
    min_width,
    min_height,
    // The switch thumb travels between height-square end caps. Its diameter
    // and travel must use the actual track height, with a two-unit rim inside.
    ...(name === "switch"
      ? { checkbox_ratio: (t.switchHeight - 4) / t.switchHeight }
      : {}),
  };
}

/** Keep choice silhouettes compact inside their full-height native target. */
export function compactControlRows(
  contract: Pick<GuiKitContract, "guiPaintPartIndex">,
  role: "checkbox" | "switch",
  rows: readonly GuiKitRow[] = [],
  contour: "native" | "shader" = "native",
): readonly GuiKitRow[] {
  const t = compactSizing;
  const outset =
    contour === "shader"
      ? [0, 0]
      : role === "checkbox"
        ? [
            (t.choiceIndicator - t.choiceTarget) / 2,
            (t.choiceIndicator - t.choiceTarget) / 2,
          ]
        : [0, 0];
  const sizing = ["background", "focusRing"].flatMap((part) =>
    [undefined, "checked", "unchecked"].flatMap((variant) =>
      (variant
        ? ["idle", "hovered", "pressed", "disabled"]
        : [undefined, "idle", "hovered", "pressed", "disabled"]
      ).map((state) => ({
        part: contract.guiPaintPartIndex({
          part,
          ...(state ? { state } : {}),
          ...(variant ? { variant } : {}),
        }),
        scale: [1, 1],
        outset,
      })),
    ),
  );
  const merged = new Map(rows.map((row) => [row.part, row]));
  for (const row of sizing)
    merged.set(row.part, { ...merged.get(row.part), ...row });
  return [...merged.values()];
}
