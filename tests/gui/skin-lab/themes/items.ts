/**
 * Item themes (a01-button-items): Buttons as the items of composites. The
 * primary buttons show the runtime's default button look, so `default` has
 * no rows; the secondary buttons and the list rows take the built-in
 * `secondary` and `docked` looks, whose selected rows give a selected item
 * the lit fill. The tint and leading bar of a selected list row belong to
 * the kit's row look, not to these. The text field and scroll view show
 * their own default looks through `default`.
 */
import type { SkinThemes } from "../theme.js";

export const themes: SkinThemes = {
  default: [],
  secondary: { look: "secondary" },
  docked: { look: "docked" },
};
