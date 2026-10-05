/** A retained Canvas paint; the Host animates the sweep's phase. */
import {
  Animation,
  Entity,
  PaintShader,
  ShaderAsset,
  assetRef,
  type AnimationHandle,
} from "@ipp/react";
import { Box, Paint } from "@ipp/react/gui";
import { useEffect, useRef } from "react";
import { BoxLayout, LEAF } from "./presentation.js";
import type { GuiScene } from "./scene.js";
import { useStoreValue } from "./store.js";
import { SCAN_RATES, linearColor } from "./tuning.js";
export const RADAR_ENTITY = "gui-radar";
export const RADAR_SIZE = 416;
export function contactCount(range: number) {
  return [1.12, 2, 2.88].filter((distance) => distance <= range).length * 2;
}
const RADAR_SHADER = `
vec2 q = (position - size * 0.5) / (min(size.x, size.y) * 0.46);
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
float ink = rim*(0.08*rings+0.12*axes+0.32*sweep+0.7*tip+blips)+pulse*0.85;
float border=1.0-smoothstep(0.005,0.005+aa,abs(r-1.0));
float alpha=clamp(ink+0.5*border,0.0,1.0);
return vec4(p_tint.rgb,alpha*color.a);`;
export function RadarPaintAsset() {
  return (
    <ShaderAsset
      id="gui-radar-shader"
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
      <PaintShader>{RADAR_SHADER}</PaintShader>
    </ShaderAsset>
  );
}
export function Radar({ scene }: { readonly scene: GuiScene }) {
  const controller = useRef<AnimationHandle>(null);
  const gain = useStoreValue(scene.state, (s) => s.gain);
  const range = useStoreValue(scene.state, (s) => s.app.range);
  const pulseStrength = useStoreValue(scene.state, (s) => s.pulseStrength);
  const scanning = useStoreValue(scene.state, (s) => s.autoscan);
  const reduced = useStoreValue(scene.state, (s) => s.reducedMotion);
  const rate = useStoreValue(scene.state, (s) => s.tuning.rate);
  const color = useStoreValue(scene.state, (s) => s.tuning.color);
  const pulse = useStoreValue(scene.state, (s) => s.pulse);
  useEffect(() => {
    const handle = controller.current;
    if (!handle) return;
    let live = true;
    const action =
      scanning && !reduced
        ? handle.playAtSpeed(SCAN_RATES.find((r) => r.key === rate)!.speed)
        : handle.pause();
    void action.catch((error) => {
      if (live) scene.reportDeclarationFailure(error);
    });
    return () => {
      live = false;
    };
  }, [scanning, reduced, rate, scene.reportDeclarationFailure]);
  return (
    <Entity id={RADAR_ENTITY}>
      <BoxLayout kind={LEAF} width={RADAR_SIZE} height={RADAR_SIZE} />
      <Box width={RADAR_SIZE} height={RADAR_SIZE} />
      <Paint
        source={assetRef("gui-radar-shader")}
        phase={0}
        gain={gain}
        range={range}
        tint={[...linearColor(color), 1]}
        pulse={pulse.value}
        active={pulse.state === "running" ? pulseStrength : 0}
      />
      <Animation
        ref={controller}
        source={scene.motions!.sweep.source}
        bindings={[
          {
            track: 0,
            property: {
              component: scene.motions!.paintComponent,
              name: "phase",
            },
          },
        ]}
        looping
        autoPlay={false}
      />
    </Entity>
  );
}
