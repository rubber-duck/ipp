/** Independent bridge cubemap and rotating planetary surfaces, driven by Host clips. */
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
import FRAGMENT from "./atmosphere.glsl";

const texture = (source: string) =>
  ({
    kind: "asset",
    value: { kind: 2, source: textureSource(source) },
  }) as const;

export function CockpitExterior({
  runtime,
  component,
  state,
}: {
  runtime: CockpitRuntime;
  component: number;
  state: CockpitOptions;
}) {
  const clip = useMemo<AnimationClipSource>(
    () => ({
      // Ten terrain turns and one relative weather turn meet at the same loop
      // boundary; clouds must not snap back when the planet completes one turn.
      duration: 18000,
      tracks: ["phase", "cloudPhase", "cityPhase"].map((name, index) => ({
        property: { component, name },
        keys: [
          {
            time: 0,
            value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
            interpolation: { kind: "linear" },
          },
          {
            time: 18000,
            value: {
              kind: "dynamic",
              value: {
                kind: "f32",
                value: Math.PI * 2 * [10, 1, 1800][index]!,
              },
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
        id="cockpit-exterior-shader"
        recipe={{ normals: true, lighting: true }}
        parameters={{
          base: "texture2D",
          cities: "texture2D",
          phase: "f32",
          cloudPhase: "f32",
          cityPhase: "f32",
          intensity: "f32",
          cityGain: "f32",
          starGain: "f32",
          illumination: "f32",
          role: "f32",
        }}
      >
        <FragmentShader backend="ipp" requiredAttributes={6}>
          {FRAGMENT}
        </FragmentShader>
      </ShaderAsset>
      <AnimationAsset id="cockpit-exterior-motion" clip={clip} />
      {runtime.exterior.bodies.map((body) => (
        <Entity key={body.entity} bindTo={body.entity}>
          <CustomMaterial
            source={assetRef("cockpit-exterior-shader")}
            alpha_mode={
              body.role === "atmosphere" ? 2 : body.role === "bridge" ? 1 : 0
            }
            base={texture(body.baseTexture)}
            cities={texture(runtime.exterior.textures.cities)}
            role={[
              "planet",
              "atmosphere",
              "stars",
              "bridge",
              "trim",
              "starlight",
            ].indexOf(body.role)}
            illumination={state.illumination}
            phase={0}
            cloudPhase={0}
            cityPhase={0}
            intensity={state.atmosphere}
            cityGain={state.cities}
            starGain={state.stars}
            receives_light
            receives_shadows={false}
            casts_shadows={false}
          />
          {(body.role === "planet" || body.role === "atmosphere") && (
            <Animation
              source={assetRef("cockpit-exterior-motion")}
              looping
              playing={state.atmospherePlaying}
            />
          )}
        </Entity>
      ))}
    </>
  );
}
