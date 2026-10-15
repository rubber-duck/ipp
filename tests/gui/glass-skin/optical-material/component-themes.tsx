import { Entity, assetRef } from "@ipp/react";
import { Metrics, Paint, Theme, ThemeMotion } from "@ipp/react/gui";
import type { GuiKitContract, GuiKitRow } from "@ipp/react/gui-kit";

export const COMPONENT_INK = [0.65, 0.83, 0.87, 1] as const;
export const DROPDOWN_TRIGGER_HEIGHT = 65;
export const DROPDOWN_MENU_HEIGHT = 150;

export function ComponentPaint({
  radius = 12,
  bevel = 4.5,
  roughness = 0.2,
  thickness = 0.8,
  fill = 0,
  lens = 0,
  feedback = [0, 0, 0, 0],
  continuation = [0, 0, 0, 0],
}: {
  radius?: number;
  bevel?: number;
  roughness?: number;
  thickness?: number;
  fill?: number;
  lens?: number;
  feedback?: readonly number[];
  continuation?: readonly number[];
}) {
  return (
    <Paint
      source={assetRef("component-glass")}
      accent={[0.25, 0.62, 0.72, 1]}
      tint={[0.3, 0.57, 0.65, 1]}
      optics={[1.5, roughness, thickness, 1]}
      keyLight={[-3, -2.5, 5, 0.65]}
      eye={[0, 0, 8, 0]}
      state={[0, 1, 0, 0]}
      geometry={[radius, bevel, fill, lens]}
      feedback={feedback}
      continuation={continuation}
    />
  );
}

export function ComponentThemes({
  contract,
  liquid = false,
}: {
  contract: GuiKitContract;
  liquid?: boolean;
}) {
  const key = (part: string, state?: string, variant?: string) =>
    contract.guiPaintPartIndex({
      part,
      ...(state ? { state } : {}),
      ...(variant ? { variant } : {}),
    });
  const table = (rows: GuiKitRow[]) => ({
    nextSlot: rows.length,
    rows: new Map(rows.map((row, i) => [i, row])),
  });
  const stateRows = (
    part: string,
    role: number,
    overrides: Record<string, unknown> = {},
  ) =>
    ["idle", "hovered", "pressed", "disabled"].flatMap((state) =>
      [undefined, "unchecked", "checked"].map((variant) => ({
        part: key(part, state, variant),
        color: [1, 1, 1, 1],
        paint: true,
        paint_data: [
          state === "hovered" ? 1 : 0,
          state === "pressed" ? 1 : 0,
          state === "disabled" ? 1 : 0,
          role === 0 && variant === "checked" ? 1 : role,
        ],
        border_width: 0,
        opacity: 1,
        ...overrides,
      })),
    );
  const names = [
    "button",
    "hover",
    "pressed",
    "focus",
    "disabled",
    "switch",
    "checkbox",
    "radio",
    "slider",
    "input",
    "search",
    "region",
    "dropdown-open",
    "menu",
  ];
  return (
    <>
      {names.map((name) => {
        const capsule = ["switch", "radio", "slider"].includes(name);
        let rows: GuiKitRow[] = [
          {
            part: key("background"),
            color: [1, 1, 1, 1],
            paint: true,
            border_width: 0,
          },
          ...stateRows("background", 0),
          { part: key("label"), color: COMPONENT_INK },
          { part: key("label", "disabled"), color: [0.19, 0.27, 0.3, 1] },
          {
            part: key("focusRing"),
            color: [1, 1, 1, 1],
            paint: true,
            paint_data: [0, 0, 0, 4],
          },
        ];
        if (name === "switch")
          rows.push(
            {
              part: key("icon"),
              color: [1, 1, 1, 1],
              paint: true,
              paint_data: [0, 0, 0, 2],
              scale: [1.6, 1.6],
              align_x: 1,
              shape: 0,
            },
            ...stateRows("icon", 2, { scale: [1.6, 1.6], shape: 0 }),
            ...["idle", "hovered", "pressed", "disabled"].map((state) => ({
              part: key("icon", state, "unchecked"),
              align_x: -1,
            })),
          );
        if (name === "checkbox")
          rows.push({
            part: key("icon"),
            color: COMPONENT_INK,
            shape: 1,
            border_width: 2.8,
            stroke_a: [0.31, 0.51, 0.46, 0.65],
            stroke_b: [0.46, 0.65, 0.74, 0.35],
          });
        if (name === "slider")
          rows.push(
            {
              part: key("fill"),
              color: [1, 1, 1, 1],
              paint: true,
              paint_data: [0, 0, 0, 1],
            },
            ...stateRows("fill", 1),
            {
              part: key("icon"),
              color: [1, 1, 1, 1],
              paint: true,
              paint_data: [0, 0, 0, 2],
            },
            ...stateRows("icon", 2),
          );
        if (name === "input" || name === "search")
          rows.push(
            { part: key("caret"), color: COMPONENT_INK },
            { part: key("selection"), color: [0.15, 0.36, 0.43, 0.5] },
          );
        // Region controls select their local tint paint through compact Skin
        // overrides. State and animation rows remain shared by every region.
        if (name === "region")
          rows = rows.map(({ paint: _paint, ...row }) => row);
        // Later geometry rows merge into, rather than replace, each resolved key.
        const merged = new Map<number, GuiKitRow>();
        for (const row of rows)
          merged.set(row.part, { ...merged.get(row.part), ...row });
        const motion = ["background", "icon"].map((part) => ({
          part: key(part),
          duration: 0.14,
          easing: 0,
        }));
        const feedback = [
          name === "hover" ? 1 : 0,
          name === "pressed" ? 1 : 0,
          name === "disabled" ? 1 : 0,
          name === "focus" ? 1 : 0,
        ];
        return (
          <Entity key={name} id={`component-theme-${name}`}>
            <Theme
              em={0}
              parts={contract.GuiTheme.encodeParts(table([...merged.values()]))}
            />
            <ThemeMotion
              parts={contract.GuiThemeMotion.encodeParts(table(motion))}
            />
            <Metrics
              em={0}
              min_width={0}
              min_height={0}
              label_inset={name === "search" ? 66 : 24}
              label_gap={8}
              checkbox_ratio={name === "switch" ? 0.5 : 1}
              icon_ratio={0.62}
              thumb_ratio={0.7}
              rail_thickness={10}
              caret_width={1.5}
            />
            {name !== "region" && (
              <ComponentPaint
                radius={
                  capsule || (liquid && name === "button")
                    ? 999
                    : name === "checkbox"
                      ? 8
                      : liquid
                        ? 24
                        : 12
                }
                roughness={
                  name === "menu" ? 0.5 : name === "pressed" ? 0.42 : 0.18
                }
                thickness={
                  name === "menu" ? 16 : name === "disabled" ? 1.6 : 0.8
                }
                feedback={feedback}
                lens={name === "radio" ? -1 : 0}
                continuation={
                  name === "dropdown-open"
                    ? [0, DROPDOWN_MENU_HEIGHT, 0, 0]
                    : name === "menu"
                      ? [DROPDOWN_TRIGGER_HEIGHT, 0, 0, 0]
                      : [0, 0, 0, 0]
                }
              />
            )}
          </Entity>
        );
      })}
    </>
  );
}
