import {
  Align,
  Button,
  Checkbox,
  Column,
  Padding,
  Row,
  ScrollView,
  Slider,
  Stack,
  Text,
  TextInput,
  type GuiControlTheme,
} from "@ipp/react/gui";
import { useMemo, type ReactNode } from "react";
import type { GuiDemoSkin, GuiSceneState } from "./scene.js";
import { Waveform, type WaveformRefs } from "./waveform.js";
import { GUI_ICONS, IconCell, Shape } from "./presentation.js";

export const SURFACE_WIDTH = 7.4;
export const SURFACE_HEIGHT = 4.8;
const CONTENT_WIDTH = 6.8;
const LEFT_WIDTH = 3.24;
const RIGHT_WIDTH = 3.4;
const PULSE_HEIGHT = 0.84;
const PULSE_ICON_CELL = 0.9;
const SPAN_NARROW = 1.2;
const SPAN_WIDE = 2.0;

type Color = readonly [number, number, number, number];

/** SCAN switch knob. The runtime's checkbox indicator is half the 0.48 m
 * track height; scale 1.5 paints a 0.36 m knob that keeps a 0.06 m inset
 * inside the track at either end. Alignment slides it between the end
 * cells, and each palette's skin motion clip samples both ends at `time` so
 * the knob glides through the skin transition. The times are exact in the
 * runtime's f32 time lane, so the last one never passes the clip end. */
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

/** Every skin uses the shared retained shape and glyph renderer. */
function shapeControlTheme(
  scene: GuiSceneState,
  palette: Palette,
  pulse = false,
): GuiControlTheme {
  const state = (color: Color, time: number, scale = 1) => ({
    color,
    opacity: 1,
    scale: [scale, scale] as const,
    transition: {
      motion: scene.motions![scene.skin],
      duration: 0.16,
      easing: "smoothstep" as const,
      track: 0,
      time,
    },
  });
  const edge = {
    cornerRadius: [0.05, 0.05] as const,
    borderWidth: 0.012,
    borderColor: palette.secondary,
  };
  // PULSE brightens radially around its icon cell; other controls shade from
  // top to bottom. Stops use local metres, so resizing keeps their scale.
  const fill = pulse
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
    scene.skin !== "neon"
      ? {}
      : { glow: pulse ? { ...halo, intensity: 0.45, radius: 0.12 } : halo };
  return {
    font: scene.font,
    parts: {
      background: {
        base: {
          ...state(palette.button, 0),
          ...edge,
          gradient: fill,
          ...restingGlow,
        },
        hovered: {
          ...state(palette.hovered, 0.1, 1.025),
          ...edge,
          gradient: fill,
          glow: { ...halo, intensity: 0.3, radius: 0.05 },
        },
        pressed: {
          ...state(palette.pressed, 0.2, 0.985),
          ...edge,
          gradient: fill,
        },
        // A solid dim fill without glow, matching the skin motion's
        // disabled sample so the transition settles on the lane values.
        disabled: {
          ...state(palette.disabled, 0.3),
          opacity: 0.45,
          cornerRadius: [0.05, 0.05] as const,
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
          cornerRadius: [0.06, 0.06] as const,
          borderWidth: 0.024,
          borderColor: palette.focus,
          glow: { ...halo, color: palette.focus, intensity: 0.4, radius: 0.07 },
        },
      },
    },
  };
}

function Label({
  scene,
  text,
  width,
  height,
  size = 0.22,
  color,
  right = false,
}: {
  scene: GuiSceneState;
  text: string;
  width: number;
  height: number;
  size?: number;
  color: Color;
  right?: boolean;
}) {
  return (
    <Align width={width} height={height} alignX={right ? 1 : -1} alignY={0}>
      <Text text={text} asset={scene.font} fontSize={size} color={color} />
    </Align>
  );
}

