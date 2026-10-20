/** One cutout pass preserves the complete console and animates authored lamps. */
import { useMemo } from "react";
import {
  Animation,
  AnimationAsset,
  CustomMaterial,
  Entity,
  FragmentShader,
  ShaderAsset,
  assetRef,
} from "@ipp/react";
import type { AnimationClipSource } from "@ipp/client";
import type { CockpitOptions } from "./options.js";
import { textureSource, type CockpitRuntime } from "./runtime.js";

const GROUPS = ["cabin", "dashboard", "buttons", "dials"] as const;
const PARAMETERS = {
  base: "texture2D",
  illumination: "f32",
  cabinPhase: "f32",
  dashboardPhase: "f32",
  buttonsPhase: "f32",
  dialsPhase: "f32",
  cabinTint: "vec3",
  dashboardTint: "vec3",
  buttonsTint: "vec3",
  dialsTint: "vec3",
} as const;

function fragment(zones: CockpitRuntime["lighting"]["zones"]): string {
  // UV regions isolate actual lamps from the silver casing. Emission is
  // attenuated from the authored exposure, so it never blooms beyond the source.
  const rectangles = zones
    .map(({ rect, group }) => {
      const value = rect.map((number) => number.toFixed(9));
      const motion =
        group === "dials"
          ? `float angle = atan(local.y - 0.5, local.x - 0.5);
         float sweep = pow(max(0.0, cos(angle - p_dialsPhase - ${value[0]} * 6.2831853)), 4.0);
         gain = p_dialsTint * (0.10 + 0.90 * sweep);`
          : group === "buttons"
            ? `float lamp = floor(min(local.x, 0.999) * 6.0);
         float wave = 0.5 + 0.5 * cos(p_buttonsPhase - lamp * 0.9 - ${value[0]} * 6.2831853);
         gain = p_buttonsTint * (0.18 + 0.82 * wave * wave);`
            : `gain = p_${group}Tint * (0.78 + 0.22 * cos(p_${group}Phase + v_uv.x));`;
      const footprint =
        group === "buttons"
          ? `float bank = min(v_uv.x, 1.0 - v_uv.x);
           float footprint = 1.0 - smoothstep(0.014, 0.025, abs(v_uv.y - (0.529 - 0.407 * bank)));`
          : group === "dials"
            ? `float footprint = 1.0 - smoothstep(0.98, 1.08, length((local - 0.5) * 2.0));`
            : `float footprint = 1.0;`;
      return `if (all(greaterThanEqual(v_uv,vec2(${value[0]},${value[1]}))) && all(lessThanEqual(v_uv,vec2(${value[2]},${value[3]})))) {
      vec2 local = (v_uv - vec2(${value[0]},${value[1]})) / vec2(${(rect[2] - rect[0]).toFixed(9)},${(rect[3] - rect[1]).toFixed(9)});
      vec3 gain;
      ${motion}
      ${footprint}
      // Follow the curved button bank and elliptical dials so their rectangular
      // registration regions cannot dim the silver casing underneath them.
      float lampMask = footprint * smoothstep(0.18, 0.72, max(base.r, max(base.g, base.b)));
      color = mix(base, base * (0.035 + 0.965 * gain * p_illumination), lampMask);
    }`;
    })
    .join("\n");
  return `vec4 materialFragment() {
    vec4 authored = ippSample(p_base, v_uv);
    if (authored.a < 0.65) ippDiscard();
    vec3 base = authored.rgb;
    vec3 color = base;
    ${rectangles}
    return vec4(color, 1.0);
  }`;
}

const texture = (source: string) =>
  ({
    kind: "asset",
    value: { kind: 2, source: textureSource(source) },
  }) as const;

export function CockpitLighting({
  runtime,
  component,
  state,
}: {
  runtime: CockpitRuntime;
  component: number;
  state: CockpitOptions;
}) {
  const shader = useMemo(() => fragment(runtime.lighting.zones), [runtime]);
  const clip = useMemo<AnimationClipSource>(
    () => ({
      duration: 12,
      tracks: GROUPS.map((group, index) => ({
        property: { component, name: `${group}Phase` },
        keys: [
          {
            time: 0,
            value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
            interpolation: { kind: "linear" },
          },
          {
            time: 12,
            value: {
              kind: "dynamic",
              value: { kind: "f32", value: Math.PI * 2 * [1, 2, 4, 2][index]! },
            },
          },
        ],
      })),
    }),
    [component],
  );
  return (
    <>
      <ShaderAsset
        id="cockpit-light-mask-shader"
        recipe={{}}
        parameters={PARAMETERS}
      >
        <FragmentShader backend="ipp" requiredAttributes={2}>
          {shader}
        </FragmentShader>
      </ShaderAsset>
      <AnimationAsset id="cockpit-light-mask-motion" clip={clip} />
      {runtime.lighting.bodies.map((body) => (
        <Entity key={body.entity} bindTo={body.entity}>
          <CustomMaterial
            source={assetRef("cockpit-light-mask-shader")}
            alpha_mode={1}
            base={texture(body.baseTexture)}
            illumination={state.illumination}
            cabinPhase={0}
            dashboardPhase={0}
            buttonsPhase={0}
            dialsPhase={0}
            cabinTint={[1, 1, 1]}
            dashboardTint={[1, 1, 1]}
            buttonsTint={[1, 1, 1]}
            dialsTint={[1, 1, 1]}
            receives_light={false}
            receives_shadows={false}
            casts_shadows={false}
          />
          <Animation
            source={assetRef("cockpit-light-mask-motion")}
            looping
            playing={state.lightsPlaying}
          />
        </Entity>
      ))}
    </>
  );
}
