/**
 * Panel themes (p01-panel-paints): the frame of the GUI kit's containers,
 * written from the design language's tokens, which a custom paint fills over.
 * `panel` is the page fill with the idle accent line and corner accents on
 * all four corners; `lit` adds the focus glow around and inside the frame, to
 * show that a paint leaves the frame's glow alone.
 */
import { GUI_SKIN_TOKENS as T } from "@ipp/host-contract";
import type { SkinRow, SkinThemes } from "../theme.js";

const panel: SkinRow = {
  part: "background",
  color: T.page,
  border_width: T.lineWidth,
  border_color: T.accent,
  corner_accent: [
    T.cornerAccent,
    T.cornerAccent,
    T.cornerAccent,
    T.cornerAccent,
  ],
  corner_accent_width: T.cornerAccentWidth,
};

export const themes: SkinThemes = {
  panel: [panel],
  lit: [
    {
      ...panel,
      glow_color: T.accent,
      glow_intensity: T.focusGlowIntensity,
      glow_radius: T.frameGlowReach,
      glow_falloff: T.glowFalloff,
    },
  ],
};
