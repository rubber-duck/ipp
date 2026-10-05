/** The scanner app, its retained GuiKit and shared theme entities. */
import { Entity } from "@ipp/react";
import { Theme, ThemeMotion } from "@ipp/react/gui";
import { GuiKit } from "@ipp/react/gui-kit";
import { useMemo, type ReactNode } from "react";
import { ScannerApp, PULSE_RECT } from "./mini-app.js";
import { ACTION_THEME, SWITCH_THEME } from "./monitor.js";
import { BODY, CONTRACT, TOKENS, type Rect } from "./presentation.js";
import type { GuiScene } from "./scene.js";
import { useStoreValue } from "./store.js";
import { FRAMELESS_SCROLL_THEME } from "./telemetry.js";

/** Symbolic ID of the panel World's top-level layout entity. */
export const CANVAS_ENTITY = "gui-canvas";

/** Symbolic ID of the toast stack. */
export const TOAST_STACK_ENTITY = "gui-toasts";

/**
 * Canvas rectangle of the input shield in front of FIRE PULSE: the button with a
 * margin that covers it from the authored camera.
 */
export const SHIELD_CONTENT_RECT: Rect = [
  PULSE_RECT[0] - 14,
  PULSE_RECT[1] - 14,
  PULSE_RECT[2] + 28,
  PULSE_RECT[3] + 28,
];

/** Canvas units one wheel notch scrolls: two dense rows. */
export const GUI_WHEEL_STEP = 2 * TOKENS.denseRow;

type LookName = "button" | "amber" | "switch" | "scroll";

function rowsTable<Row>(rows: readonly Row[]) {
  return {
    nextSlot: rows.length,
    rows: new Map(rows.map((row, slot) => [slot, row] as const)),
  };
}

/** A built-in look as theme rows, with its em and transition timing. */
function lookTheme(
  name: LookName,
  overrides: readonly ({ part: number } & Record<string, unknown>)[] = [],
) {
  const look = CONTRACT.GUI_SKIN_LOOKS[name];
  const rows = new Map(look.parts.map((row) => [row.part, row]));
  for (const row of overrides)
    rows.set(row.part, { ...rows.get(row.part), ...row });
  return {
    em: look.em,
    parts: CONTRACT.GuiTheme.encodeParts(rowsTable([...rows.values()])),
    motion: CONTRACT.GuiThemeMotion.encodeParts(rowsTable(look.motion)),
  };
}

/**
 * The variants the dashboard shows, as ordinary theme entities: the primary
 * actions in the selected accent, the switch, and a scroll view without the
 * list frame, for a body that the panel's frame already holds.
 */
function VariantThemes({ scene }: { readonly scene: GuiScene }) {
  const accent = useStoreValue(scene.state, (state) => state.accent);
  const themes = useMemo(() => {
    const background = CONTRACT.guiPaintPartIndex({ part: "background" });
    return {
      switch: lookTheme("switch"),
      scroll: lookTheme("scroll", [
        { part: background, color: [0, 0, 0, 0], border_width: 0 },
      ]),
      cyan: lookTheme("button"),
      amber: lookTheme("amber"),
    };
  }, []);
  const action = themes[accent];
  return (
    <>
      <Entity id={ACTION_THEME}>
        <Theme parts={action.parts} em={action.em} />
        <ThemeMotion parts={action.motion} />
      </Entity>
      <Entity id={SWITCH_THEME}>
        <Theme parts={themes.switch.parts} em={themes.switch.em} />
        <ThemeMotion parts={themes.switch.motion} />
      </Entity>
      <Entity id={FRAMELESS_SCROLL_THEME}>
        <Theme parts={themes.scroll.parts} em={themes.scroll.em} />
        <ThemeMotion parts={themes.scroll.motion} />
      </Entity>
    </>
  );
}

/**
 * The panel World's GuiKit. REDUCED MOTION is its preference; changing it
 * re-renders the kit and what reads the preference, not the dashboard, whose
 * elements the kit receives unchanged.
 */
function DashboardKit({
  scene,
  children,
}: {
  readonly scene: GuiScene;
  readonly children: ReactNode;
}) {
  const reducedMotion = useStoreValue(
    scene.state,
    (state) => state.reducedMotion,
  );
  return (
    <GuiKit
      contract={CONTRACT}
      font={scene.font.source}
      fontSize={BODY}
      reducedMotion={reducedMotion}
    >
      {children}
    </GuiKit>
  );
}

/**
 * The dashboard. It selects no value from the page's state: each section
 * selects what it shows, so a value change re-renders only the parts that
 * show it.
 */
export function ProjectorDashboard({ scene }: { readonly scene: GuiScene }) {
  return (
    <DashboardKit scene={scene}>
      <VariantThemes scene={scene} />
      <ScannerApp scene={scene} />
    </DashboardKit>
  );
}
