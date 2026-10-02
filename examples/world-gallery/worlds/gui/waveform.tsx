/**
 * The SIGNAL MONITOR's scope: the scrolling sine trace and the independent
 * pulse, immutable drawing assets on ordinary Canvas entities, over the scope
 * paint, a canvas paint that draws the reference grid or scanlines and the
 * sweep band. Only the two trace entities and the band move, through
 * Host-owned animation of the traces' Canvas style translation and the
 * paint's `phase` property; gain changes the traces' vertical scale and
 * OFFSET their height without rewriting their geometry.
 */
import type {
  AnimationClipSource,
  AnimationPlaybackEvent,
  ClientAssetSource,
} from "@ipp/client";
import {
  Animation,
  Entity,
  PaintShader,
  ShaderAsset,
  assetRef,
  type AnimationHandle,
} from "@ipp/react";
import { Box, Drawing, Paint, Style } from "@ipp/react/gui";
import { useCallback, useEffect, useRef } from "react";
import { BoxLayout, LEAF, Stack, TOKENS, type Color } from "./presentation.js";
import type { GuiPageState, GuiScene } from "./scene.js";
import { useStoreValue } from "./store.js";
import { SCAN_RATES, linearColor } from "./tuning.js";

/** Drawing units per canvas unit: the drawings' 330 by 170 period box. */
const CURVE_SCALE = 0.7;
const PERIOD = 330;
const PACKET_WIDTH = 110;

/** The scope's viewport in canvas units: one drawn period. */
export const WAVE_WIDTH = PERIOD * CURVE_SCALE;
export const WAVE_HEIGHT = 170 * CURVE_SCALE;

/** Seconds the pulse packet takes to cross the scope. */
export const PULSE_SECONDS = 1.2;

/** Seconds of one scan loop: two sine cycles slide through the scope. */
const SCAN_SECONDS = 2.4;

/** Symbolic IDs of the two animated trace entities and the scope paint. */
export const WAVEFORM_ENTITIES = {
  signal: "gui-waveform-signal",
  pulse: "gui-waveform-pulse",
  paint: "gui-scope-paint",
} as const;

/** The scope paint's shader asset. */
export const SCOPE_PAINT = "gui-scope-paint-shader";

/**
 * The scope paint: the grid of eight by four cells and the frame, or
 * scanlines, in the line colour, and the sweep band, a soft vertical band at
 * `phase` of the way from `low` to `high` across the scope. The box paints
 * nothing else, so the panel shows through.
 */
const SCOPE_PAINT_BODY = `
vec2 footprint = max(fwidth(position), vec2(1.0e-4));
vec2 cell = size / p_cells;
vec2 offset = abs(fract(position / cell + 0.5) - 0.5) * cell;
vec2 lines = clamp((0.5 * p_width - offset) / footprint + 0.5, 0.0, 1.0);
float frame = clamp((p_width + edge) / max(fwidth(edge), 1.0e-4) + 0.5, 0.0, 1.0);
float grid = max(max(lines.x, lines.y) * p_grid, frame * max(p_grid, p_scan));
float row = abs(fract(position.y / p_pitch) - 0.5) * p_pitch;
float scan = clamp((0.5 * p_width - row) / footprint.y + 0.5, 0.0, 1.0) * p_scan;
float ink = max(grid, scan * 0.6) * p_line.a;
float centre = mix(p_low, p_high, p_phase) * size.x;
float distance = (position.x - centre) / max(p_band * size.x, 1.0e-4);
float band = exp(-distance * distance) * p_sweep.a;
float alpha = clamp(ink + band, 0.0, 1.0);
vec3 rgb = (p_line.rgb * ink + p_sweep.rgb * band) / max(ink + band, 1.0e-4);
return vec4(rgb, alpha * color.a);`;

/** The scope paint's shader, declared beside the panel's components. */
export function ScopePaintAsset() {
  return (
    <ShaderAsset
      id={SCOPE_PAINT}
      recipe={{}}
      parameters={{
        cells: "vec2",
        width: "f32",
        grid: "f32",
        pitch: "f32",
        scan: "f32",
        line: "vec4",
        sweep: "vec4",
        low: "f32",
        high: "f32",
        band: "f32",
        phase: "f32",
      }}
    >
      <PaintShader>{SCOPE_PAINT_BODY}</PaintShader>
    </ShaderAsset>
  );
}

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
  return ["waveform", "waveform-pulse"].map(asset);
}

