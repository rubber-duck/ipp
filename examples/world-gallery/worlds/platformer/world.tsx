import {
  CustomMaterial,
  Entity,
  FragmentShader,
  ShaderAsset,
  assetRef,
} from "@ipp/react";
import { World } from "@ipp/react/web";
import ORB_SHADER from "./orb.glsl";

/** React adds runtime choreography to the Blender-authored hierarchy. */
export function PlatformerWorld({ onCommit }: { onCommit: () => void }) {
  return (
    <World onCommit={onCommit}>
      <ShaderAsset
        id="platformer-orb-shader"
        recipe={{ normals: true, lighting: true }}
        parameters={{ time: "f32", color: "vec4" }}
      >
        <FragmentShader>{ORB_SHADER}</FragmentShader>
      </ShaderAsset>
      <Entity bindTo="platformer-root" />
      <Entity bindTo="platformer-rig" />
      <Entity bindTo="platformer-character" />
      <Entity bindTo="platformer-camera-target" />
      <Entity bindTo="platformer-camera" />
      <Entity bindTo="platformer-overhead-light" />
      <Entity bindTo="platformer-orb">
        <CustomMaterial
          source={assetRef("platformer-orb-shader")}
          time={0}
          color={[0.22, 0.72, 1, 1]}
          alpha_mode={2}
          receives_light
          receives_shadows={false}
          casts_shadows={false}
        />
      </Entity>
    </World>
  );
}
