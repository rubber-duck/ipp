/** Authored dimensions in pixels at 80 px/m, independent of rasterization. */
export const SEPARATOR_CASES = ["horizontal", "vertical"].flatMap((axis) =>
  [0.125, 0.25, 0.5, 1].flatMap((thickness, size) =>
    [0, 0.25, 0.5, 0.75].map((phase, offset) => {
      const index = size * 4 + offset;
      const horizontal = axis === "horizontal";
      return {
        id: `${axis}-${thickness}-${phase}`,
        axis,
        thickness,
        phase,
        scale: offset % 2 === 0 ? 0.5 : 2,
        x: horizontal ? 20 : 180 + index * 7 + phase - thickness / 2,
        y: horizontal ? 20 + index * 10 + phase - thickness / 2 : 40,
        width: horizontal ? 64 : thickness,
        height: horizontal ? thickness : 80,
      };
    }),
  ),
);
