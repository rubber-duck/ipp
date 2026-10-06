/**
 * Button themes (a01-button). The PULSE buttons show the runtime's default
 * button look, so `default` has no rows; rows added here sit on that look
 * property by property. The PURGE variant is the runtime's built-in `amber`
 * look. Window controls and the other buttons of composites are the GUI
 * kit's. The looks follow the design language in the README.
 */
import type { SkinThemes } from "../theme.js";

export const themes: SkinThemes = {
  default: [],
  amber: { look: "amber" },
};
