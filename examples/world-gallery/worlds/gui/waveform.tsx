/**
 * The scrolling sine trace, independent pulse and reference grid: immutable
 * drawing assets on ordinary Canvas entities. Only the two trace entities
 * move, through Host-owned animation of their Canvas style translation; gain
 * changes their scale without rewriting their geometry.
 */
import type {
  AnimationClipSource,
  AnimationPlaybackEvent,
  ClientAssetSource,
} from "@ipp/client";
import { Animation, Entity, type AnimationHandle } from "@ipp/react";
import { Drawing, Style } from "@ipp/react/gui";
import { useCallback, useEffect, useRef } from "react";
import type { Palette } from "./dashboard.js";
import { BoxLayout, Stack } from "./presentation.js";
import type { GuiSceneState } from "./scene.js";

const WIDTH = 3.54;
const HEIGHT = 0.46;
const PERIOD = 330;
const PACKET_WIDTH = 110;
const CURVE_SCALE = WIDTH / PERIOD;

/** Symbolic IDs of the two animated trace entities. */
export const WAVEFORM_ENTITIES = {
  signal: "gui-waveform-signal",
  pulse: "gui-waveform-pulse",
} as const;

function asset(name: string): ClientAssetSource {
  return {
    kind: 18,
    source: new URL(
      `/target/gallery-gui-assets/${name}.ippd`,
      globalThis.location.href,
    ).href,
  };
}

export function waveformResourceSources(): readonly ClientAssetSource[] {
  return ["waveform-grid", "waveform", "waveform-pulse"].map(asset);
}

/** The Canvas style translation both traces animate. */
export interface WaveformTranslation {
  readonly component: number;
  readonly offset: number;
}

export interface WaveformMotionAssets {
  readonly scan: ClientAssetSource;
  readonly wavePulse: ClientAssetSource;
  readonly translation: WaveformTranslation;
}

/** One linear track of the trace's horizontal Canvas style translation. */
function translationClip(
  translation: WaveformTranslation,
  duration: number,
  from: number,
  to: number,
): AnimationClipSource {
  return {
    duration,
    tracks: [
      {
        property: {
          component: translation.component,
          offsets: [translation.offset],
        },
        keys: [
          {
            time: 0,
            value: { kind: "f32", value: from },
            interpolation: { kind: "linear" },
          },
          { time: duration, value: { kind: "f32", value: to } },
        ],
      },
    ],
  };
}

/** Scan clip: two sine cycles slide through the stationary graph. Pulse
 * clip: the packet crosses the graph from its right edge. */
export function waveformClips(
  translation: WaveformTranslation,
): readonly AnimationClipSource[] {
  return [
    translationClip(translation, 2.4, 0, -WIDTH),
    translationClip(translation, 1.2, WIDTH, -PACKET_WIDTH * CURVE_SCALE),
  ];
}

/** A trace drawing whose curve's zero line sits at the middle of the
 * viewport. The entity's layout box is the authored trace extent; the
 * drawing paints in its own units scaled by the Canvas style. */
function Trace({
  id,
  source,
  width,
  scale,
  color,
  opacity,
}: {
  readonly id: string;
  readonly source: string;
  readonly width: number;
  readonly scale: readonly [number, number];
  readonly color: readonly [number, number, number, number];
  readonly opacity: number;
}) {
  return (
    <Entity id={id}>
      <BoxLayout
        kind={0}
        width={width}
        height={0.01}
        margin={[HEIGHT / 2, 0, 0, 0]}
      />
      <Style
        scale_x={scale[0]}
        scale_y={scale[1]}
        red={color[0]}
        green={color[1]}
        blue={color[2]}
        alpha={color[3]}
        opacity={opacity}
      />
      <Drawing source={source} />
    </Entity>
  );
}

/** Only the traces move; the clipped viewport and reference grid remain
 * fixed. */
