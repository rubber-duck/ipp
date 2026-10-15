/** The silhouette stays fixed; Host clips vary only the registered light masks. */
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
      duration: 12,
      tracks: ["phase", "cityPhase", "starPhase"].map((name, index) => ({
        property: { component, name },
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
              value: { kind: "f32", value: Math.PI * 2 * (index + 1) },
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
        recipe={{}}
        parameters={{
          base: "texture2D",
          cities: "texture2D",
          atmosphere: "texture2D",
          stars: "texture2D",
          phase: "f32",
          cityPhase: "f32",
          starPhase: "f32",
          intensity: "f32",
          cityGain: "f32",
          starGain: "f32",
          imageSize: "vec2",
        }}
      >
        <FragmentShader requiredAttributes={2}>{FRAGMENT}</FragmentShader>
      </ShaderAsset>
      <AnimationAsset id="cockpit-exterior-motion" clip={clip} />
      {runtime.exterior.bodies.map((body) => (
        <Entity key={body.entity} bindTo={body.entity}>
          <CustomMaterial
            source={assetRef("cockpit-exterior-shader")}
            alpha_mode={0}
            base={texture(body.baseTexture)}
            cities={texture(runtime.exterior.textures.cities)}
            atmosphere={texture(runtime.exterior.textures.atmosphere)}
            stars={texture(runtime.exterior.textures.stars)}
            imageSize={[runtime.reference.width, runtime.reference.height]}
            phase={0}
            cityPhase={0}
            starPhase={0}
            intensity={state.atmosphere}
            cityGain={state.cities}
            starGain={state.stars}
            receives_light={false}
            receives_shadows={false}
            casts_shadows={false}
          />
          <Animation
            source={assetRef("cockpit-exterior-motion")}
            looping
            playing={state.atmospherePlaying}
          />
        </Entity>
      ))}
    </>
  );
}
