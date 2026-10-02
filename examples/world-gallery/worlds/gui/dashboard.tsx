/**
 * The dashboard the Surface presents: the panel World's GuiKit, the theme
 * entities of the few variants it shows, the page, three columns of panels
 * and the roots that float above them: the node rows' context menu, PURGE's
 * confirmation dialog and the toast stack.
 *
 * Every control paints the runtime's default look unless it shows a
 * variant: the switch look for SCAN and EXPLODE LAYERS, the amber look for
 * PURGE, the frameless scroll look for the TELEMETRY body, and the action
 * theme, which ACCENT rewrites between the button look and the amber look in
 * place. The kit's components draw from the same tokens and looks.
 */
import { Children, Entity } from "@ipp/react";
import { Behavior, Font, Theme, ThemeMotion } from "@ipp/react/gui";
import { GuiKit, ToastStack, useContextMenu } from "@ipp/react/gui-kit";
import { useMemo } from "react";
import { ADVANCED_HEIGHT, Advanced } from "./advanced.js";
import {
  ACTION_THEME,
  MONITOR_WIDTH,
  SWITCH_THEME,
  SignalMonitor,
} from "./monitor.js";
import { NodeContextMenu, PurgeDialog, purgeRect } from "./nodes.js";
import {
  BODY,
  BoxLayout,
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  COLUMN,
  CONTRACT,
  Fill,
  ROW,
  STACK,
  Spacer,
  TOKENS,
  WORKBENCH_HEIGHT,
  WORKBENCH_WIDTH,
  type Color,
  type Rect,
} from "./presentation.js";
import type { Accent, GuiSceneState } from "./scene.js";
import { StatusPanel } from "./status.js";
import { ScopePaintAsset } from "./waveform.js";
import { Workbench } from "./workbench.js";
import {
  FRAMELESS_SCROLL_THEME,
  TELEMETRY_HEIGHT,
  TELEMETRY_WIDTH,
  Telemetry,
} from "./telemetry.js";

/** Symbolic ID of the panel World's top-level layout entity. */
export const CANVAS_ENTITY = "gui-canvas";

/** Symbolic ID of the toast stack. */
export const TOAST_STACK_ENTITY = "gui-toasts";

/** Space around and between the columns. */
const GAP = TOKENS.inset;

/** Left edges of the three columns. */
export const COLUMNS = {
  left: GAP,
  centre: GAP + TELEMETRY_WIDTH + GAP,
  right: GAP + TELEMETRY_WIDTH + GAP + MONITOR_WIDTH + GAP,
} as const;

/** The page behind the panels: translucent, so the hologram shows the scene. */
const PAGE: Color = [TOKENS.page[0], TOKENS.page[1], TOKENS.page[2], 0.86];

/**
 * Toasts: a column of control rows at the canvas's bottom right, as wide as
 * the right column and its gap, so they cover none of the STATUS panel's
 * content.
 */
const TOAST_WIDTH = WORKBENCH_WIDTH + GAP;

/**
 * Canvas rectangle of the input shield in front of PURGE: the button with a
 * margin that covers it from the authored camera.
 */
export const SHIELD_CONTENT_RECT = (() => {
  const [x, y, width, height] = purgeRect(COLUMNS.right, GAP);
  const margin = 14;
  return [
    x - margin,
    y - margin,
    width + 2 * margin,
    height + 2 * margin,
  ] as Rect;
})();

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
function VariantThemes({ accent }: { readonly accent: Accent }) {
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

export function ProjectorDashboard({
  scene,
}: {
  readonly scene: GuiSceneState;
}) {
  const font = scene.font.source;
  const nodeMenu = useContextMenu<string>();
  return (
    <GuiKit
      contract={CONTRACT}
      font={font}
      fontSize={BODY}
      reducedMotion={scene.reducedMotion}
    >
      <VariantThemes accent={scene.accent} />
      {/* Assets are declared beside components, never as child entities. */}
      <Entity id="gui-paints">
        <ScopePaintAsset />
      </Entity>
      <Entity id={CANVAS_ENTITY}>
        <BoxLayout kind={STACK} width={CANVAS_WIDTH} height={CANVAS_HEIGHT} />
        <Font source={font} font_size={BODY} />
        {/* Staged content stays hidden and inert until its resources load. */}
        <Behavior visible={scene.prepared} />
        <Children>
          <Fill
            id="gui-page"
            width={CANVAS_WIDTH}
            height={CANVAS_HEIGHT}
            color={PAGE}
          />
          <Entity id="gui-columns">
            <BoxLayout
              kind={ROW}
              width={CANVAS_WIDTH}
              height={CANVAS_HEIGHT}
              padding={[GAP, GAP, GAP, GAP]}
            />
            <Children>
              <Entity id="gui-column-left">
                <BoxLayout kind={COLUMN} width={TELEMETRY_WIDTH} />
                <Children>
                  <Telemetry scene={scene} />
                </Children>
              </Entity>
              <Spacer id="gui-gap-left" width={GAP} />
              <Entity id="gui-column-centre">
                <BoxLayout
                  kind={COLUMN}
                  width={MONITOR_WIDTH}
                  height={TELEMETRY_HEIGHT}
                />
                <Children>
                  <SignalMonitor scene={scene} />
                  <Spacer id="gui-gap-centre" flex={1} />
                  <StatusPanel scene={scene} />
                </Children>
              </Entity>
              <Spacer id="gui-gap-right" width={GAP} />
              <Entity id="gui-column-right">
                <BoxLayout
                  kind={COLUMN}
                  width={WORKBENCH_WIDTH}
                  height={WORKBENCH_HEIGHT + GAP + ADVANCED_HEIGHT}
                />
                <Children>
                  <Workbench scene={scene} menu={nodeMenu} />
                  <Spacer id="gui-gap-nodes" height={GAP} />
                  <Advanced scene={scene} />
                </Children>
              </Entity>
            </Children>
          </Entity>
        </Children>
      </Entity>
      {/* Roots of the canvas on the kit's overlay layers. */}
      <NodeContextMenu scene={scene} menu={nodeMenu} />
      <PurgeDialog scene={scene} />
      <ToastStack
        id={TOAST_STACK_ENTITY}
        toasts={scene.station.toasts}
        onDismiss={scene.station.dismissToast}
        limit={3}
        layout={{ width: TOAST_WIDTH }}
      />
    </GuiKit>
  );
}
