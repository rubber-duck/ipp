/** Four fixed sphere sections share registered, independently controlled lamps. */
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
import FRAGMENT from "./bridge.glsl";

const SHADER = lightTextureSampling(2) + FRAGMENT;

export function CockpitBridge({
  runtime,
  component,
  state,
}: {
  runtime: CockpitRuntime;
  component: number;
  state: CockpitOptions;
}) {
  const bodies = runtime.exterior.bodies.filter(
    (body) => body.role === "bridge",
  );
  const clip = useMemo<AnimationClipSource>(
    () => ({
      duration: 12,
      tracks: [
        {
          property: { component, name: "cabinPhase" },
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
                value: { kind: "f32", value: Math.PI * 2 },
              },
            },
          ],
        },
      ],
    }),
    [component],
  );
  return (
    <>
      <ShaderAsset
        id="cockpit-bridge-shader"
        recipe={{}}
        parameters={{
          ...lightTextureParameters(2),
          illumination: "f32",
          cabinPhase: "f32",
          cabinTint: "vec3",
        }}
      >
        <FragmentShader backend="ipp" requiredAttributes={2}>
          {SHADER}
        </FragmentShader>
      </ShaderAsset>
      <AnimationAsset id="cockpit-cabin-motion" clip={clip} />
      {bodies.map((body) => (
        <Entity key={body.entity} bindTo={body.entity}>
          <CustomMaterial
            source={assetRef("cockpit-bridge-shader")}
            alpha_mode={1}
            {...lightTextureProperties(body.tiles!)}
            illumination={state.illumination}
            cabinPhase={0}
            cabinTint={[1, 0.86, 0.68]}
            receives_light={false}
            receives_shadows={false}
            casts_shadows={false}
          />
          <Animation
            source={assetRef("cockpit-cabin-motion")}
            looping
            playing={state.lightsPlaying}
          />
        </Entity>
      ))}
    </>
  );
}
