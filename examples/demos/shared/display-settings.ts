/** Supported output scales, shared by display settings and application validation. */
export const DEMO_RENDER_SCALES = [0.5, 0.75, 1, 1.5, 2] as const;

/** The in-world sheet exposes the most useful everyday resolutions. */
export const COCKPIT_RENDER_SCALES = DEMO_RENDER_SCALES.filter(
  (scale) => scale >= 0.75 && scale <= 1.5,
);
