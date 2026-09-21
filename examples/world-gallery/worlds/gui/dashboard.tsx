import type { ClientAssetSource } from "@ipp/client";
import {
  Align,
  Button,
  Checkbox,
  Column,
  Drawing,
  Padding,
  Row,
  ScrollView,
  Slider,
  Stack,
  Text,
  TextInput,
  type GuiControlTheme,
} from "@ipp/react/gui";
import { useMemo, type ReactNode, type ComponentProps } from "react";
import type { GuiDemoSkin, GuiSceneState } from "./scene.js";
import {
  Waveform,
  waveformResourceSources,
  type WaveformRefs,
} from "./waveform.js";

export const SURFACE_WIDTH = 7.4;
export const SURFACE_HEIGHT = 4.8;
const CONTENT_WIDTH = 6.8;
const LEFT_WIDTH = 3.24;
const RIGHT_WIDTH = 3.4;

type Color = readonly [number, number, number, number];

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
};

const DRAWING_SIZES = {
  "panel-fill": [740, 480],
  "panel-frame": [740, 480],
  "section-frame": [680, 100],
  "telemetry-frame": [344, 136],
  "pulse-button": [328, 84],
  "pulse-button-ember": [328, 84],
  "control-outline": [300, 60],
  "switch-on": [104, 48],
  "switch-off": [104, 48],
  "slider-scale": [480, 15],
  "slider-thumb": [100, 100],
  "slider-track": [510, 10],
  "slider-fill": [1, 1],
  "link-status": [62, 24],
} as const;

function drawing(name: string): ClientAssetSource {
  return {
    kind: 18,
    source: new URL(
      `/target/gallery-gui-assets/${name}.ippd`,
      globalThis.location.href,
    ).href,
  };
}

export function dashboardResourceSources(): readonly ClientAssetSource[] {
  return [
    ...Object.keys(DRAWING_SIZES).map(drawing),
    ...waveformResourceSources(),
  ];
}

/** Drawing leaves retain authored coordinates; controls fit skin assets themselves. */
function Artwork(props: ComponentProps<typeof Drawing>) {
  const name = props.asset?.source.split("/").pop()?.replace(".ippd", "") ?? "";
  const native =
    name === "gui-mark"
      ? [0.52, 0.5]
      : DRAWING_SIZES[name as keyof typeof DRAWING_SIZES];
  return (
    <Drawing
      {...props}
      theme={{
        parts: {
          icon: {
            base: {
              scale: [
                (props.width ?? 1) / native[0]!,
                (props.height ?? 1) / native[1]!,
              ],
            },
          },
        },
      }}
    />
  );
}

