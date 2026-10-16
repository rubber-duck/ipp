/** One opaque texture pass preserves the photograph while animating light groups. */
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
  cabin: "texture2D",
  dashboard: "texture2D",
  cyan: "texture2D",
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
  // These constants are authored UV classification, independent of the viewport.
  const rectangles = zones
    .filter(({ group }) => group === "buttons" || group === "dials")
    .map(({ rect, group }) => {
      const value = rect.map((number) => number.toFixed(9));
      const motion =
        group === "dials"
          ? `float angle = atan(local.y - 0.5, local.x - 0.5);
           float sweep = pow(max(0.0, cos(angle - p_dialsPhase - ${value[0]} * 6.2831853)), 4.0);
           cyanGain = p_dialsTint * (0.22 + 0.78 * sweep);`
          : `float lamp = floor(min(local.x, 0.999) * 6.0);
           float wave = 0.5 + 0.5 * cos(p_buttonsPhase - lamp * 0.9 - ${value[0]} * 6.2831853);
           cyanGain = p_buttonsTint * (0.25 + 0.75 * wave * wave);`;
      return `if (all(greaterThanEqual(v_uv,vec2(${value[0]},${value[1]}))) && all(lessThanEqual(v_uv,vec2(${value[2]},${value[3]})))) {
        vec2 local = (v_uv - vec2(${value[0]},${value[1]})) / vec2(${(rect[2] - rect[0]).toFixed(9)},${(rect[3] - rect[1]).toFixed(9)});
        ${motion}
      }`;
    })
    .join("\n");
  return `vec4 materialFragment() {
    vec3 base = texture(p_base, v_uv).rgb;
    // Keep peaks at the authored exposure. Motion comes from attenuation,
    // with spatial sweeps on instruments rather than flashing the whole cabin.
    vec3 cabinGain = p_cabinTint * (0.91 + 0.09 * cos(p_cabinPhase + v_uv.x));
    vec3 dashboardGain = p_dashboardTint * (0.82 + 0.18 * cos(p_dashboardPhase));
    vec3 cyanGain = p_buttonsTint;
    ${rectangles}
    vec3 emission = texture(p_cabin,v_uv).rgb * cabinGain
      + texture(p_dashboard,v_uv).rgb * dashboardGain
      + texture(p_cyan,v_uv).rgb * cyanGain;
    return vec4(base + emission * p_illumination, 1.0);
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
        <FragmentShader requiredAttributes={2}>{shader}</FragmentShader>
      </ShaderAsset>
      <AnimationAsset id="cockpit-light-mask-motion" clip={clip} />
      {runtime.lighting.bodies.map((body) => (
        <Entity key={body.entity} bindTo={body.entity}>
          <CustomMaterial
            source={assetRef("cockpit-light-mask-shader")}
            alpha_mode={0}
            base={texture(body.baseTexture)}
            cabin={texture(runtime.lighting.textures.cabin)}
            dashboard={texture(runtime.lighting.textures.dashboard)}
            cyan={texture(runtime.lighting.textures.cyan)}
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
