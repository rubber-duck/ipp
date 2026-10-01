/**
 * The settings panel as ordinary Canvas entities in the panel's own World:
 * layout containers, plain rounded boxes, text and glyph leaves, four control
 * kinds, a ScrollView and a VirtualList, all skinned by theme entities.
 */
import { Children, Entity } from "@ipp/react";
import {
  Behavior,
  Button,
  Checkbox,
  Font,
  ScrollView,
  Skin,
  Slider,
  Text,
  TextInput,
  Theme,
  VirtualList,
  type GuiVirtualRange,
} from "@ipp/react/gui";
import { useMemo, type ReactNode } from "react";
import type { GuiDemoSkin, GuiSceneState } from "./scene.js";
import { Waveform } from "./waveform.js";
import {
  Align,
  BoxLayout,
  Column,
  GUI_ICONS,
  IconCell,
  Label,
  Padding,
  Row,
  Shape,
  Spacer,
  Stack,
  TextLeaf,
  Tint,
  encodeTheme,
  type BoxModel,
  type Color,
  type EncodedTheme,
  type Edges,
  type PartStyle,
  type ThemeParts,
} from "./presentation.js";

export const SURFACE_WIDTH = 7.4;
export const SURFACE_HEIGHT = 4.8;
const CONTENT_WIDTH = 6.8;
const LEFT_WIDTH = 3.24;
const RIGHT_WIDTH = 3.4;
const PULSE_HEIGHT = 0.84;
const TELEMETRY_VIEW_HEIGHT = 0.78;
const EVENT_LOG_VIEW_WIDTH = 2.86;
const EVENT_LOG_VIEW_HEIGHT = 0.56;

/** Canvas logical units one wheel notch scrolls: an eighth of the telemetry
 * viewport, so eight notches page through it. */
export const GUI_WHEEL_STEP = TELEMETRY_VIEW_HEIGHT / 8;

/** Symbolic ID of the panel World's top-level layout entity. */
export const CANVAS_ENTITY = "gui-canvas";

/** Symbolic IDs of the theme entities controls reference. */
export const THEME_ENTITIES = {
  controls: "gui-theme-controls",
  pulse: "gui-theme-pulse",
  gain: "gui-theme-gain",
  scan: "gui-theme-scan",
  bars: "gui-theme-bars",
} as const;

export type ThemeName = keyof typeof THEME_ENTITIES;

/** Event log entry width and type size: the monospaced font fits 31
 * columns, so longer entries wrap into a second line. */
const EVENT_TEXT_WIDTH = 2.74;
const EVENT_FONT_SIZE = 0.16;

/** Event log item extent estimate. A one-line entry measures its 0.28
 * minimum height plus the 0.05 gap below it, and a two-line entry about
 * 0.41. Estimating the taller item means measuring items only ever shortens
 * the content, so the anchored offset settles at the end of the log when
 * its thumb is dragged there: an underestimate would leave the measured
 * excess of the last items below the viewport. */
const EVENT_ITEM_EXTENT = 0.42;

/** Event log items declared beyond each end of the visible ones. */
const EVENT_LOG_OVERSCAN = 2;

/** Operator notes. The Text leaf has a fixed width and no height, so layout
 * wraps it at word boundaries into as many lines as it needs. */
export const TELEMETRY_NOTES =
  "GAIN DRIVES THE PROJECTOR LIGHT, CUBE ENERGY AND WAVE AMPLITUDE. " +
  "HOLD SCAN TO ARM UPLINK. THE AMBER SHIELD GUARDS PURGE FROM STRAY CLICKS.";

/** Bottom command row: CALLSIGN label, callsign editor, UPLINK, the status
 * readout and PURGE, separated by fixed gaps across the content width. */
const COMMAND_ROW = {
  label: 1.12,
  callsign: 2.12,
  uplink: 1.0,
  status: 1.34,
  purge: 0.84,
  height: 0.38,
} as const;

/**
 * Surface content rectangle `[x, y, width, height]` of the input shield in
 * front of PURGE: the button's cell with a 0.1 margin, clear of UPLINK and
 * of the panel's rounded shell. The panel column starts 0.3 from the left
 * and 0.25 from the top; the command row follows 3.81 of rows and gaps plus
 * its 0.1 top margin, and PURGE ends the 6.8-wide row.
 */
export const SHIELD_CONTENT_RECT = [
  0.3 + 6.8 - COMMAND_ROW.purge - 0.1,
  0.25 + 3.81 + 0.1 - 0.1,
  COMMAND_ROW.purge + 0.2,
  COMMAND_ROW.height + 0.2,
] as const;

