import {
  Asset,
  Entity,
  Transform,
  Camera,
  MeshInstance,
  CustomMaterial,
  ShaderAsset,
  PaintShader,
  VertexShader,
  FragmentShader,
  assetRef,
} from "@ipp/react";
import { Box, Paint, Style, Text } from "@ipp/react/gui";
import { glassColor } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import {
  BAR_PAINT,
  BAR_VERTEX,
  BAR_FRAGMENT,
  BACK_FRAGMENT,
  PANEL_PAINT,
  PARAMETERS,
} from "./material.js";
import { FLOOR_FRAGMENT } from "./reflections.js";

export const STUDY = {
  width: 400,
  height: 500,
  viewY: 90,
  viewHeight: 380,
  ortho: 4.4,
};
export const EXTENT = { width: 800, height: 1000, devicePixelRatio: 2 };
const CUBE = "ipp://mesh/cube?width=1&height=1&length=1";
const EYE = [5, 4.3, 7] as const,
  TARGET = [0, 1.1, 0] as const;
export interface Finish {
  amount: number;
  light: number;
  contrast: boolean;
  violet: boolean;
}
export const BASE: Finish = {
  amount: 0,
  light: 0,
  contrast: false,
  violet: false,
};
const a = (o: Finish) => glassColor(o.violet ? "#BF9CFF" : "#00D9FF");
const t = (o: Finish) => glassColor(o.violet ? "#9F58E8" : "#00ACDB");
const light = (o: Finish, spatial = false) =>
  spatial
    ? [-4 + o.light * 11, 8 - o.light * 4, 6 - o.light * 8, 1]
    : [-0.3 + o.light * 2.1, -0.2 + o.light * 1.4, 0.75, 1];
function pose() {
  const dx = EYE[0] - TARGET[0],
    dy = EYE[1] - TARGET[1],
    dz = EYE[2] - TARGET[2],
    length = Math.hypot(dx, dy, dz),
    yaw = Math.atan2(dx, dz) / 2,
    pitch = -Math.asin(dy / length) / 2;
  return {
    x: EYE[0],
    y: EYE[1],
    z: EYE[2],
    qx: Math.cos(yaw) * Math.sin(pitch),
    qy: Math.sin(yaw) * Math.cos(pitch),
    qz: -Math.sin(yaw) * Math.sin(pitch),
    qw: Math.cos(yaw) * Math.cos(pitch),
  };
}
function text(
  id: string,
  x: number,
  y: number,
  value: string,
  size: number,
  color = "#B9E7FA",
) {
  const c = glassColor(color);
  return (
    <Entity id={id}>
      <Style x={x} y={y} layer={2} red={c[0]} green={c[1]} blue={c[2]} />
      <Text source={assetRef("font")} text={value} font_size={size} />
    </Entity>
  );
}
export function reflectionSheet(
  font: Uint8Array<ArrayBuffer>,
  dimension: 2 | 3,
  o: Finish,
) {
  const bg = glassColor(o.contrast ? "#A94D7B" : "#00111D");
  return (
    <>
      <Asset id="font" kind={17} data={font} encode={(v) => v} />
      <ShaderAsset id="bar-paint" recipe={{}} parameters={PARAMETERS}>
        <PaintShader>{BAR_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset id="panel" recipe={{}} parameters={PARAMETERS}>
        <PaintShader>{PANEL_PAINT}</PaintShader>
      </ShaderAsset>
      <Entity id="background">
        <Style red={bg[0]} green={bg[1]} blue={bg[2]} />
        <Box width={STUDY.width} height={STUDY.height} />
      </Entity>
      {text(
        "heading",
        32,
        24,
        dimension === 2
          ? "2D BAR"
          : o.amount > 0
            ? "3D / STYLIZED GLASS"
            : "3D / CURRENT",
        23,
      )}
      {dimension === 2
        ? text(
            "finish",
            32,
            57,
            o.amount > 0 ? "STYLIZED GLASS" : "CURRENT MATERIAL",
            16,
            "#B9E7FA",
          )
        : null}
      {dimension === 2 ? (
        <>
          <Entity id="bar">
            <Style x={147} y={104} />
            <Box width={106} height={296} />
            <Paint
              source={assetRef("bar-paint")}
              tint={t(o)}
              accent={a(o)}
              light={light(o)}
              profile={[1, 8, o.amount, 0.5]}
            />
          </Entity>
          <Entity id="baseline">
            <Style x={80} y={392} red={0.027} green={0.096} blue={0.155} />
            <Box width={240} height={0.65} />
          </Entity>
        </>
      ) : null}
      {dimension === 2
        ? text(
            "footnote",
            32,
            466,
            o.light === 0
              ? "LIGHT / LEFT"
              : o.light === 1
                ? "LIGHT / RIGHT"
                : "LIGHT / MOVING",
            14,
            "#B9E7FA",
          )
        : null}
    </>
  );
}
export function reflectionWorld(o: Finish) {
  return (
    <>
      <ShaderAsset
        id="bar-mesh"
        recipe={{ normals: true }}
        parameters={{ ...PARAMETERS, dimensions: "vec4", eye: "vec4" }}
      >
        <VertexShader>{BAR_VERTEX}</VertexShader>
        <FragmentShader>{BAR_FRAGMENT}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="floor"
        recipe={{}}
        parameters={{ accent: "vec4", profile: "vec4", light: "vec4" }}
      >
        <FragmentShader>{FLOOR_FRAGMENT}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="backdrop-paint"
        recipe={{}}
        parameters={{ tint: "vec4" }}
      >
        <FragmentShader>{BACK_FRAGMENT}</FragmentShader>
      </ShaderAsset>
      <Entity id="camera">
        <Transform {...pose()} />
        <Camera
          projection={1}
          ortho_height={STUDY.ortho}
          near={0.1}
          far={200}
        />
      </Entity>
      <Entity id="backdrop">
        <Transform z={-35} sx={150} sy={150} sz={0.01} />
        <MeshInstance source={CUBE} />
        <CustomMaterial
          source={assetRef("backdrop-paint")}
          tint={glassColor(o.contrast ? "#A94D7B" : "#00111D")}
          alpha_mode={0}
        />
      </Entity>
      <Entity id="bar">
        <Transform y={1.35} sx={1.4} sy={2.7} sz={1.25} />
        <MeshInstance source={CUBE} />
        <CustomMaterial
          source={assetRef("bar-mesh")}
          tint={t(o)}
          accent={a(o)}
          light={light(o, true)}
          profile={[1, 0, o.amount, 0.5]}
          dimensions={[1.4, 2.7, 1.25, STUDY.viewHeight / STUDY.ortho]}
          eye={[...EYE, 1]}
          alpha_mode={0}
        />
      </Entity>
      {o.amount > 0 ? (
        <Entity id="floor-sheen">
          <Transform y={-0.008} sx={5} sy={0.001} sz={5} />
          <MeshInstance source={CUBE} />
          <CustomMaterial
            source={assetRef("floor")}
            accent={a(o)}
            profile={[0, 0, o.amount, 0]}
            light={light(o, true)}
            alpha_mode={2}
          />
        </Entity>
      ) : null}
    </>
  );
}
