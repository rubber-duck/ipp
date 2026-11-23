import type { GuiKitColor, GuiKitContract, GuiKitRow } from "../gui-kit/kit.js";
import {
  cartoonTokens as t,
  blue,
  clear,
  coral,
  cream,
  gold,
  grass,
  honey,
  ink,
  pine,
  sky,
} from "./palette.js";

export function cartoonLooks(contract: GuiKitContract) {
  const key = (part: string, state?: string, variant?: string) =>
    contract.guiPaintPartIndex({
      part,
      ...(state && { state }),
      ...(variant && { variant }),
    });
  const row = (
    part: string,
    fields: Omit<GuiKitRow, "part">,
    state?: string,
    variant?: string,
  ) => ({
    ...fields,
    part: key(part, state, variant),
  });
  const outline = (color: GuiKitColor, radius = 8) => ({
    color,
    corner_radius: [radius, radius],
    border_color: ink,
    border_width: 2.5,
  });
  const focus = row("focusRing", {
    color: gold,
    border_width: 2.5,
    corner_radius: [8, 8],
  });
  const label = [
    row("label", { color: cream }),
    row("label", { color: t.neutral }, "disabled"),
  ];
  const motion = ["background", "label", "icon"].map((part) => ({
    part: key(part),
    duration: 0.09,
    easing: 1,
  }));
  const look = (parts: GuiKitRow[]) => ({ em: t.em, parts, motion });
  const chosen = (part: string, color: GuiKitColor) =>
    ["idle", "hovered", "pressed", "disabled"].map((state) =>
      row(
        part,
        {
          fill_mode: 0,
          color:
            state === "disabled" ? (part === "label" ? cream : t.line) : color,
        },
        state,
        "checked",
      ),
    );
  // Standalone primary buttons carry a hard lower edge. Composed rows use
  // solid faces so selection bars and independently painted children read clearly.
  const band = (top: GuiKitColor, edge: GuiKitColor) => ({
    fill_mode: 1,
    gradient_start: [0, t.controlHeight - 7],
    gradient_end: [0, t.controlHeight - 6.5],
    gradient_color0: top,
    gradient_color1: edge,
  });
  const action = (
    face: GuiKitColor,
    hover: GuiKitColor,
    text: GuiKitColor,
    edge?: GuiKitColor,
  ) =>
    look([
      row("background", { ...outline(face), ...(edge && band(face, edge)) }),
      row(
        "background",
        { color: hover, border_color: cream, ...(edge && band(hover, face)) },
        "hovered",
      ),
      row(
        "background",
        { color: edge ?? face, fill_mode: 0, border_width: 3.5 },
        "pressed",
      ),
      row(
        "background",
        { color: t.surface, fill_mode: 0, border_color: t.line },
        "disabled",
      ),
      ...chosen("background", grass),
      row("label", { color: text }),
      row("label", { color: t.neutral }, "disabled"),
      ...chosen("label", ink),
      focus,
    ]);
  const choice = [
    row("background", outline(t.surface, 6)),
    row("background", { border_color: sky }, "hovered"),
    row("background", { border_color: gold }, "pressed"),
    ...chosen("background", grass),
    ...label,
    focus,
  ];
  const rails = [
    row("background", outline(ink, 8)),
    row("fill", { color: sky, corner_radius: [6, 6] }),
    row("fill", { color: t.line }, "disabled"),
  ];
  const knob = [
    row("icon", outline(gold, 12)),
    row("icon", { color: cream }, "hovered"),
    row("icon", { color: honey }, "pressed"),
    row("icon", { color: t.neutral }, "disabled"),
  ];
  const checker = {
    border_width: 2,
    border_color: ink,
    checker_size: 7,
    checker_color0: cream,
    checker_color1: t.neutral,
  };
  const segment = look([
    row("background", { color: clear, border_width: 0 }),
    row("background", { color: blue }, "hovered"),
    row("background", { color: pine }, "pressed"),
    ...label,
    row("focusRing", {
      color: gold,
      paint: false,
      border_width: 2,
      corner_radius: [8, 8],
      outset: [-2, -2],
    }),
  ]);
  const button = action(blue, t.page, cream);
  const amber = action(coral, gold, ink);
  return {
    primary: action(gold, cream, ink, honey),
    segment,
    button,
    amber,
    secondary: button,
    secondaryAmber: amber,
    docked: button,
    checkbox: look([
      ...choice,
      row("icon", {
        color: ink,
        shape: 1,
        border_width: 3,
        stroke_a: [0.04, 0.46, 0.42, 0.84],
        stroke_b: [0.31, 0.84, 0.97, 0.15],
      }),
    ]),
    switch: look([
      ...choice,
      row("icon", { ...outline(cream, 12), border_width: 1.75, align_x: 1 }),
      ...["idle", "hovered", "pressed", "disabled"].map((state) =>
        row("icon", { align_x: -1, color: t.neutral }, state, "unchecked"),
      ),
    ]),
    slider: look([...rails, ...knob, focus]),
    dial: look([
      row("background", outline(t.surface, 40)),
      row("ticks", {
        color: t.neutral,
        shape: 2,
        border_width: 3,
        arc_dashes: [12, 0.15],
      }),
      row("track", { color: ink, shape: 2, border_width: 5 }),
      row("fill", { color: sky, shape: 2, border_width: 5 }),
      row("fill", { color: t.line }, "disabled"),
      row("icon", {
        color: gold,
        shape: 1,
        border_width: 3,
        stroke_a: [0.5, 0.3, 0.5, 0],
      }),
      ...label,
      focus,
    ]),
    textInput: look([
      row("background", outline(t.surface)),
      row("background", { border_color: sky }, "hovered"),
      row("caret", { color: gold }),
      row("selection", { color: blue }),
      ...label,
      focus,
      ...["decrement", "increment"].flatMap((part) => [
        row(part, outline(blue, 5)),
        row(part, { border_color: sky }, "hovered"),
        row(part, { color: pine }, "pressed"),
        row(part + "Mark", {
          shape: 1,
          color: cream,
          border_width: 2,
          stroke_a: [0, 0.5, 1, 0.5],
          stroke_b: part === "increment" ? [0.5, 0, 0.5, 1] : [0, 0, 0, 0],
        }),
      ]),
    ]),
    scroll: look([
      row("background", outline(t.surface)),
      ...["X", "Y"].flatMap((axis) => [
        row("scrollTrack" + axis, { color: ink, corner_radius: [4, 4] }),
        row("scrollThumb" + axis, { color: sky, corner_radius: [4, 4] }),
        row("scrollThumb" + axis, { color: gold }, "pressed"),
      ]),
    ]),
    color: look([
      row("background", { color: clear }),
      row("track", checker),
      row("fill", checker),
      row("marker", { ...outline(clear, 8), border_color: cream }),
      focus,
    ]),
  };
}