/** PURGE label inset. Button labels lay out left-aligned from the content
 * origin, so the left padding is tuned as for SPAN and UPLINK below: PURGE
 * measures 0.396 wide, leaving (0.84 - 0.396) / 2 per side. */
const PURGE_LABEL_INSET = 0.222;

export const PULSE_ICON_CELL = 0.9;
const SPAN_NARROW = 1.2;
const SPAN_WIDE = 2.0;

/** SCAN switch knob. The runtime's checkbox indicator is half the 0.48 m
 * track height; scale 1.5 paints a 0.36 m knob that keeps a 0.06 m inset
 * inside the track at either end. Alignment slides it between the end
 * cells, and each palette's skin motion clip samples both ends at `time` so
 * the knob glides through the skin transition. The times are exact in the
 * runtime's f32 time property, so the last one never passes the clip end. */
export const SWITCH_KNOB = {
  scale: 1.5,
  radius: 0.18,
  unchecked: { alignX: -1, time: 0.625 },
  checked: { alignX: 1, time: 0.75 },
} as const;

/** Knob fill inside its primary ring: hollow over the dark track while
 * off, and an opaque shell disc that stands out from the lit track while on. */
export function switchKnobColor(palette: Palette, checked: boolean): Color {
  return checked
    ? [palette.shell[0], palette.shell[1], palette.shell[2], 1]
    : [0, 0, 0, 0];
}

function dim(color: Color, factor: number): Color {
  return [color[0] * factor, color[1] * factor, color[2] * factor, color[3]];
}

export interface Palette {
  readonly shell: Color;
  readonly panel: Color;
  readonly primary: Color;
  readonly secondary: Color;
  readonly muted: Color;
  readonly button: Color;
  readonly hovered: Color;
  readonly pressed: Color;
  readonly disabled: Color;
  readonly focus: Color;
}

export const PALETTES: Readonly<Record<GuiDemoSkin, Palette>> = {
  aurora: {
    shell: [0.004, 0.014, 0.026, 0.9],
    panel: [0.006, 0.021, 0.035, 0.9],
    primary: [0.52, 0.9, 1, 1],
    secondary: [0.2, 0.8, 0.94, 1],
    muted: [0.26, 0.49, 0.62, 1],
    button: [0.38, 0.85, 1, 0.9],
    hovered: [0.78, 0.96, 1, 1],
    pressed: [0.2, 0.65, 0.88, 0.9],
    disabled: [0.2, 0.35, 0.42, 0.45],
    focus: [0.98, 0.79, 0.23, 1],
  },
  ember: {
    shell: [0.027, 0.008, 0.007, 0.9],
    panel: [0.037, 0.014, 0.01, 0.9],
    primary: [1, 0.76, 0.4, 1],
    secondary: [1, 0.4, 0.16, 1],
    muted: [0.67, 0.42, 0.29, 1],
    button: [1, 0.61, 0.27, 0.9],
    hovered: [1, 0.87, 0.58, 1],
    pressed: [0.83, 0.34, 0.12, 0.9],
    disabled: [0.45, 0.23, 0.14, 0.45],
    focus: [1, 0.93, 0.63, 1],
  },
  neon: {
    shell: [0.002, 0.008, 0.016, 0.96],
    panel: [0.004, 0.016, 0.03, 0.96],
    primary: [0.82, 0.98, 1, 1],
    secondary: [0.16, 0.85, 1, 1],
    muted: [0.3, 0.52, 0.62, 1],
    button: [0.05, 0.45, 0.62, 0.95],
    hovered: [0.55, 0.95, 1, 1],
    pressed: [0.04, 0.35, 0.5, 0.95],
    disabled: [0.22, 0.32, 0.36, 0.45],
    focus: [1, 0.8, 0.35, 1],
  },
};

/** Command buttons, the callsign editor and PULSE share one shaded shape.
 * `motion` is the palette's skin motion clip; without it the controls change
 * appearance without a transition. */
