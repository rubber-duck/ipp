import {
  Animation,
  AnimationAsset,
  Asset,
  AttachedWorld,
  Camera,
  CylinderSurface,
  Entity,
  FlatSurface,
  Light,
  ShaderAsset,
  PaintShader,
  SphereSurface,
  SurfaceCache,
  Transform,
  assetRef,
} from "@ipp/react";
import { Box, Paint, Style, Text } from "@ipp/react/gui";
import { OPTICAL_PARAMETERS, OPTICAL_PRESENTED_PAINT } from "./material.js";

export const CONTEXT_SYSTEMS = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.render",
  "ipp.camera",
  "ipp.surface",
  "ipp.world-attachment",
  "ipp.lifecycle-publisher",
] as const;
export interface ContextOptions {
  shape?: "flat" | "cylinder" | "sphere";
  angle?: number;
  light?: number;
  intensity?: number;
  probe?: number;
  cache?: boolean;
  layer?: number;
  spacing?: number;
  motion?: { component: number; offset: number };
  /** Logical canvas extent for the component sheet; density remains 200. */
  extent?: readonly [number, number];
}

export function contextScene(
  child: import("@ipp/client").WorldReference,
  options: ContextOptions = {},
) {
  const angle = options.angle ?? 0;
  const surface = {
    width: options.extent ? options.extent[0] / 200 : 5.5,
    height: options.extent ? options.extent[1] / 200 : 2.4,
    layer_spacing: options.spacing ?? 0,
  };
  return (
    <>
      {options.motion && (
        <AnimationAsset
          id="light-motion"
          clip={{
            duration: 8,
            tracks: [
              {
                property: {
                  component: options.motion.component,
                  offsets: [options.motion.offset],
                },
                keys: Array.from({ length: 33 }, (_, i) => ({
                  time: i / 4,
                  value: {
                    kind: "f32" as const,
                    value: 3 - 3 * Math.cos((i * Math.PI) / 16),
                  },
                })),
              },
            ],
          }}
        />
      )}
      <Entity id="camera">
        <Transform
          x={Math.sin(angle) * 8}
          z={Math.cos(angle) * 8}
          qy={Math.sin(angle / 2)}
          qw={Math.cos(angle / 2)}
        />
        <Camera
          projection={1}
          ortho_height={options.extent ? options.extent[1] / 200 : 4.0}
          near={0.1}
          far={50}
        />
      </Entity>
      <Entity id="light">
        <Transform x={options.light ?? -3} y={3} z={5} />
        {options.motion && (
          <Animation source={assetRef("light-motion")} looping playing />
        )}
        <Light
          kind={1}
          r={1}
          g={1}
          b={1}
          intensity={options.intensity ?? 50}
          range={30}
        />
      </Entity>
      <Entity id="surface">
        {options.shape === "cylinder" ? (
          <CylinderSurface {...surface} curvature={0.35} />
        ) : options.shape === "sphere" ? (
          <SphereSurface {...surface} curvature={0.35} />
        ) : (
          <FlatSurface {...surface} />
        )}
        {options.cache && (
          <SurfaceCache direct_distance={0} max_refresh_hz={0.2} />
        )}
      </Entity>
      <AttachedWorld
        anchor="surface"
        child={{ borrow: child }}
        attachment={{ mode: "surface-canvas" }}
      />
    </>
  );
}

export function contextSamples(
  font: Uint8Array<ArrayBuffer>,
  options: ContextOptions = {},
) {
  return (
    <>
      <Asset id="font" kind={17} data={font} encode={(v) => v} />
      <ShaderAsset
        id="optical"
        recipe={{ presentation: true, lighting: true }}
        parameters={OPTICAL_PARAMETERS}
      >
        <PaintShader>{OPTICAL_PRESENTED_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset id="backdrop" recipe={{}} parameters={{}}>
        <PaintShader>{`
          vec2 p=position/size;
          float pool=exp(-dot((p-vec2(.35,.48))*vec2(1.8,3.2),(p-vec2(.35,.48))*vec2(1.8,3.2)));
          float ribbon=exp(-pow((p.x+p.y*.38-.7)/.025,2.0));
          return vec4(vec3(.002,.005,.008)+vec3(.015,.027,.030)*pool+vec3(.022,.032,.034)*ribbon,1);
        `}</PaintShader>
      </ShaderAsset>
      <Entity id="background">
        <Style red={0.0018} green={0.0065} blue={0.012} />
        <Box width={1100} height={480} />
        <Paint source={assetRef("backdrop")} />
      </Entity>
      <Entity id="title">
        <Style x={70} y={50} layer={1} red={0.52} green={0.84} blue={0.94} />
        <Text
          source={assetRef("font")}
          text="GLASS / LIVE SURFACE OPTICS"
          font_size={30}
        />
      </Entity>
      {[0.12, 0.48, 0.17].map((roughness, i) => (
        <Entity key={i} id={`button-${i}`}>
          <Style x={70 + i * 330} y={190} layer={options.layer ?? 0} />
          <Box width={300} height={90} />
          <Paint
            source={assetRef("optical")}
            accent={[0, 0.79, 1, 1]}
            tint={[0.22, 0.64, 0.82, 1]}
            optics={[1.5, roughness, i === 2 ? 2.5 : 0.8, i === 2 ? 1.15 : 1.0]}
            keyLight={[-3, -2.5, 5, 0.8]}
            eye={[0, 0, 8, 0]}
            state={[0.1, 1, options.probe ?? 0, 0]}
          />
        </Entity>
      ))}
      {!options.probe &&
        ["CLEAR", "FROSTED", "SMOKED"].map((label, i) => (
          <Entity key={label} id={`label-${i}`}>
            <Style
              x={165 + i * 330}
              y={222}
              layer={1}
              red={0.66}
              green={0.8}
              blue={0.85}
            />
            <Text source={assetRef("font")} text={label} font_size={26} />
          </Entity>
        ))}
      <Entity id="subtitle">
        <Style x={70} y={345} layer={1} red={0.18} green={0.32} blue={0.42} />
        <Text
          source={assetRef("font")}
          text="Camera and scene light supplied by the renderer"
          font_size={21}
        />
      </Entity>
    </>
  );
}