function Frame({
  width,
  height,
  palette,
  children,
}: {
  width: number;
  height: number;
  palette: Palette;
  children: ReactNode;
}) {
  return (
    <Stack
      width={width}
      height={height}
      backgroundColor={palette.panel}
      theme={{
        parts: {
          background: {
            base: {
              color: palette.panel,
              cornerRadius: [0.05, 0.05],
              borderWidth: 0.008,
              borderColor: [
                palette.secondary[0],
                palette.secondary[1],
                palette.secondary[2],
                0.42,
              ],
            },
          },
        },
      }}
    >
      {children}
    </Stack>
  );
}

function spanWidthFor(scene: GuiSceneState): number {
  return scene.wide ? SPAN_WIDE : SPAN_NARROW;
}

/** A resizable frame: SPAN toggles its width while corner radii, border
 * width and the vertical gradient keep their authored metres. */
function Span({ scene, palette }: { scene: GuiSceneState; palette: Palette }) {
  return (
    <Stack
      width={spanWidthFor(scene)}
      height={0.38}
      alignY={0}
      enabled={false}
      backgroundColor={palette.panel}
      theme={{
        parts: {
          background: {
            base: {
              color: palette.panel,
              cornerRadius: [0.1, 0.1],
              borderWidth: 0.014,
              borderColor: palette.secondary,
              gradient: {
                kind: "linear",
                start: [0, 0],
                end: [0, 0.38],
                color0: dim(palette.secondary, 0.3),
                color1: palette.panel,
              },
            },
          },
        },
      }}
    >
      <Align alignX={0} alignY={0} enabled={false}>
        <Text
          text={scene.wide ? "WIDE" : "NARROW"}
          asset={scene.font}
          fontSize={0.15}
          color={palette.primary}
        />
      </Align>
    </Stack>
  );
}