function shapeControlTheme(
  skin: GuiDemoSkin,
  palette: Palette,
  motion: string | undefined,
  pulse = false,
): ThemeParts {
  const state = (color: Color, time: number, scale = 1): PartStyle => ({
    color,
    opacity: 1,
    scale: [scale, scale],
    ...(motion
      ? {
          transition: {
            motion,
            duration: 0.16,
            smoothstep: true,
            track: 0,
            time,
          },
        }
      : {}),
  });
  const edge = {
    cornerRadius: [0.05, 0.05],
    borderWidth: 0.012,
    borderColor: palette.secondary,
  } as const;
  // PULSE brightens radially around its icon cell; other controls shade from
  // top to bottom. Stops use local logical units, so resizing keeps their
  // scale.
  const gradient = pulse
    ? {
        kind: "radial" as const,
        start: [PULSE_ICON_CELL / 2, PULSE_HEIGHT / 2] as const,
        radius: 1.7,
        color0: dim(palette.button, 0.4),
        color1: palette.shell,
      }
    : {
        kind: "linear" as const,
        start: [0, 0] as const,
        end: [0, 0.38] as const,
        color0: palette.panel,
        color1: palette.shell,
      };
  const halo = {
    color: palette.secondary,
    intensity: 0.18,
    radius: 0.035,
    falloff: 2,
  };
  // Neon adds a resting halo; PULSE carries the widest one.
  const restingGlow =
    skin !== "neon"
      ? {}
      : { glow: pulse ? { ...halo, intensity: 0.45, radius: 0.12 } : halo };
  return {
    background: {
      base: { ...state(palette.button, 0), ...edge, gradient, ...restingGlow },
      hovered: {
        ...state(palette.hovered, 0.1, 1.025),
        ...edge,
        gradient,
        glow: { ...halo, intensity: 0.3, radius: 0.05 },
      },
      pressed: { ...state(palette.pressed, 0.2, 0.985), ...edge, gradient },
      // A solid dim fill without glow, matching the skin motion's disabled
      // sample so the transition settles on its values.
      disabled: {
        ...state(palette.disabled, 0.3),
        opacity: 0.45,
        cornerRadius: [0.05, 0.05],
        borderWidth: 0.012,
        borderColor: palette.muted,
        glow: { intensity: 0 },
      },
    },
    label: {
      base: { color: palette.primary },
      disabled: { color: palette.muted },
    },
    // A hollow stroke with its own halo; the ring never fills the control.
    focusRing: {
      base: {
        color: palette.focus,
        opacity: 1,
        cornerRadius: [0.06, 0.06],
        borderWidth: 0.024,
        borderColor: palette.focus,
        glow: { ...halo, color: palette.focus, intensity: 0.4, radius: 0.07 },
      },
    },
  };
}

/** Gain track size and its contained thumb travel. The core sizes the slider
 * thumb edge at 0.75 of the control height, and the thumb centre travels
 * from half an edge in from each end; value v sits at
 * `GAIN_THUMB_HALF + v * GAIN_TRAVEL`. */
const GAIN_WIDTH = 5.1;
const GAIN_HEIGHT = 0.36;
const GAIN_THUMB_HALF = (0.75 * GAIN_HEIGHT) / 2;
const GAIN_TRAVEL = GAIN_WIDTH - 2 * GAIN_THUMB_HALF;
const GAIN_TICK_WIDTH = 0.012;

function gainTheme(skin: GuiDemoSkin, palette: Palette): ThemeParts {
  const thumbEdge = {
    cornerRadius: [0.08, 0.08],
    borderWidth: 0.01,
    borderColor: palette.secondary,
  } as const;
  return {
    background: {
      base: {
        color: palette.panel,
        cornerRadius: [0.04, 0.04],
        borderWidth: 0.01,
        borderColor: palette.muted,
      },
    },
    fill: {
      base: {
        color: palette.secondary,
        cornerRadius: [0.04, 0.04],
        gradient: {
          kind: "linear",
          // Stops are in the fill's local units from its left edge; the
          // fill reaches the value-1 thumb centre at full gain.
          start: [0, 0],
          end: [GAIN_WIDTH - GAIN_THUMB_HALF, 0],
          color0: palette.secondary,
          color1: palette.primary,
        },
      },
    },
    icon: {
      base: {
        color: palette.primary,
        ...thumbEdge,
        glow: {
          color: palette.secondary,
          intensity: skin === "neon" ? 0.18 : 0,
          radius: 0.025,
          falloff: 2,
        },
      },
      hovered: { color: palette.hovered, scale: [1.12, 1.12], ...thumbEdge },
      pressed: { color: palette.secondary, ...thumbEdge },
    },
    focusRing: {
      base: {
        color: palette.focus,
        borderWidth: 0.01,
        borderColor: palette.focus,
      },
    },
  };
}