function controlTheme(
  scene: GuiSceneState,
  palette: Palette,
  asset: string,
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
  return {
    font: scene.font,
    parts: {
      background: {
        base: { ...state(palette.button, 0), asset: drawing(asset) },
        hovered: state(palette.hovered, 0.1, 1.025),
        pressed: state(palette.pressed, 0.2, 0.985),
        disabled: state(palette.disabled, 0.3),
      },
      label: { base: { color: palette.primary } },
      focusRing: { base: { color: palette.focus, opacity: 1 } },
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
  asset = "section-frame",
}: {
  width: number;
  height: number;
  palette: Palette;
  children: ReactNode;
  asset?: string;
}) {
  return (
    <Stack width={width} height={height} backgroundColor={palette.panel}>
      {children}
      <Artwork
        width={width}
        height={height}
        asset={drawing(asset)}
        color={palette.secondary}
        opacity={0.42}
        enabled={false}
      />
    </Stack>
  );
}

function Gain({ scene, palette }: { scene: GuiSceneState; palette: Palette }) {
  const theme = useMemo<GuiControlTheme>(
    () => ({
      font: scene.font,
      parts: {
        background: {
          base: { color: palette.muted, asset: drawing("slider-track") },
        },
        icon: {
          base: { color: palette.primary, asset: drawing("slider-thumb") },
          hovered: { color: palette.hovered, scale: [1.12, 1.12] },
          pressed: { color: palette.secondary },
        },
        focusRing: { base: { color: palette.focus } },
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
            <Padding width={5.1} height={0.36} padding={[0.142, 0, 0, 0.045]}>
              <Artwork
                width={0.09 + 4.83 * scene.gain}
                height={0.076}
                asset={drawing("slider-fill")}
                color={palette.secondary}
                enabled={false}
              />
            </Padding>
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
          <Artwork
            width={5.04}
            height={0.1}
            margin={[0, 0.03, 0, 0.03]}
            asset={drawing("slider-scale")}
            color={palette.muted}
            enabled={false}
          />
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
  const theme = useMemo<GuiControlTheme>(
    () => ({
      font: scene.font,
      parts: {
        background: {
          base: { color: palette.primary },
          checked: { asset: drawing("switch-on") },
          unchecked: { asset: drawing("switch-off"), color: palette.muted },
          hovered: { color: palette.hovered },
          pressed: { color: palette.secondary },
        },
        icon: {
          base: { opacity: 0 },
          checked: { opacity: 0 },
          unchecked: { opacity: 0 },
        },
        focusRing: { base: { color: palette.focus } },
      },
    }),
    [scene.font, palette],
  );
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
        <Align width={3.54} height={0.66} alignX={0} alignY={0}>
          <Waveform scene={scene} palette={palette} nodes={waveform} />
        </Align>
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
    () =>
      controlTheme(
        scene,
        palette,
        scene.skin === "ember" ? "pulse-button-ember" : "pulse-button",
      ),
    [scene.font, scene.motions, scene.skin, palette],
  );
  const auroraTheme = useMemo(
    () => controlTheme(scene, palette, "pulse-button"),
    [scene.font, scene.motions, scene.skin, palette],
  );
  const emberTheme = useMemo(
    () => controlTheme(scene, palette, "pulse-button-ember"),
    [scene.font, scene.motions, scene.skin, palette],
  );
  return (
    <Column width={LEFT_WIDTH} height={1.32}>
      <Button
        key="pulse"
        width={LEFT_WIDTH}
        height={0.84}
        padding={[0.17, 0.2, 0.17, 1.08]}
        label="PULSE"
        fontSize={0.42}
        theme={pulseTheme}
        opacity={scene.prepared ? 1 : 0}
        onPress={scene.pulse}
      />
      <Row width={LEFT_WIDTH} height={0.38} margin={[0.1, 0, 0, 0]}>
        <Label
          scene={scene}
          text="SKIN"
          width={0.7}
          height={0.38}
          size={0.17}
          color={palette.muted}
        />
        <Button
          key="skin-aurora"
          width={1.22}
          height={0.38}
          padding={[0.085, 0.06, 0.085, 0.38]}
          label="AURORA"
          fontSize={0.16}
          theme={auroraTheme}
          opacity={scene.prepared ? 1 : 0}
          onPress={() => scene.selectSkin("aurora")}
        />
        <Padding width={0.1} height={0.38} />
        <Button
          key="skin-ember"
          width={1.22}
          height={0.38}
          padding={[0.085, 0.06, 0.085, 0.38]}
          label="EMBER"
          fontSize={0.16}
          theme={emberTheme}
          opacity={scene.prepared ? 1 : 0}
          onPress={() => scene.selectSkin("ember")}
        />
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
    <Frame
      width={RIGHT_WIDTH}
      height={1.32}
      palette={palette}
      asset="telemetry-frame"
    >
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
    () => controlTheme(scene, palette, "control-outline"),
    [scene.font, scene.motions, scene.skin, palette],
  );
  return (
    <Stack width={SURFACE_WIDTH} height={SURFACE_HEIGHT}>
      <Artwork
        width={SURFACE_WIDTH}
        height={SURFACE_HEIGHT}
        asset={drawing("panel-fill")}
        color={palette.shell}
        enabled={false}
      />
      <Column
        width={SURFACE_WIDTH}
        height={SURFACE_HEIGHT}
        padding={[0.25, 0.3, 0.25, 0.3]}
      >
        <Row width={CONTENT_WIDTH} height={0.57}>
          <Align width={0.48} height={0.57} alignX={-1} alignY={0}>
            <Artwork
              width={0.32}
              height={0.32}
              asset={scene.mark}
              color={palette.secondary}
              enabled={false}
            />
          </Align>
          <Label
            scene={scene}
            text="GUI DEMO"
            width={5.65}
            height={0.57}
            size={0.4}
            color={palette.primary}
          />
          <Align width={0.67} height={0.57} alignX={1} alignY={0}>
            <Artwork
              width={0.55}
              height={0.24}
              asset={drawing("link-status")}
              color={palette.primary}
              enabled={false}
            />
          </Align>
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
          <Label
            scene={scene}
            text={
              scene.lastCommand === "Awaiting command"
                ? "ARRAY ONLINE  /  V.07"
                : scene.lastCommand.toUpperCase()
            }
            width={3.4}
            height={0.38}
            size={0.15}
            color={palette.muted}
            right
          />
        </Row>
      </Column>
      <Artwork
        width={SURFACE_WIDTH}
        height={SURFACE_HEIGHT}
        asset={drawing("panel-frame")}
        color={palette.primary}
        opacity={0.86}
        enabled={false}
      />
    </Stack>
  );
}
