/**
 * Sheet scales and theme scaling. Skin lengths in `themes/` are in units of
 * the controls sheet (sheet a). Sheets b and c draw their sections at other
 * scales, so a specimen of those crops lays out at the crop's own scale,
 * multiplies every length it adds by the sheet's factor, takes its theme
 * rows through `scaledTheme` and the runtime's built-in looks with that
 * `scale`.
 */
import type { SkinRow, SkinRows } from "./theme.js";

/**
 * Sheet b (gui-components-layout.png): text pitch 1.40 to 1.48 times sheet
 * a's, the editing field 55 against the 39-unit text field (1.41), and the
 * panel frame's line, accent span and accent width all agree on 1.4.
 */
export const SHEET_B_SCALE = 1.4;

/**
 * Sheet c's panel and window-control sections (gui-components-extra.png):
 * captions and descriptions 0.90 to 0.91 times sheet a's pitch, and the
 * close buttons 62 by 37 against sheet b's 95 by 55 (68 by 40 either way).
 */
export const SHEET_C_PANEL_SCALE = 0.93;

/**
 * Sheet c's data grid section, drawn larger than its panels: the editing
 * field 40 high against 39 (1.03), glyph pitch 15.9 against 15.7 (1.01) and
 * capitals 12.7 (1.06).
 */
export const SHEET_C_GRID_SCALE = 1.03;

/**
 * Sheet i, the feedback sheet (gui-components-feedback.png): body capitals
 * 15.5 against 11.2 (1.38) and the inline alerts 57 high against the
 * 40-unit control (1.42).
 */
export const SHEET_I_SCALE = 1.4;

/**
 * Sheet h, the navigation sheet (gui-components-navigation.png): body
 * capitals 15 against 11.2.
 */
export const SHEET_H_SCALE = 1.34;

/**
 * Sheet g, the context sheet (gui-components-context.png): its tree labels'
 * capitals 13.5 against 11.2. Its tree rows are denser than the language's
 * row against that text.
 */
export const SHEET_G_SCALE = 1.2;

/**
 * Sheet d, the value-controls sheet (gui-components-values.png): its numeric
 * stepper 56 high against the 40-unit text field and its range thumbs 24
 * against the 16-unit slider thumb, sheet b's factor.
 */
export const SHEET_D_SCALE = 1.4;

/**
 * Sheet e (gui-components-vertical-sliders.png), drawn like the values sheet
 * d at its factor: the design analysis in Beads ipp-sfgq.19 measured the two
 * sheets together.
 */
export const SHEET_E_SCALE = SHEET_D_SCALE;

/**
 * Sheet f, the selection sheet (gui-components-selection.png): its dropdown
 * triggers 48 high against the 40-unit control.
 */
export const SHEET_F_SCALE = 1.2;

/**
 * Row fields that are lengths in the part's local units: scalars, per-corner
 * lengths and local points. `scale`, `align_x`, opacity, colours and the
 * normalised stroke points are not.
 */
export const LENGTHS = [
  "border_width",
  "corner_radius",
  "corner_cut",
  "corner_accent",
  "corner_accent_width",
  "glow_radius",
  "glow_inner_radius",
  "gradient_start",
  "gradient_end",
  "gradient_radius",
] as const;

/** One row with every length multiplied by `factor`. */
function scaledRow(row: SkinRow, factor: number): SkinRow {
  const scaled: Record<string, unknown> = { ...row };
  for (const field of LENGTHS) {
    const value = row[field];
    if (typeof value === "number") scaled[field] = value * factor;
    else if (Array.isArray(value))
      scaled[field] = value.map((length: number) => length * factor);
  }
  return scaled as SkinRow;
}

/** Rows drawn `factor` times larger: every row length multiplied. */
export function scaledTheme(theme: SkinRows, factor: number): SkinRows {
  return factor === 1 ? theme : theme.map((row) => scaledRow(row, factor));
}