function scanTheme(
  skin: GuiDemoSkin,
  palette: Palette,
  motion: string | undefined,
): ThemeParts {
  const capsule = [0.24, 0.24] as const;
  const outline = (color: Color, border: Color): PartStyle => ({
    color,
    cornerRadius: capsule,
    borderWidth: 0.012,
    borderColor: border,
  });
  const knob = (checked: boolean): PartStyle => {
    const end = checked ? SWITCH_KNOB.checked : SWITCH_KNOB.unchecked;
    return {
      color: switchKnobColor(palette, checked),
      opacity: 1,
      alignX: end.alignX,
      ...(motion
        ? {
            transition: {
              motion,
              duration: 0.16,
              smoothstep: true,
              track: 0,
              time: end.time,
            },
          }
        : {}),
    };
  };
  return {
    background: {
      base: outline(palette.panel, palette.muted),
      checked: {
        ...outline(palette.button, palette.secondary),
        gradient: {
          kind: "linear",
          start: [0, 0],
          end: [1, 0],
          color0: palette.secondary,
          color1: palette.button,
        },
        glow: {
          color: palette.secondary,
          intensity: skin === "neon" ? 0.18 : 0,
          radius: 0.035,
          falloff: 2,
        },
      },
      unchecked: outline(palette.panel, palette.muted),
      hovered: outline(palette.hovered, palette.secondary),
      pressed: outline(palette.secondary, palette.secondary),
    },
    icon: {
      // Base values are the animated knob's resting values; each variant
      // selects its end and clip sample.
      base: {
        color: switchKnobColor(palette, false),
        opacity: 1,
        alignX: SWITCH_KNOB.unchecked.alignX,
        scale: [SWITCH_KNOB.scale, SWITCH_KNOB.scale],
        cornerRadius: [SWITCH_KNOB.radius, SWITCH_KNOB.radius],
        borderWidth: 0.014,
        borderColor: palette.primary,
      },
      checked: knob(true),
      unchecked: knob(false),
    },
    focusRing: {
      base: {
        color: palette.focus,
        cornerRadius: capsule,
        borderWidth: 0.01,
        borderColor: palette.focus,
      },
    },
  };
}

/**
 * Themed scroll bars: a dim rounded track and a bright thumb that lights
 * under hover and press. Disabled track and thumb colours keep both bars
 * visible while their content fits, as the event log does after PURGE.
 */
function scrollBarTheme(palette: Palette): ThemeParts {
  const rounded = [0.015, 0.015] as const;
  const track: PartStyle = {
    color: [palette.muted[0], palette.muted[1], palette.muted[2], 0.35],
    cornerRadius: rounded,
  };
  const thumb = (color: Color): PartStyle => ({ color, cornerRadius: rounded });
  return {
    // Scroll views paint no background of their own.
    background: { base: { color: [0, 0, 0, 0] } },
    scrollTrackY: { base: track, disabled: track },
    scrollThumbY: {
      base: thumb(palette.secondary),
      hovered: thumb(palette.hovered),
      pressed: thumb(palette.primary),
      disabled: thumb(palette.muted),
    },
  };
}

/** Every theme of one skin. `motion` is that skin's motion clip source. */
export function dashboardThemes(
  skin: GuiDemoSkin,
  motion: string | undefined,
): Readonly<Record<ThemeName, EncodedTheme>> {
  const palette = PALETTES[skin];
  return {
    controls: encodeTheme(shapeControlTheme(skin, palette, motion)),
    pulse: encodeTheme(shapeControlTheme(skin, palette, motion, true)),
    gain: encodeTheme(gainTheme(skin, palette)),
    scan: encodeTheme(scanTheme(skin, palette, motion)),
    bars: encodeTheme(scrollBarTheme(palette)),
  };
}

/** Configuration shared by every control entity. */
function Control({
  id,
  scene,
  theme,
  fontSize,
  enabled = true,
  semanticLabel,
  box,
  children,
}: {
  readonly id: string;
  readonly scene: GuiSceneState;
  readonly theme: ThemeName;
  readonly fontSize?: number;
  readonly enabled?: boolean;
  readonly semanticLabel?: string;
  readonly box: BoxModel;
  readonly children: ReactNode;
}) {
  return (
    <Entity id={id}>
      <BoxLayout kind={0} {...box} />
      <Skin theme={THEME_ENTITIES[theme]} />
      {fontSize !== undefined && (
        <Font source={scene.font.source} font_size={fontSize} />
      )}
      {/* Staged controls stay hidden and inert until their resources load. */}
      <Behavior
        visible={scene.prepared}
        enabled={enabled}
        semantic_label={semanticLabel}
      />
      {children}
    </Entity>
  );
}

