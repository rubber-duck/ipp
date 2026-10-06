/**
 * Scroll view and virtual list specimen themes (a06-scroll-view and
 * a07-virtual-list). The lists show the runtime's default scroll look and
 * default bar geometry, so `default` has no rows; rows added here sit on that
 * look property by property. Rows, separators and the empty-list text are
 * application content of the specimens. Motion on the sheet, for the runtime
 * default: content scrolls immediately, clamps at both ends without bounce,
 * and rows have no entrance or placeholder animation.
 */
import type { SkinThemes } from "../theme.js";

export const themes: SkinThemes = {
  default: [],
};