export function Waveform({
  scene,
  palette,
}: {
  readonly scene: GuiSceneState;
  readonly palette: Palette;
}) {
  const amplitude = 0.12 + scene.gain * 0.88;
  return (
    <Stack id="gui-waveform" width={WIDTH} height={HEIGHT} clip>
      <Entity id="gui-waveform-grid">
        <BoxLayout kind={0} width={WIDTH} height={HEIGHT} />
        <Style
          scale_x={WIDTH / PERIOD}
          scale_y={HEIGHT / 50}
          red={palette.muted[0]}
          green={palette.muted[1]}
          blue={palette.muted[2]}
          alpha={palette.muted[3]}
          opacity={0.35}
        />
        <Drawing source={asset("waveform-grid").source} />
      </Entity>
      <Trace
        id={WAVEFORM_ENTITIES.signal}
        source={asset("waveform").source}
        width={WIDTH * 2}
        scale={[CURVE_SCALE, CURVE_SCALE * amplitude]}
        color={palette.primary}
        opacity={scene.autoscan ? 0.9 : 0.35}
      />
      <Trace
        id={WAVEFORM_ENTITIES.pulse}
        source={asset("waveform-pulse").source}
        width={PACKET_WIDTH * CURVE_SCALE}
        scale={[CURVE_SCALE, CURVE_SCALE * (0.65 + scene.gain * 0.35)]}
        color={palette.hovered}
        opacity={scene.pulseActive ? 1 : 0}
      />
    </Stack>
  );
}

/** Controllers for both traces, declared in the panel World beside their
 * targets. The Host owns both clocks. */
export function WaveformAnimations({
  scene,
}: {
  readonly scene: GuiSceneState;
}) {
  const scanController = useRef<AnimationHandle>(null);
  const pulseController = useRef<AnimationHandle>(null);
  const previousPulse = useRef(scene.pulseSequence);
  const restart = useRef(Promise.resolve());
  const live = useRef(false);
  const { reportFailure, readWaveformPulse, setPulseActive } = scene;
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const onPlaybackEvent = useCallback(
    (event: AnimationPlaybackEvent) => {
      if (
        event.kind !== "completed" ||
        previousPulse.current !== scene.pulseSequence
      )
        return;
      const sequence = previousPulse.current;
      // Completion can arrive after a newer PULSE. Observe state after its
      // restart acknowledgement.
      void restart.current
        .then(() => {
          if (!live.current || previousPulse.current !== sequence)
            return undefined;
          return readWaveformPulse();
        })
        .then(
          (controller) => {
            if (
              live.current &&
              previousPulse.current === sequence &&
              controller?.id === event.controller.id &&
              controller.state === "completed"
            )
              setPulseActive(false);
          },
          (failure: unknown) => {
            if (live.current && previousPulse.current === sequence)
              reportFailure(failure);
          },
        );
    },
    [scene.pulseSequence, readWaveformPulse, reportFailure, setPulseActive],
  );
  useEffect(() => {
    let active = true;
    // The scan starts once the placed panel has presented a complete frame.
    const action =
      scene.ready && scene.autoscan
        ? scanController.current?.play()
        : scanController.current?.pause();
    void action?.catch((failure: unknown) => {
      if (active) reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [scene.ready, scene.autoscan, reportFailure]);
  useEffect(() => {
    if (previousPulse.current === scene.pulseSequence) return;
    previousPulse.current = scene.pulseSequence;
    let active = true;
    setPulseActive(true);
    restart.current = pulseController.current!.restart();
    void restart.current.catch((failure: unknown) => {
      if (active) reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [scene.pulseSequence, reportFailure, setPulseActive]);
  const motions = scene.motions!;
  const translation = {
    component: motions.translation.component,
    offsets: [motions.translation.offset],
  };
  return (
    <>
      <Animation
        ref={scanController}
        source={motions.scan.source}
        target={WAVEFORM_ENTITIES.signal}
        bindings={[{ track: 0, property: translation }]}
        looping
        autoPlay={false}
      />
      <Animation
        ref={pulseController}
        source={motions.wavePulse.source}
        target={WAVEFORM_ENTITIES.pulse}
        bindings={[{ track: 0, property: translation }]}
        onPlaybackEvent={onPlaybackEvent}
        autoPlay={false}
      />
    </>
  );
}
