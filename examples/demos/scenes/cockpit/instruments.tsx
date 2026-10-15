/** The former scanner's retained radar and waveform, with independent cockpit inputs. */
import { useMemo } from "react";
import {
  Animation,
  AnimationAsset,
  Entity,
  PaintShader,
  ShaderAsset,
  assetRef,
  type AnimationHandle,
} from "@ipp/react";
import { Box, Drawing, Layout, Paint, Style } from "@ipp/react/gui";
import type { AnimationClipSource } from "@ipp/client";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { GalleryAssets } from "../../shared/scene.js";
import type { CockpitOptions } from "./options.js";
import { useEffect, useRef } from "react";

const SIGNAL_WIDTH = 330;
const SIGNAL_HEIGHT = 170;
export const INSTRUMENT_RESOURCES = [
  { kind: 18, path: "/target/demos-drawings/waveform.ippd" },
  { kind: 18, path: "/target/demos-drawings/waveform-pulse.ippd" },
] as const;

export function dynamicClip(
  component: number,
  name: string,
  duration: number,
  to = 1,
): AnimationClipSource {
  return {
    duration,
    tracks: [
      {
        property: { component, name },
        keys: [
          {
            time: 0,
            value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
            interpolation: { kind: "linear" },
          },
          {
            time: duration,
            value: { kind: "dynamic", value: { kind: "f32", value: to } },
          },
        ],
      },
    ],
  };
}

export function contactCount(range: number): number {
  return [1.12, 2, 2.88].filter((distance) => distance <= range).length * 2;
}

// Preserve the scanner's sweep, contact falloff and pulse ring algorithm.
const RADAR = `
vec2 q = (position - size * 0.5) / (min(size.x,size.y) * 0.46);
float r = length(q);
float aa = max(length(fwidth(q)), 0.001);
float rim = 1.0 - smoothstep(1.0-aa, 1.0+aa, r);
float rings = 1.0 - smoothstep(0.006, 0.006+aa, abs(fract(r*4.0+0.5)-0.5)/4.0);
float axes = 1.0 - smoothstep(0.002, 0.002+aa, min(abs(q.x), abs(q.y)));
float angle = atan(q.y,q.x)/6.28318530718;
float tail = fract(p_phase-angle);
float sweep = exp(-tail*18.0)*rim;
float tip = 1.0-smoothstep(0.001,0.001+aa, min(tail,1.0-tail));
float blips = 0.0;
for (int i=0;i<6;i++) {
 float n=float(i);
 float a=0.72+n*2.399963;
 float distance=(0.28+mod(n,3.0)*0.22)*4.0;
 if (distance>p_range) continue;
 vec2 contact=vec2(cos(a),sin(a))*distance/p_range;
 float d=length(q-contact);
 float hit=1.0-smoothstep(0.012+p_gain*0.012,0.02+p_gain*0.014,d);
 float age=fract(p_phase-a/6.28318530718);
 blips=max(blips,hit*(0.18+exp(-age*3.0)));
}
float pulse=exp(-pow((r-p_pulse)/max(0.014,aa),2.0))*p_active;
float ink=rim*(0.08*rings+0.12*axes+0.32*sweep+0.7*tip+blips)+pulse*0.85;
float border=1.0-smoothstep(0.005,0.005+aa,abs(r-1.0));
return vec4(p_tint.rgb,clamp(ink+0.5*border,0.0,1.0)*color.a);`;

const SCOPE = `
vec2 footprint=max(fwidth(position),vec2(1.0e-4));
vec2 cell=size/vec2(8.0,4.0);
vec2 offset=abs(fract(position/cell+0.5)-0.5)*cell;
vec2 lines=clamp((0.5-offset)/footprint+0.5,0.0,1.0);
float grid=max(lines.x,lines.y)*0.18;
float centre=p_phase*size.x;
float distance=(position.x-centre)/max(size.x*0.06,1.0e-4);
float band=exp(-distance*distance)*0.18;
return vec4(p_tint.rgb,clamp(grid+band,0.0,1.0)*color.a);`;

export interface InstrumentContract extends GuiKitContract {
  readonly CanvasPaint: { readonly id: number };
  readonly CanvasStyle: {
    readonly id: number;
    readonly fields: { readonly x: { readonly offset: number } };
  };
}

