/**
 * Checkbox specimen themes (a02-checkbox and a03-switch). The square
 * checkboxes show the runtime's default checkbox look, so `checkbox` has no
 * rows; rows added here sit on that look property by property. The wide
 * checkboxes used as switches take the runtime's built-in `switch` look.
 */
import type { SkinThemes } from "../theme.js";

export const themes: SkinThemes = {
  checkbox: [],
  switch: { look: "switch" },
};