/** The Canvas style translation both traces animate. */
export interface WaveformTranslation {
  readonly component: number;
  readonly offset: number;
}

export interface WaveformMotionAssets {
  readonly scan: ClientAssetSource;
  readonly wavePulse: ClientAssetSource;
  /** The scope paint's sweep phase over one scan loop. */
  readonly sweep: ClientAssetSource;
  readonly translation: WaveformTranslation;
  /** The CanvasPaint component, whose `phase` property the sweep animates. */
  readonly paintComponent: number;
}

/**
 * Sweep clip: the band's phase from 0 to 1 over one scan loop, which the
 * scope paint maps onto the SWEEP range. A clip on a property adds its change
 * to the authored value, which rests at 0.
 */
export function sweepClip(paintComponent: number): AnimationClipSource {
  return {
    duration: SCAN_SECONDS,
    tracks: [
      {
        property: { component: paintComponent, name: "phase" },
        keys: [
          {
            time: 0,
            value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
            interpolation: { kind: "linear" },
          },
          {
            time: SCAN_SECONDS,
            value: { kind: "dynamic", value: { kind: "f32", value: 1 } },
          },
        ],
      },
    ],
  };
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

/** Scan clip: two sine cycles slide through the stationary scope. Pulse
 * clip: the packet crosses the scope from its right edge. */
export function waveformClips(
  translation: WaveformTranslation,
): readonly AnimationClipSource[] {
  return [
    translationClip(translation, SCAN_SECONDS, 0, -WAVE_WIDTH),
    translationClip(
      translation,
      PULSE_SECONDS,
      WAVE_WIDTH,
      -PACKET_WIDTH * CURVE_SCALE,
    ),
  ];
}

/** A trace drawing whose zero line runs through the middle of the scope,
 * raised by `y`. The entity's layout box is the authored trace extent; the
 * drawing paints in its own units scaled by the Canvas style. */
function Trace({
  id,
  source,
  width,
  scaleY,
  y,
  color,
  opacity,
}: {
  readonly id: string;
  readonly source: string;
  readonly width: number;
  readonly scaleY: number;
  readonly y: number;
  readonly color: Color;
  readonly opacity: number;
}) {
  return (
    <Entity id={id}>
      <BoxLayout
        kind={LEAF}
        width={width}
        height={1}
        margin={[WAVE_HEIGHT / 2 - 0.5, 0, 0, 0]}
      />
      <Style
        y={y}
        scale_x={CURVE_SCALE}
        scale_y={scaleY}
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

/** Whether the sweep band shows: SCAN is on and the popover shows the sweep.
 * Under reduced motion the band stands still halfway across its range. */
function sweeping(state: GuiPageState): boolean {
  return state.autoscan && state.tuning.sweepShown;
}

/**
 * Only the traces and the band move; the clipped scope stays fixed. GAIN
 * scales the traces, OFFSET raises them, SCAN dims the signal while it stands
 * by, and the scope popover and SWEEP set the paint.
 */
export function Waveform({ scene }: { readonly scene: GuiScene }) {
  const gain = useStoreValue(scene.state, (state) => state.gain);
  const autoscan = useStoreValue(scene.state, (state) => state.autoscan);
  const pulseActive = useStoreValue(scene.state, (state) => state.pulseActive);
  const reducedMotion = useStoreValue(
    scene.state,
    (state) => state.reducedMotion,
  );
  const offset = useStoreValue(scene.state, (state) => state.tuning.offset);
  const color = useStoreValue(scene.state, (state) => state.tuning.color);
  const grid = useStoreValue(scene.state, (state) => state.tuning.grid);
  const sweep = useStoreValue(scene.state, (state) => state.tuning.sweep);
  const swept = useStoreValue(scene.state, sweeping);
  const amplitude = 0.12 + gain * 0.88;
  const y = (-offset / 100) * (WAVE_HEIGHT / 2);
  const line = TOKENS.neutral;
  // The band crosses the scope in the projection's colour.
  const band = linearColor(color);
  return (
    <Stack id="gui-waveform" width={WAVE_WIDTH} height={WAVE_HEIGHT} clip>
      <Entity id={WAVEFORM_ENTITIES.paint}>
        <BoxLayout kind={LEAF} width={WAVE_WIDTH} height={WAVE_HEIGHT} />
        <Box />
        <Paint
          source={assetRef(SCOPE_PAINT)}
          cells={[8, 4]}
          width={1}
          grid={grid === "lines" ? 1 : 0}
          scan={grid === "scanlines" ? 1 : 0}
          pitch={4}
          line={[line[0], line[1], line[2], 0.5]}
          sweep={[band[0], band[1], band[2], swept ? 0.35 : 0]}
          low={sweep[0] / 100}
          high={sweep[1] / 100}
          band={0.06}
          // At rest the band stands halfway; the sweep clip adds its travel.
          phase={reducedMotion ? 0.5 : 0}
        />
      </Entity>
      <Trace
        id={WAVEFORM_ENTITIES.signal}
        source={asset("waveform").source}
        width={WAVE_WIDTH * 2}
        scaleY={CURVE_SCALE * amplitude}
        y={y}
        color={TOKENS.accent}
        opacity={autoscan ? 1 : 0.4}
      />
      <Trace
        id={WAVEFORM_ENTITIES.pulse}
        source={asset("waveform-pulse").source}
        width={PACKET_WIDTH * CURVE_SCALE}
        scaleY={CURVE_SCALE * (0.65 + gain * 0.35)}
        y={y}
        color={TOKENS.text}
        opacity={pulseActive ? 1 : 0}
      />
    </Stack>
  );
}

/** Controllers for both traces, declared in the panel World beside their
 * targets. The Host owns both clocks. */
export function WaveformAnimations({ scene }: { readonly scene: GuiScene }) {
  const scanController = useRef<AnimationHandle>(null);
  const sweepController = useRef<AnimationHandle>(null);
  const pulseController = useRef<AnimationHandle>(null);
  const pulseSequence = useStoreValue(
    scene.state,
    (state) => state.pulseSequence,
  );
  const autoscan = useStoreValue(scene.state, (state) => state.autoscan);
  const reducedMotion = useStoreValue(
    scene.state,
    (state) => state.reducedMotion,
  );
  const rate = useStoreValue(scene.state, (state) => state.tuning.rate);
  const swept = useStoreValue(scene.state, sweeping);
  const previousPulse = useRef(pulseSequence);
  const restart = useRef(Promise.resolve());
  const live = useRef(false);
  const { ready, reportFailure, readWaveformPulse, setPulseActive } = scene;
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const onPlaybackEvent = useCallback(
    (event: AnimationPlaybackEvent) => {
      if (event.kind !== "completed" || previousPulse.current !== pulseSequence)
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
    [pulseSequence, readWaveformPulse, reportFailure, setPulseActive],
  );
  const speed = SCAN_RATES.find(({ key }) => key === rate)?.speed ?? 1;
  useEffect(() => {
    let active = true;
    // The scan starts once the placed panel has presented a complete frame,
    // at the RATE's speed.
    const action =
      ready && autoscan
        ? scanController.current?.playAtSpeed(speed)
        : scanController.current?.pause();
    void action?.catch((failure: unknown) => {
      if (active) reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [ready, autoscan, speed, reportFailure]);
  // The band crosses the SWEEP range with each scan loop; under reduced
  // motion its controller is not declared and the band stands still.
  const sweepRuns = ready && swept && !reducedMotion;
  useEffect(() => {
    let active = true;
    const action = sweepRuns
      ? sweepController.current?.playAtSpeed(speed)
      : sweepController.current?.pause();
    void action?.catch((failure: unknown) => {
      if (active) reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [sweepRuns, speed, reducedMotion, reportFailure]);
  useEffect(() => {
    if (previousPulse.current === pulseSequence) return;
    previousPulse.current = pulseSequence;
    let active = true;
    setPulseActive(true);
    restart.current = pulseController.current!.restart();
    void restart.current.catch((failure: unknown) => {
      if (active) reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [pulseSequence, reportFailure, setPulseActive]);
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
      {!reducedMotion && (
        <Animation
          ref={sweepController}
          source={motions.sweep.source}
          target={WAVEFORM_ENTITIES.paint}
          bindings={[
            {
              track: 0,
              property: { component: motions.paintComponent, name: "phase" },
            },
          ]}
          looping
          autoPlay={false}
        />
      )}
    </>
  );
}