function Frame({
  id,
  width,
  height,
  palette,
  children,
}: {
  readonly id: string;
  readonly width: number;
  readonly height: number;
  readonly palette: Palette;
  readonly children: ReactNode;
}) {
  return (
    <Stack id={id} width={width} height={height}>
      <Shape
        id={`${id}/fill`}
        width={width}
        height={height}
        color={palette.panel}
        radius={0.05}
      />
      {children}
    </Stack>
  );
}

/** A dimmed accent at the panel fills' translucency, so the SPAN frame's
 * resized edge and corners stand out from the shell. */
function spanFill(palette: Palette): Color {
  const [r, g, b] = dim(palette.secondary, 0.2);
  return [r, g, b, palette.panel[3]];
}

/** A resizable frame: SPAN toggles its width while the corner radii keep
 * their authored logical units. */
function Span({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const width = scene.wide ? SPAN_WIDE : SPAN_NARROW;
  return (
    <Stack id="gui-span-frame" width={width} height={0.38} alignY={0}>
      <Shape
        id="gui-span-frame/fill"
        width={width}
        height={0.38}
        color={spanFill(palette)}
        radius={0.1}
      />
      <Align id="gui-span-frame/label" width={width} height={0.38}>
        <Entity id="gui-span-frame/text">
          <Tint color={palette.primary} />
          <Text
            text={scene.wide ? "WIDE" : "NARROW"}
            source={scene.font.source}
            font_size={0.15}
          />
        </Entity>
      </Align>
    </Stack>
  );
}

function Header({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const font = scene.font.source;
  return (
    <Row id="gui-header" width={CONTENT_WIDTH} height={0.57}>
      <IconCell
        id="gui-icon-dashboard"
        width={0.48}
        height={0.57}
        alignX={-1}
        glyph={GUI_ICONS.dashboard}
        font={font}
        fontSize={0.48}
        color={palette.secondary}
      />
      <Label
        id="gui-title"
        text="GUI DEMO"
        font={font}
        width={2.2}
        height={0.57}
        size={0.4}
        color={palette.primary}
      />
      <Span scene={scene} palette={palette} />
      {/* The flex spacer absorbs SPAN resizing, keeping the header
          controls to its right in place. */}
      <Spacer id="gui-header-gap" flex={1} height={0.57} />
      <Control
        id="gui-span"
        scene={scene}
        theme="controls"
        fontSize={0.15}
        box={{
          width: 0.8,
          height: 0.38,
          alignY: 0,
          // Button labels lay out left-aligned from the content origin, so
          // the left padding is tuned per label to centre the ink: SPAN
          // measures 0.386 wide, leaving (0.8 - 0.386) / 2 per side.
          padding: [0.085, 0.1, 0.085, 0.207],
        }}
      >
        <Button label="SPAN" onPress={scene.toggleSpan} />
      </Control>
      <Spacer id="gui-header-signal-gap" width={0.12} height={0.57} />
      <IconCell
        id="gui-icon-signal"
        width={0.55}
        height={0.57}
        alignX={1}
        glyph={GUI_ICONS.signal}
        font={font}
        fontSize={0.44}
        color={palette.primary}
      />
    </Row>
  );
}

function Gain({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const font = scene.font.source;
  return (
    <Frame
      id="gui-gain-frame"
      width={CONTENT_WIDTH}
      height={0.9}
      palette={palette}
    >
      <Row
        id="gui-gain-row"
        width={CONTENT_WIDTH}
        height={0.9}
        padding={[0, 0.2, 0, 0.2]}
      >
        <Column
          id="gui-gain-column"
          width={GAIN_WIDTH}
          height={0.9}
          padding={[0.08, 0, 0.06, 0]}
        >
          <Label
            id="gui-gain-label"
            text="GAIN"
            font={font}
            width={GAIN_WIDTH}
            height={0.3}
            size={0.28}
            color={palette.primary}
          />
          <Control
            id="gui-gain"
            scene={scene}
            theme="gain"
            semanticLabel="GAIN"
            box={{ width: GAIN_WIDTH, height: GAIN_HEIGHT }}
          >
            <Slider
              value={INITIAL_GAIN}
              min={0}
              max={1}
              step={0.05}
              onScalarCommit={(event) => scene.setGain(event.value)}
            />
          </Control>
          {/* Tick i marks the thumb centre at gain i/24. */}
          <Stack id="gui-gain-ticks" width={GAIN_WIDTH} height={0.1}>
            {Array.from({ length: 25 }, (_, index) => (
              <Shape
                key={index}
                id={`gui-gain-tick-${index}`}
                x={
                  GAIN_THUMB_HALF +
                  (index / 24) * GAIN_TRAVEL -
                  GAIN_TICK_WIDTH / 2
                }
                width={GAIN_TICK_WIDTH}
                height={index % 4 === 0 ? 0.09 : 0.045}
                color={palette.muted}
              />
            ))}
          </Stack>
        </Column>
        <Spacer id="gui-gain-gap" width={0.22} height={0.9} />
        <Label
          id="gui-gain-value"
          text={`${Math.round(scene.gain * 100)}%`}
          font={font}
          width={1.08}
          height={0.84}
          size={0.39}
          color={palette.primary}
          right
        />
      </Row>
    </Frame>
  );
}

/** Initial gain. The declared value never changes, so the slider keeps the
 * user's value afterwards. */
export const INITIAL_GAIN = 0.64;

function Scan({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  return (
    <Frame
      id="gui-scan-frame"
      width={CONTENT_WIDTH}
      height={0.66}
      palette={palette}
    >
      <Row
        id="gui-scan-row"
        width={CONTENT_WIDTH}
        height={0.66}
        padding={[0, 0.2, 0, 0.2]}
      >
        <Label
          id="gui-scan-label"
          text="SCAN"
          font={scene.font.source}
          width={1.28}
          height={0.66}
          size={0.28}
          color={palette.primary}
        />
        <Align
          id="gui-scan-cell"
          width={1.28}
          height={0.66}
          alignX={-1}
          alignY={0}
        >
          <Control
            id="gui-scan"
            scene={scene}
            theme="scan"
            semanticLabel="SCAN"
            box={{ width: 1.04, height: 0.48 }}
          >
            <Checkbox
              checked={INITIAL_AUTOSCAN}
              onToggle={(event) => scene.setAutoscan(event.value)}
            />
          </Control>
        </Align>
        <Spacer id="gui-scan-gap" width={0.3} height={0.66} />
        <Padding
          id="gui-waveform-cell"
          width={3.54}
          height={0.66}
          padding={[0.1, 0, 0.1, 0]}
        >
          <Waveform scene={scene} palette={palette} />
        </Padding>
      </Row>
    </Frame>
  );
}

/** Initial SCAN state. The declared value never changes, so the checkbox
 * keeps the user's value afterwards. */
export const INITIAL_AUTOSCAN = true;

function PulseAndSkins({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const font = scene.font.source;
  return (
    <Column id="gui-pulse-column" width={LEFT_WIDTH} height={1.32}>
      <Stack id="gui-pulse-cell" width={LEFT_WIDTH} height={PULSE_HEIGHT}>
        <Control
          id="gui-pulse"
          scene={scene}
          theme="pulse"
          fontSize={0.42}
          box={{
            width: LEFT_WIDTH,
            height: PULSE_HEIGHT,
            padding: [0.17, 0.2, 0.17, 1.08],
          }}
        >
          <Button label="PULSE" onPress={scene.pulse} />
        </Control>
        <IconCell
          id="gui-icon-pulse"
          width={PULSE_ICON_CELL}
          height={PULSE_HEIGHT}
          glyph={GUI_ICONS.pulse}
          font={font}
          fontSize={0.56}
          color={palette.primary}
        />
      </Stack>
      <Row
        id="gui-skin-row"
        width={LEFT_WIDTH}
        height={0.38}
        margin={[0.1, 0, 0, 0]}
      >
        <Label
          id="gui-skin-label"
          text="SKIN"
          font={font}
          width={0.48}
          height={0.38}
          size={0.17}
          color={palette.muted}
        />
        <Spacer id="gui-skin-gap" width={0.06} height={0.38} />
        {(["aurora", "ember", "neon"] as const).map((skin) => (
          <Stack
            key={skin}
            id={`gui-skin-${skin}-cell`}
            width={0.86}
            height={0.38}
            margin={[0, skin === "neon" ? 0 : 0.06, 0, 0]}
          >
            <Control
              id={`gui-skin-${skin}`}
              scene={scene}
              theme="controls"
              fontSize={0.15}
              box={{
                width: 0.86,
                height: 0.38,
                padding: [0.085, 0.04, 0.085, 0.27],
              }}
            >
              <Button
                label={skin.toUpperCase()}
                onPress={() => scene.selectSkin(skin)}
              />
            </Control>
            <IconCell
              id={`gui-icon-${skin}`}
              width={0.25}
              height={0.38}
              glyph={GUI_ICONS[skin]}
              font={font}
              fontSize={0.22}
              color={palette.secondary}
            />
          </Stack>
        ))}
      </Row>
    </Column>
  );
}

/**
 * Telemetry readouts, wrapped operator notes and the event log. The event
 * log is a VirtualList nested inside the outer telemetry ScrollView: a
 * wheel over the log scrolls the log until it reaches an end, then the
 * runtime passes the unused movement outward to the telemetry view. The
 * runtime scrolls the whole history by its item count and estimate and asks
 * for the items it shows; React declares only those entries.
 */
function Telemetry({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const font = scene.font.source;
  return (
    <Frame
      id="gui-telemetry-frame"
      width={RIGHT_WIDTH}
      height={1.32}
      palette={palette}
    >
      <Column
        id="gui-telemetry-column"
        width={RIGHT_WIDTH}
        height={1.32}
        padding={[0.04, 0.16, 0.04, 0.16]}
      >
        <Label
          id="gui-telemetry-label"
          text="TELEMETRY"
          font={font}
          width={3.08}
          height={0.3}
          size={0.24}
          color={palette.primary}
        />
        <Control
          id="gui-telemetry"
          scene={scene}
          theme="bars"
          semanticLabel="TELEMETRY"
          box={{
            width: 3.05,
            height: TELEMETRY_VIEW_HEIGHT,
            margin: [0.1, 0.03, 0, 0],
          }}
        >
          <ScrollView />
          <Children>
            <Column
              id="gui-telemetry-content"
              width={2.92}
              padding={[0, 0, 0.06, 0]}
            >
              {(
                [
                  ["OUTPUT", `${(2.1 + scene.gain * 1.7).toFixed(1)} KW`],
                  ["SYNC", scene.autoscan ? "99%" : "HOLD"],
                  ["STATUS", scene.autoscan ? "STABLE" : "STANDBY"],
                ] as const
              ).map(([label, value]) => (
                <Row
                  key={label}
                  id={`gui-readout-${label.toLowerCase()}`}
                  width={2.92}
                  height={0.25}
                >
                  <Label
                    id={`gui-readout-${label.toLowerCase()}-name`}
                    text={label}
                    font={font}
                    width={1.55}
                    height={0.25}
                    size={0.18}
                    color={palette.muted}
                  />
                  <Label
                    id={`gui-readout-${label.toLowerCase()}-value`}
                    text={value}
                    font={font}
                    width={1.37}
                    height={0.25}
                    size={0.18}
                    color={palette.primary}
                    right
                  />
                </Row>
              ))}
              <TextLeaf
                id="gui-notes"
                text={TELEMETRY_NOTES}
                width={2.86}
                margin={[0.1, 0, 0.12, 0]}
                font={font}
                fontSize={0.13}
                color={palette.muted}
              />
              <Control
                id="gui-event-log"
                scene={scene}
                theme="bars"
                semanticLabel="EVENT LOG"
                box={{
                  width: EVENT_LOG_VIEW_WIDTH,
                  height: EVENT_LOG_VIEW_HEIGHT,
                }}
              >
                <VirtualList
                  item_count={scene.events.length}
                  item_extent={EVENT_ITEM_EXTENT}
                  overscan={EVENT_LOG_OVERSCAN}
                  onRangeChange={(range: GuiVirtualRange) =>
                    scene.setEventWindow({
                      first: range.first,
                      last: range.last,
                    })
                  }
                  // Each item is a column holding its entry: the column's
                  // measured extent includes the gap below the entry, so the
                  // runtime places the next item after it.
                  renderItem={(index) => (
                    <>
                      <BoxLayout kind={2} width={EVENT_TEXT_WIDTH} />
                      <Children>
                        <Entity id={`gui-event-${index}`}>
                          <BoxLayout
                            kind={0}
                            width={EVENT_TEXT_WIDTH}
                            minHeight={0.28}
                            margin={[0, 0, 0.05, 0]}
                          />
                          <Tint
                            color={
                              index === 0 ? palette.primary : palette.muted
                            }
                          />
                          <Text
                            text={scene.events[index] ?? ""}
                            source={font}
                            font_size={EVENT_FONT_SIZE}
                          />
                        </Entity>
                      </Children>
                    </>
                  )}
                />
              </Control>
            </Column>
          </Children>
        </Control>
      </Column>
    </Frame>
  );
}

function CommandRow({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const font = scene.font.source;
  const button: Edges = [0.085, 0.1, 0.085, 0];
  return (
    <Row
      id="gui-command-row"
      width={CONTENT_WIDTH}
      height={COMMAND_ROW.height}
      margin={[0.1, 0, 0, 0]}
    >
      <Label
        id="gui-callsign-label"
        text="CALLSIGN"
        font={font}
        width={COMMAND_ROW.label}
        height={0.38}
        size={0.17}
        color={palette.muted}
      />
      <Control
        id="gui-callsign"
        scene={scene}
        theme="controls"
        fontSize={0.18}
        semanticLabel="CALLSIGN"
        box={{
          width: COMMAND_ROW.callsign,
          height: 0.38,
          padding: [0.075, 0.14, 0.075, 0.14],
        }}
      >
        <TextInput
          text={INITIAL_CALLSIGN}
          placeholder="CALLSIGN"
          onTextCommit={(event) => scene.setCallsign(event.value)}
        />
      </Control>
      <Spacer id="gui-callsign-gap" width={0.16} height={0.38} />
      <Control
        id="gui-uplink"
        scene={scene}
        theme="controls"
        fontSize={0.15}
        // Uplink is available only while SCAN holds the waveform.
        enabled={!scene.autoscan}
        box={{
          width: COMMAND_ROW.uplink,
          height: 0.38,
          // As for SPAN: UPLINK measures 0.502 wide, leaving
          // (1.0 - 0.502) / 2 per side.
          padding: [button[0], button[1], button[2], 0.249],
        }}
      >
        <Button label="UPLINK" onPress={scene.uplink} />
      </Control>
      <Spacer id="gui-uplink-gap" width={0.12} height={0.38} />
      <Label
        id="gui-status"
        text={
          scene.lastCommand === "Awaiting command"
            ? "ONLINE / V.07"
            : scene.lastCommand.toUpperCase()
        }
        font={font}
        width={COMMAND_ROW.status}
        height={0.38}
        size={0.15}
        color={palette.muted}
        right
      />
      <Spacer id="gui-status-gap" flex={1} height={0.38} />
      {/* PURGE clears the event log. The 3D input shield stands in front
          of it; keyboard traversal and semantic actions still reach it. */}
      <Control
        id="gui-purge"
        scene={scene}
        theme="controls"
        fontSize={0.15}
        box={{
          width: COMMAND_ROW.purge,
          height: 0.38,
          padding: [button[0], button[1], button[2], PURGE_LABEL_INSET],
        }}
      >
        <Button label="PURGE" onPress={scene.purge} />
      </Control>
    </Row>
  );
}

/** Initial callsign. The declared text never changes, so the text input
 * keeps the user's text afterwards. */
export const INITIAL_CALLSIGN = "VESPER-7";

/**
 * Theme entities and the panel layout the Surface presents. Skin selection
 * rewrites the theme rows in place; the controls that reference them keep
 * their identities, focus and committed values.
 */
export function ProjectorDashboard({
  scene,
}: {
  readonly scene: GuiSceneState;
}) {
  const palette = PALETTES[scene.skin];
  const themes = useMemo(
    () => dashboardThemes(scene.skin, scene.motions?.[scene.skin].source),
    [scene.skin, scene.motions],
  );
  return (
    <>
      {(Object.keys(THEME_ENTITIES) as ThemeName[]).map((name) => (
        <Entity key={name} id={THEME_ENTITIES[name]}>
          <Theme parts={themes[name].parts} />
        </Entity>
      ))}
      <Entity id={CANVAS_ENTITY}>
        <BoxLayout kind={3} width={SURFACE_WIDTH} height={SURFACE_HEIGHT} />
        <Children>
          <Shape
            id="gui-shell"
            x={0.03}
            y={0.03}
            width={SURFACE_WIDTH - 0.06}
            height={SURFACE_HEIGHT - 0.06}
            color={palette.shell}
            radius={0.17}
          />
          <Column
            id="gui-content"
            width={SURFACE_WIDTH}
            height={SURFACE_HEIGHT}
            padding={[0.25, 0.3, 0.25, 0.3]}
          >
            <Header scene={scene} palette={palette} />
            <Spacer id="gui-gap-header" width={CONTENT_WIDTH} height={0.12} />
            <Gain scene={scene} palette={palette} />
            <Spacer id="gui-gap-gain" width={CONTENT_WIDTH} height={0.12} />
            <Scan scene={scene} palette={palette} />
            <Spacer id="gui-gap-scan" width={CONTENT_WIDTH} height={0.12} />
            <Row id="gui-lower" width={CONTENT_WIDTH} height={1.32}>
              <PulseAndSkins scene={scene} palette={palette} />
              <Spacer id="gui-lower-gap" width={0.16} height={1.32} />
              <Telemetry scene={scene} palette={palette} />
            </Row>
            <CommandRow scene={scene} palette={palette} />
          </Column>
        </Children>
      </Entity>
    </>
  );
}
