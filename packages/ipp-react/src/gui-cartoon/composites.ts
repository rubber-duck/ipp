import type { GuiKitColor, GuiKitContract, GuiKitRow } from "../gui-kit/kit.js";
import type { KitThemeName } from "../gui-kit/themes.js";
import type { GuiSkinDefinition, GuiSkinTheme } from "../gui/skin-types.js";
import { paletteColor } from "../gui-kit/palette.js";
import {
  blue,
  cartoonTokens as t,
  clear,
  gold,
  grass,
  ink,
  pine,
  coral,
} from "./palette.js";
import { meterAppearance } from "./meter-paint.js";

export function cartoonComposites(
  contract: GuiKitContract,
  skin: GuiSkinDefinition,
  themes: Record<string, GuiSkinTheme>,
) {
  const key = (part: string, state?: string, variant?: string) =>
    contract.guiPaintPartIndex({
      part,
      ...(state && { state }),
      ...(variant && { variant }),
    });
  const bg = key("background");
  const patch = (role: KitThemeName, rows: GuiKitRow[]) => {
    const id = skin.kit[role];
    const base = themes[id]!;
    const merged = new Map(base.rows.map((row) => [row.part, row]));
    for (const row of rows)
      merged.set(row.part, { ...merged.get(row.part), ...row });
    themes[id] = { ...base, rows: [...merged.values()] };
  };
  const outline = (radius: number, weight = 2) => ({
    corner_radius: [radius, radius],
    border_color: ink,
    border_width: weight,
  });
  const plaque = (
    role: KitThemeName,
    radius: number,
    weight: number,
    color?: GuiKitColor,
  ) =>
    patch(role, [
      { part: bg, ...outline(radius, weight), ...(color && { color }) },
    ]);
  const surfaceRadius = 12;
  // An open selector joins the floating list into one contour.
  patch("selectTrigger", [
    { part: bg, ...outline(surfaceRadius, 2.6) },
    { part: key("focusRing"), corner_radius: [surfaceRadius, surfaceRadius] },
  ]);
  for (const role of [
    "frame",
    "container",
    "containerUnlit",
    "floating",
    "floatingSmall",
  ] as const)
    plaque(
      role,
      role === "frame" ? 8 : surfaceRadius,
      2.6,
      role === "container" || role === "containerUnlit" ? t.page : t.surface,
    );
  const fill = (role: KitThemeName, color: GuiKitColor, radius = 0) =>
    patch(role, [
      {
        part: bg,
        color,
        fill_mode: 0,
        corner_radius: [radius, radius],
        border_width: 0,
      },
    ]);
  patch("gridHeader", [
    {
      part: bg,
      color: t.surface,
      paint: false,
      corner_radius: [5, 5],
      outset: [0, -2],
    },
  ]);
  fill("division", ink);
  fill("quiet", paletteColor("#425677"));
  const [tabPadX, tabPadY] = t.tabPaintPadding;
  patch("tabRail", [
    {
      part: bg,
      color: t.surface,
      fill_mode: 0,
      border_width: 0,
      corner_radius: [8, 8],
      outset: [-tabPadX, -tabPadY],
    },
  ]);
  // The ring track doubles as the coin's dark face. The native arc drawn
  // over it retains its actual sweep, including complete and spinning states.
  patch("arcTrack", [
    {
      part: bg,
      shape: 0,
      color: t.surface,
      border_color: ink,
      corner_radius: [128, 128],
    },
  ]);
  for (const [role, color, ring] of [
    ["valueAccent", grass, false],
    ["valueAmber", gold, false],
    ["valueError", coral, false],
    ["valueNeutral", t.neutral, false],
    ["arcAccent", grass, true],
    ["arcError", coral, true],
  ] as const) {
    patch(role, [
      {
        part: bg,
        color,
        paint: true,
        paint_data: [ring ? 1 : 0, 0, 0, 0],
        ...(ring ? {} : { corner_radius: [5, 5] }),
      },
    ]);
    const id = skin.kit[role];
    themes[id] = { ...themes[id]!, paint: meterAppearance };
  }
  plaque("progressFrame", 9, 2, blue);
  for (const [kind, color] of [
    ["badgeAccent", blue],
    ["alertInformation", blue],
    ["toastAccent", blue],
    ["badgeNeutral", t.surface],
    ["badgeAmber", paletteColor("#604922")],
    ["alertWarning", paletteColor("#654828")],
    ["toastAmber", paletteColor("#604922")],
    ["badgeError", paletteColor("#71424C")],
    ["alertError", paletteColor("#71424C")],
    ["toastError", paletteColor("#71424C")],
  ] as const)
    plaque(kind, 8, 1.8, color);

  const states = ["idle", "hovered", "pressed"] as const;
  for (const role of ["tab", "stepperStep", "stepperComplete"] as const) {
    const tab = role === "tab";
    patch(role, [
      { part: bg, color: tab ? clear : blue, ...outline(8, tab ? 0 : 2) },
      ...states.flatMap((state) => [
        {
          part: key("background", state, "checked"),
          color: grass,
          ...outline(8),
          fill_mode: 0,
        },
        { part: key("label", state, "checked"), color: ink },
      ]),
      {
        part: key("background", "hovered"),
        color: blue,
        ...outline(8),
        fill_mode: 0,
      },
    ]);
  }
  fill("stepperConnector", t.line, 2);
  fill("stepperConnectorComplete", grass, 2);
  for (const role of [
    "gridRow",
    "menuRow",
    "menuRowAmber",
    "optionCheckRow",
  ] as const) {
    patch(role, [
      { part: bg, color: clear, paint: false, border_width: 0 },
      {
        part: key("background", "hovered"),
        color: blue,
        fill_mode: 0,
        border_width: 0,
        corner_radius: [6, 6],
        ...(role === "gridRow" && { outset: [0, -2] }),
      },
      {
        part: key("background", "pressed"),
        color: pine,
        fill_mode: 0,
        border_width: 0,
      },
      ...states.map((state) => ({
        part: key("background", state, "checked"),
        color: grass,
        ...outline(6, 1.25),
        fill_mode: 0,
      })),
    ]);
  }
  for (const role of ["expanderBody", "expanderBodyDisabled"] as const)
    plaque(role, 9, 2.5, t.surface);
  patch("expanderHeader", [
    { part: bg, color: clear, paint: false, border_width: 0 },
    {
      part: key("background", "pressed"),
      color: clear,
      paint: false,
      border_width: 0,
    },
  ]);
  for (const role of [
    "dockedIcon",
    "dockedIconAmber",
    "secondaryIcon",
    "secondaryIconAmber",
  ] as const) {
    const id = skin.kit[role];
    themes[id] = {
      ...themes[id]!,
      metrics: { ...themes[id]!.metrics, label_inset: 0 },
    };
  }
  patch(
    "optionCheckRow",
    states.map((state) => ({
      part: key("icon", state, "checked"),
      color: ink,
    })),
  );

  // Native circles share geometry across press, release and focus.
  const indicator = t.choiceIndicator;
  const inset = (indicator - t.choiceTarget) / 2;
  const circle = {
    paint: false,
    shape: 0,
    fill_mode: 0,
    corner_radius: [indicator, indicator],
    outset: [inset, inset],
  };
  patch("radio", [
    { part: bg, ...circle, ...outline(indicator, 2.25), color: blue },
    {
      part: key("focusRing"),
      ...circle,
      color: gold,
      border_width: 2,
      outset: [inset + 2, inset + 2],
    },
    ...[...states, "disabled"].flatMap((state) => [
      {
        part: key("background", state, "checked"),
        ...circle,
        border_color: ink,
        border_width: 2.25,
        color: state === "disabled" ? t.line : grass,
      },
      {
        part: key("icon", state, "checked"),
        color: state === "disabled" ? t.neutral : ink,
      },
    ]),
  ]);
}