export function InstrumentAssets({
  contract,
}: {
  contract: InstrumentContract;
}) {
  const sweep = useMemo(
    () => dynamicClip(contract.CanvasPaint.id, "phase", 2.4),
    [contract],
  );
  const pulse = useMemo(
    () => dynamicClip(contract.CanvasPaint.id, "pulse", 1.2),
    [contract],
  );
  const scroll = useMemo<AnimationClipSource>(
    () => ({
      duration: 2.4,
      tracks: [
        {
          property: {
            component: contract.CanvasStyle.id,
            offsets: [contract.CanvasStyle.fields.x!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: 0 },
              interpolation: { kind: "linear" },
            },
            { time: 2.4, value: { kind: "f32", value: -SIGNAL_WIDTH } },
          ],
        },
      ],
    }),
    [contract],
  );
  return (
    <>
      <ShaderAsset
        id="cockpit-radar-shader"
        recipe={{}}
        parameters={{
          phase: "f32",
          gain: "f32",
          range: "f32",
          tint: "vec4",
          pulse: "f32",
          active: "f32",
        }}
      >
        <PaintShader>{RADAR}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id="cockpit-scope-shader"
        recipe={{}}
        parameters={{ phase: "f32", tint: "vec4" }}
      >
        <PaintShader>{SCOPE}</PaintShader>
      </ShaderAsset>
      <AnimationAsset id="cockpit-sweep" clip={sweep} />
      <AnimationAsset id="cockpit-pulse" clip={pulse} />
      <AnimationAsset id="cockpit-wave-scroll" clip={scroll} />
    </>
  );
}

export function RadarInstrument({
  state,
  size,
  report,
}: {
  state: CockpitOptions;
  size: number;
  report(error: unknown): void;
}) {
  const pulse = useRef<AnimationHandle>(null);
  const previous = useRef(state.pulse);
  useEffect(() => {
    if (previous.current === state.pulse) return;
    previous.current = state.pulse;
    void pulse.current?.restart().catch(report);
  }, [state.pulse, report]);
  return (
    <Entity id="cockpit-radar">
      <Layout width={size} height={size} />
      <Box width={size} height={size} />
      <Paint
        source={assetRef("cockpit-radar-shader")}
        phase={0}
        gain={state.gain}
        range={state.range}
        tint={[0.28, 0.72, 0.94, 1]}
        pulse={0}
        active={state.pulse ? 1 : 0}
      />
      <Animation
        source={assetRef("cockpit-sweep")}
        looping
        playing={state.scanning}
      />
      <Animation
        ref={pulse}
        source={assetRef("cockpit-pulse")}
        autoPlay={false}
      />
    </Entity>
  );
}

export function PulseInstrument({
  state,
  assets,
  contract,
  width,
  height,
  report,
}: {
  state: CockpitOptions;
  assets: GalleryAssets;
  contract: InstrumentContract;
  width: number;
  height: number;
  report(error: unknown): void;
}) {
  const controller = useRef<AnimationHandle>(null);
  const previous = useRef(state.pulse);
  const pulseClip = useMemo<AnimationClipSource>(
    () => ({
      duration: 1.2,
      tracks: [
        {
          property: {
            component: contract.CanvasStyle.id,
            offsets: [contract.CanvasStyle.fields.x.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: 0 },
              interpolation: { kind: "linear" },
            },
            {
              time: 1.2,
              value: {
                kind: "f32",
                value: -(width + (110 * width) / SIGNAL_WIDTH),
              },
            },
          ],
        },
      ],
    }),
    [contract, width],
  );
  useEffect(() => {
    if (previous.current === state.pulse) return;
    previous.current = state.pulse;
    void controller.current?.restart().catch(report);
  }, [state.pulse, report]);
  const scale = width / SIGNAL_WIDTH;
  return (
    <Entity id="cockpit-waveform">
      <AnimationAsset id="cockpit-pulse-travel" clip={pulseClip} />
      <Layout kind={3} width={width} height={height} clip />
      <Box width={width} height={height} />
      <Paint
        source={assetRef("cockpit-scope-shader")}
        phase={0}
        tint={[0.28, 0.72, 0.94, 1]}
      />
      <Animation
        source={assetRef("cockpit-sweep")}
        looping
        playing={state.scanning}
      />
      <Entity id="cockpit-wave-trace">
        <Layout width={width * 2} height={1} margin_top={height / 2} />
        <Style
          scale_x={scale}
          scale_y={(height / SIGNAL_HEIGHT) * (0.12 + state.gain * 0.88)}
          red={0.28}
          green={0.72}
          blue={0.94}
          opacity={state.scanning ? 1 : 0.45}
        />
        <Drawing source={assets.url(INSTRUMENT_RESOURCES[0].path)} />
        <Animation
          source={assetRef("cockpit-wave-scroll")}
          looping
          playing={state.scanning}
        />
      </Entity>
      <Entity id="cockpit-wave-pulse">
        <Layout width={width} height={1} margin_top={height / 2} />
        <Style
          x={width}
          scale_x={scale}
          scale_y={(height / SIGNAL_HEIGHT) * (0.65 + state.gain * 0.35)}
          red={0.7}
          green={0.9}
          blue={1}
        />
        <Drawing source={assets.url(INSTRUMENT_RESOURCES[1].path)} />
        <Animation
          ref={controller}
          source={assetRef("cockpit-pulse-travel")}
          autoPlay={false}
        />
      </Entity>
    </Entity>
  );
}
