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
import type { CockpitRuntime } from "./runtime.js";
import {
  lightTextureParameters,
  lightTextureProperties,
  lightTextureSampling,
} from "./light-textures.js";

const GROUPS = ["cabin", "dashboard", "buttons", "dials"] as const;
const PARAMETERS = {
  ...lightTextureParameters(3),
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
  // Regions assign the separate lamp image to independently animated groups.
  // Base metalwork never participates in the animation or brightness control.
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
      return `if (all(greaterThanEqual(v_uv,vec2(${value[0]},${value[1]}))) && all(lessThanEqual(v_uv,vec2(${value[2]},${value[3]})))) {
      vec2 local = (v_uv - vec2(${value[0]},${value[1]})) / vec2(${(rect[2] - rect[0]).toFixed(9)},${(rect[3] - rect[1]).toFixed(9)});
      ${motion}
    }`;
    })
    .join("\n");
  return (
    lightTextureSampling(3) +
    `vec4 materialFragment() {
    vec4 authored = cockpitBase(v_uv);
    if (authored.a < 0.65) ippDiscard();
    vec3 gain = p_cabinTint * (0.78 + 0.22 * cos(p_cabinPhase));
    ${rectangles}
    float contribution = cockpitContribution(v_uv).a;
    return vec4(authored.rgb / authored.a + gain * contribution * p_illumination, 1.0);
  }`
  );
}

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
            {...lightTextureProperties(body.tiles)}
            illumination={state.illumination}
            cabinPhase={0}
            dashboardPhase={0}
            buttonsPhase={0}
            dialsPhase={0}
            cabinTint={[1, 0.86, 0.68]}
            dashboardTint={[1, 0.92, 0.8]}
            buttonsTint={[1, 0.86, 0.68]}
            dialsTint={[1, 0.86, 0.68]}
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