function Gain({ scene, palette }: { scene: GuiSceneState; palette: Palette }) {
  const theme = useMemo<GuiControlTheme>(
    () => ({
      font: scene.font,
      parts: {
        background: {
          base: {
            color: palette.panel,
            cornerRadius: [0.04, 0.04] as const,
            borderWidth: 0.01,
            borderColor: palette.muted,
          },
        },
        fill: {
          base: {
            color: palette.secondary,
            cornerRadius: [0.04, 0.04] as const,
            gradient: {
              kind: "linear" as const,
              start: [0, 0] as const,
              end: [1, 0] as const,
              color0: palette.secondary,
              color1: palette.primary,
            },
          },
        },
        icon: {
          base: {
            color: palette.primary,
            cornerRadius: [0.08, 0.08] as const,
            borderWidth: 0.01,
            borderColor: palette.secondary,
            glow: {
              color: palette.secondary,
              intensity: scene.skin === "neon" ? 0.18 : 0,
              radius: 0.025,
              falloff: 2,
            },
          },
          hovered: {
            color: palette.hovered,
            scale: [1.12, 1.12],
            cornerRadius: [0.08, 0.08] as const,
            borderWidth: 0.01,
            borderColor: palette.secondary,
          },
          pressed: {
            color: palette.secondary,
            cornerRadius: [0.08, 0.08] as const,
            borderWidth: 0.01,
            borderColor: palette.secondary,
          },
        },
        focusRing: {
          base: {
            color: palette.focus,
            borderWidth: 0.01,
            borderColor: palette.focus,
          },
        },
      },
    }),
    [scene.font, palette],
  );
  return (
    <Frame width={CONTENT_WIDTH} height={0.9} palette={palette}>
      <Row width={6.8} height={0.9} padding={[0, 0.2, 0, 0.2]}>
        <Column width={5.1} height={0.9} padding={[0.08, 0, 0.06, 0]}>
          <Label
            scene={scene}
            text="GAIN"
            width={5.1}
            height={0.3}
            size={0.28}
            color={palette.primary}
          />
          <Stack width={5.1} height={0.36}>
            <Slider
              key="signal-gain"
              width={5.1}
              height={0.36}
              value={0.64}
              min={0}
              max={1}
              step={0.05}
              theme={theme}
              opacity={scene.prepared ? 1 : 0}
              onScalarCommit={(event) => scene.setGain(event.value)}
            />
          </Stack>
          <Stack width={5.1} height={0.1} enabled={false}>
            {Array.from({ length: 25 }, (_, index) => (
              <Shape
                key={index}
                x={0.03 + index * 0.21}
                width={0.012}
                height={index % 4 === 0 ? 0.09 : 0.045}
                material={{ color: palette.muted }}
              />
            ))}
          </Stack>
        </Column>
        <Padding width={0.22} height={0.9} />
        <Label
          scene={scene}
          text={`${Math.round(scene.gain * 100)}%`}
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

function Scan({
  scene,
  palette,
  waveform,
}: {
  scene: GuiSceneState;
  palette: Palette;
  waveform: WaveformRefs;
}) {
  const capsule = [0.24, 0.24] as const;
  const theme = useMemo<GuiControlTheme>(() => {
    const knob = (checked: boolean) => {
      const end = checked ? SWITCH_KNOB.checked : SWITCH_KNOB.unchecked;
      return {
        color: switchKnobColor(palette, checked),
        opacity: 1,
        alignX: end.alignX,
        transition: {
          motion: scene.motions![scene.skin],
          duration: 0.16,
          easing: "smoothstep" as const,
          track: 0,
          time: end.time,
        },
      };
    };
    return {
      font: scene.font,
      parts: {
        background: {
          base: {
            color: palette.panel,
            cornerRadius: capsule,
            borderWidth: 0.012,
            borderColor: palette.muted,
          },
          checked: {
            color: palette.button,
            cornerRadius: capsule,
            borderWidth: 0.012,
            borderColor: palette.secondary,
            gradient: {
              kind: "linear" as const,
              start: [0, 0] as const,
              end: [1, 0] as const,
              color0: palette.secondary,
              color1: palette.button,
            },
            glow: {
              color: palette.secondary,
              intensity: scene.skin === "neon" ? 0.18 : 0,
              radius: 0.035,
              falloff: 2,
            },
          },
          unchecked: {
            color: palette.panel,
            cornerRadius: capsule,
            borderWidth: 0.012,
            borderColor: palette.muted,
          },
          hovered: {
            color: palette.hovered,
            cornerRadius: capsule,
            borderWidth: 0.012,
            borderColor: palette.secondary,
          },
          pressed: {
            color: palette.secondary,
            cornerRadius: capsule,
            borderWidth: 0.012,
            borderColor: palette.secondary,
          },
        },
        icon: {
          // Base lanes are the animated knob's resting values; each variant
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
      },
    };
  }, [scene.font, scene.motions, scene.skin, palette]);
  return (
    <Frame width={CONTENT_WIDTH} height={0.66} palette={palette}>
      <Row width={6.8} height={0.66} padding={[0, 0.2, 0, 0.2]}>
        <Label
          scene={scene}
          text="SCAN"
          width={1.28}
          height={0.66}
          size={0.28}
          color={palette.primary}
        />
        <Align width={1.28} height={0.66} alignX={-1} alignY={0}>
          <Checkbox
            key="autoscan"
            width={1.04}
            height={0.48}
            checked={true}
            theme={theme}
            opacity={scene.prepared ? 1 : 0}
            onToggle={(event) => scene.setAutoscan(event.value)}
          />
        </Align>
        <Padding width={0.3} height={0.66} />
        <Padding width={3.54} height={0.66} padding={[0.1, 0, 0.1, 0]}>
          <Waveform scene={scene} palette={palette} nodes={waveform} />
        </Padding>
      </Row>
    </Frame>
  );
}

function PulseAndSkins({
  scene,
  palette,
}: {
  scene: GuiSceneState;
  palette: Palette;
}) {
  const pulseTheme = useMemo(
    () => shapeControlTheme(scene, palette, true),
    [scene.font, scene.motions, scene.skin, palette],
  );
  const skinTheme = useMemo(
    () => shapeControlTheme(scene, palette),
    [scene.font, scene.motions, scene.skin, palette],
  );
  return (
    <Column width={LEFT_WIDTH} height={1.32}>
      <Stack width={LEFT_WIDTH} height={PULSE_HEIGHT}>
        <Button
          key="pulse"
          width={LEFT_WIDTH}
          height={PULSE_HEIGHT}
          padding={[0.17, 0.2, 0.17, 1.08]}
          label="PULSE"
          fontSize={0.42}
          theme={pulseTheme}
          opacity={scene.prepared ? 1 : 0}
          onPress={scene.pulse}
        />
        <IconCell
          width={PULSE_ICON_CELL}
          height={PULSE_HEIGHT}
          font={scene.font}
          glyph={GUI_ICONS.pulse}
          size={0.45}
          color={palette.primary}
        />
      </Stack>
      <Row width={LEFT_WIDTH} height={0.38} margin={[0.1, 0, 0, 0]}>
        <Label
          scene={scene}
          text="SKIN"
          width={0.48}
          height={0.38}
          size={0.17}
          color={palette.muted}
        />
        <Padding width={0.06} height={0.38} />
        {(["aurora", "ember", "neon"] as const).map((skin) => (
          <Stack
            key={skin}
            width={0.86}
            height={0.38}
            margin={[0, skin === "neon" ? 0 : 0.06, 0, 0]}
          >
            <Button
              key={`skin-${skin}`}
              width={0.86}
              height={0.38}
              padding={[0.085, 0.04, 0.085, 0.27]}
              label={skin.toUpperCase()}
              fontSize={0.15}
              theme={skinTheme}
              opacity={scene.prepared ? 1 : 0}
              onPress={() => scene.selectSkin(skin)}
            />
            <IconCell
              width={0.25}
              height={0.38}
              font={scene.font}
              glyph={GUI_ICONS[skin]}
              size={0.17}
              color={palette.secondary}
            />
          </Stack>
        ))}
      </Row>
    </Column>
  );
}

function Telemetry({
  scene,
  palette,
}: {
  scene: GuiSceneState;
  palette: Palette;
}) {
  return (
    <Frame width={RIGHT_WIDTH} height={1.32} palette={palette}>
      <Column width={3.4} height={1.32} padding={[0.04, 0.16, 0.04, 0.16]}>
        <Label
          scene={scene}
          text="TELEMETRY"
          width={3.08}
          height={0.3}
          size={0.24}
          color={palette.primary}
        />
        <ScrollView
          width={3.05}
          height={0.78}
          margin={[0.1, 0.03, 0, 0]}
          opacity={scene.prepared ? 1 : 0}
        >
          <Column width={2.92} padding={[0, 0, 0.06, 0]}>
            {[
              ["OUTPUT", `${(2.1 + scene.gain * 1.7).toFixed(1)} KW`],
              ["SYNC", scene.autoscan ? "99%" : "HOLD"],
              ["STATUS", scene.autoscan ? "STABLE" : "STANDBY"],
            ].map(([label, value]) => (
              <Row key={label} width={2.92} height={0.25}>
                <Label
                  scene={scene}
                  text={label!}
                  width={1.55}
                  height={0.25}
                  size={0.18}
                  color={palette.muted}
                />
                <Label
                  scene={scene}
                  text={value!}
                  width={1.37}
                  height={0.25}
                  size={0.18}
                  color={palette.primary}
                  right
                />
              </Row>
            ))}
            <Padding width={2.92} height={0.12} />
            {scene.events.map((event, index) => (
              <Text
                key={event}
                text={event}
                width={2.9}
                minHeight={0.28}
                margin={[0, 0, 0.05, 0]}
                asset={scene.font}
                fontSize={0.16}
                color={index === 0 ? palette.primary : palette.muted}
              />
            ))}
          </Column>
        </ScrollView>
      </Column>
    </Frame>
  );
}

export function ProjectorDashboard({
  scene,
  waveform,
}: {
  scene: GuiSceneState;
  waveform: WaveformRefs;
}) {
  const palette = PALETTES[scene.skin];
  const inputTheme = useMemo(
    () => shapeControlTheme(scene, palette),
    [scene.font, scene.motions, scene.skin, palette],
  );
  return (
    <Stack width={SURFACE_WIDTH} height={SURFACE_HEIGHT}>
      <Shape
        x={0.03}
        y={0.03}
        width={SURFACE_WIDTH - 0.06}
        height={SURFACE_HEIGHT - 0.06}
        material={{
          color: palette.shell,
          cornerRadius: [0.17, 0.17],
          borderWidth: 0.014,
          borderColor: palette.primary,
        }}
      />
      <Column
        width={SURFACE_WIDTH}
        height={SURFACE_HEIGHT}
        padding={[0.25, 0.3, 0.25, 0.3]}
      >
        <Row width={CONTENT_WIDTH} height={0.57}>
          <IconCell
            width={0.48}
            height={0.57}
            alignX={-1}
            font={scene.font}
            glyph={GUI_ICONS.cube}
            size={0.36}
            color={palette.secondary}
          />
          <Label
            scene={scene}
            text="GUI DEMO"
            width={2.2}
            height={0.57}
            size={0.4}
            color={palette.primary}
          />
          <Span scene={scene} palette={palette} />
          {/* The flex spacer absorbs SPAN resizing, keeping the header
              controls to its right in place. */}
          <Padding flex={1} height={0.57} />
          <Button
            key="span"
            width={0.8}
            height={0.38}
            alignY={0}
            padding={[0.085, 0.1, 0.085, 0.16]}
            label="SPAN"
            fontSize={0.15}
            theme={inputTheme}
            opacity={scene.prepared ? 1 : 0}
            onPress={scene.toggleSpan}
          />
          <Padding width={0.12} height={0.57} />
          <IconCell
            width={0.55}
            height={0.57}
            alignX={1}
            font={scene.font}
            glyph={GUI_ICONS.signal}
            size={0.3}
            color={palette.primary}
          />
        </Row>
        <Padding width={CONTENT_WIDTH} height={0.12} />
        <Gain scene={scene} palette={palette} />
        <Padding width={CONTENT_WIDTH} height={0.12} />
        <Scan scene={scene} palette={palette} waveform={waveform} />
        <Padding width={CONTENT_WIDTH} height={0.12} />
        <Row width={CONTENT_WIDTH} height={1.32}>
          <PulseAndSkins scene={scene} palette={palette} />
          <Padding width={0.16} height={1.32} />
          <Telemetry scene={scene} palette={palette} />
        </Row>
        <Row width={CONTENT_WIDTH} height={0.38} margin={[0.1, 0, 0, 0]}>
          <Label
            scene={scene}
            text="CALLSIGN"
            width={1.12}
            height={0.38}
            size={0.17}
            color={palette.muted}
          />
          <TextInput
            key="callsign"
            width={2.12}
            height={0.38}
            padding={[0.075, 0.14, 0.075, 0.14]}
            text="VESPER-7"
            placeholder="CALLSIGN"
            fontSize={0.18}
            theme={inputTheme}
            opacity={scene.prepared ? 1 : 0}
            onTextCommit={(event) => scene.setCallsign(event.value)}
          />
          <Padding width={0.16} height={0.38} />
          <Button
            key="uplink"
            width={1.0}
            height={0.38}
            padding={[0.085, 0.1, 0.085, 0.14]}
            label="UPLINK"
            fontSize={0.15}
            theme={inputTheme}
            // Uplink is available only while SCAN holds the waveform.
            enabled={!scene.autoscan}
            opacity={scene.prepared ? 1 : 0}
            onPress={scene.uplink}
          />
          <Padding width={0.12} height={0.38} />
          <Label
            scene={scene}
            text={
              scene.lastCommand === "Awaiting command"
                ? "ARRAY ONLINE  /  V.07"
                : scene.lastCommand.toUpperCase()
            }
            width={2.28}
            height={0.38}
            size={0.15}
            color={palette.muted}
            right
          />
        </Row>
      </Column>
    </Stack>
  );
}
