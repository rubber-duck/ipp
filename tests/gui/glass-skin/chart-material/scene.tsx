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
  UnlitMaterial,
  FlatSurface,
  AttachedWorld,
} from "@ipp/react";
import type { CameraOutputReference, WorldReference } from "@ipp/client";
import { Box, Paint, Style, Text } from "@ipp/react/gui";
import { glassColor } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import {
  BAR_PAINT,
  PANEL_PAINT,
  LINE_PAINT,
  BAR_VERTEX,
  BAR_FRAGMENT,
  HALO_FRAGMENT,
  BACK_FRAGMENT,
  PARAMETERS,
} from "./material.js";
import {
  VALUES,
  SHEET,
  PLOT,
  VIEW,
  FRAME,
  EYE,
  ORTHO,
  PIXELS_PER_UNIT,
  pose,
  project,
  bar2d,
  bar3d,
  halo3d,
  CALLOUT2,
  callout3d,
  type Point,
  type V3,
} from "./mapping.js";
export { VALUES, EXTENT } from "./mapping.js";
export interface Options {
  dimension: 2 | 3;
  right: boolean;
  contrast: boolean;
  violet: boolean;
}
const CUBE = "ipp://mesh/cube?width=1&height=1&length=1";
const textColor = glassColor("#B9E7FA"),
  gridColor = glassColor("#244C61");
const accent = (o: Options) => glassColor(o.violet ? "#BF9CFF" : "#00D9FF");
const tint = (i: number, o: Options) =>
  glassColor(
    i === 1
      ? o.violet
        ? "#9F58E8"
        : "#00ACDB"
      : (o.violet
          ? ["#37356E", "#6946A5", "#293665"]
          : ["#005D84", "#12669A", "#164A80", "#176AA5", "#006887", "#154F87"])[
          i % (o.violet ? 3 : 6)
        ]!,
  );
const light = (o: Options) =>
  o.dimension === 2
    ? [o.right ? 1.8 : -0.3, o.right ? 1.2 : -0.2, 0.75, 1]
    : [o.right ? 15 : -4, o.right ? 4 : 12, o.right ? -3 : 10, 1];
function shaders() {
  return (
    <>
      <ShaderAsset id="bar-paint" recipe={{}} parameters={PARAMETERS}>
        <PaintShader>{BAR_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset id="panel" recipe={{}} parameters={PARAMETERS}>
        <PaintShader>{PANEL_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset id="line" recipe={{}} parameters={PARAMETERS}>
        <PaintShader>{LINE_PAINT}</PaintShader>
      </ShaderAsset>
    </>
  );
}
function label(id: string, x: number, y: number, text: string, fontSize = 15) {
  return (
    <Entity key={id} id={id}>
      <Style
        x={x}
        y={y}
        red={textColor[0]}
        green={textColor[1]}
        blue={textColor[2]}
      />
      <Text text={text} font_size={fontSize * 1.2} source={assetRef("font")} />
    </Entity>
  );
}
function plate(
  id: string,
  x: number,
  y: number,
  width: number,
  height: number,
  o: Options,
  cut = 18,
  callout = false,
  ornament = false,
) {
  const pad = 6;
  return (
    <Entity key={id} id={id}>
      <Style x={x - pad} y={y - pad} />
      <Box width={width + pad * 2} height={height + pad * 2} />
      <Paint
        source={assetRef("panel")}
        tint={glassColor(o.contrast && !callout ? "#A94D7B" : "#00111D")}
        accent={accent(o)}
        light={light(o)}
        profile={[ornament ? 1 : 0, cut, callout ? 1 : 0, pad]}
      />
    </Entity>
  );
}
function line(
  id: string,
  a: Point,
  b: Point,
  color: readonly number[] = textColor,
  width = 0.65,
) {
  const x = Math.min(a[0], b[0]) - 2,
    y = Math.min(a[1], b[1]) - 2;
  return (
    <Entity key={id} id={id}>
      <Style x={x} y={y} />
      <Box
        width={Math.abs(a[0] - b[0]) + 4}
        height={Math.abs(a[1] - b[1]) + 4}
      />
      <Paint
        source={assetRef("line")}
        tint={color}
        accent={color}
        light={[a[0] - x, a[1] - y, b[0] - x, b[1] - y]}
        profile={[width, 0, 0, 0]}
      />
    </Entity>
  );
}
function marker(id: string, p: Point, o: Options) {
  return (
    <Entity key={id} id={id}>
      <Style x={p[0] - 6} y={p[1] - 6} />
      <Box width={12} height={12} />
      <Paint
        source={assetRef("panel")}
        tint={textColor}
        accent={accent(o)}
        light={light(o)}
        profile={[0, 0, 2, 0]}
      />
    </Entity>
  );
}
function annotation(o: Options) {
  const b = bar2d(1),
    a: Point =
      o.dimension === 2 ? [b.x + b.width / 2, b.y] : callout3d().anchor;
  const box = o.dimension === 2 ? CALLOUT2 : callout3d().box;
  const end: Point = [box.x, box.y + box.height * 0.5],
    elbow: Point = [end[0] - (o.dimension === 2 ? 7 : 14), end[1]];
  return (
    <>
      {line("leader-slope", a, elbow, textColor, 0.65)}
      {line("leader-end", elbow, end, textColor, 0.65)}
      {plate("callout", box.x, box.y, box.width, box.height, o, box.cut, true)}
      {label("callout-text", box.x + 13, box.y + 9, "Sample 2 / 64", 15)}
      {marker("selected-marker", a, o)}
    </>
  );
}
function axes2d() {
  return (
    <>
      {Array.from({ length: 6 }, (_, i) => {
        const y = PLOT.y + PLOT.height - (i * PLOT.height) / 5;
        return (
          <Entity id={`group-y-${i}`} key={`y-${i}`}>
            {line(
              `grid-y-${i}`,
              [PLOT.x, y],
              [PLOT.x + PLOT.width, y],
              gridColor,
              0.45,
            )}
            {label(`tick-y-${i}`, PLOT.x - 32, y - 7, String(i * 20), 14)}
          </Entity>
        );
      })}
      {Array.from({ length: 6 }, (_, i) => {
        const x = PLOT.x + ((i + 0.5) * PLOT.width) / 6;
        return (
          <Entity id={`group-x-${i}`} key={`x-${i}`}>
            {line(
              `grid-x-${i}`,
              [x, PLOT.y],
              [x, PLOT.y + PLOT.height],
              gridColor,
              0.45,
            )}
            {label(
              `tick-x-${i}`,
              x - 4,
              PLOT.y + PLOT.height + 12,
              String(i),
              14,
            )}
          </Entity>
        );
      })}
      {line(
        "boundary-right",
        [PLOT.x + PLOT.width, PLOT.y],
        [PLOT.x + PLOT.width, PLOT.y + PLOT.height],
        gridColor,
        0.45,
      )}
      {line(
        "axis-y",
        [PLOT.x, PLOT.y],
        [PLOT.x, PLOT.y + PLOT.height],
        textColor,
        0.5,
      )}
      {line(
        "axis-x",
        [PLOT.x, PLOT.y + PLOT.height],
        [PLOT.x + PLOT.width, PLOT.y + PLOT.height],
        textColor,
        0.5,
      )}
      {label("y-title", PLOT.x, PLOT.y - 23, "VALUE", 14)}
      {label("x-title", PLOT.x + PLOT.width * 0.5 - 26, 431, "SAMPLE", 14)}
    </>
  );
}
function labels3d() {
  return (
    <>
      {Array.from({ length: 5 }, (_, i) => {
        const p = project([0, (i * FRAME.height) / 4, FRAME.depth]);
        return label(`tick-y-${i}`, p[0] - 32, p[1] - 7, String(i * 25), 13);
      })}
      {Array.from({ length: 4 }, (_, i) => {
        const p = project([((i + 0.5) * FRAME.width) / 4, 0, FRAME.depth]);
        return label(`tick-x-${i}`, p[0] - 4, p[1] + 9, String(i), 13);
      })}
      {Array.from({ length: 3 }, (_, i) => {
        const p = project([0, FRAME.height, ((i + 0.5) * FRAME.depth) / 3]);
        return label(`tick-z-${i}`, p[0] - 18, p[1] - 8, String(i), 13);
      })}
      {(() => {
        const p = project([0, FRAME.height, FRAME.depth]);
        return label("y-title", p[0] - 57, p[1] - 24, "VALUE", 13);
      })()}
      {(() => {
        const p = project([FRAME.width * 0.5, 0, FRAME.depth]);
        return label("x-title", p[0] - 3, p[1] + 30, "X", 14);
      })()}
      {(() => {
        const p = project([0, FRAME.height, FRAME.depth * 0.5]);
        return label("z-title", p[0] - 37, p[1] - 8, "Z", 14);
      })()}
    </>
  );
}
/** Both chart panels are authored Canvas; the 3D chart is a real attached Camera output. */
export function chartSheet(
  font: Uint8Array<ArrayBuffer>,
  o: Options,
  camera?: { world: WorldReference; output: CameraOutputReference },
) {
  return (
    <>
      <Asset id="font" kind={17} data={font} encode={(v) => v} />
      {shaders()}
      <Entity id="background">
        <Style red={0} green={0.004} blue={0.009} />
        <Box width={SHEET.width} height={SHEET.height} />
      </Entity>
      {plate("panel", 8, 8, 724, 444, o)}
      {o.dimension === 3 && camera ? (
        <Entity id="camera-view">
          <Style x={VIEW.x} y={VIEW.y} />
          <FlatSurface width={VIEW.width / 96} height={VIEW.height / 96} />
          <AttachedWorld
            anchor="camera-view"
            child={{ borrow: camera.world }}
            attachment={{ mode: "surface-camera", output: camera.output }}
          />
        </Entity>
      ) : null}
      {o.dimension === 3
        ? plate("panel-ornaments", 8, 8, 724, 444, o, 18, false, true)
        : null}
      {label(
        "heading",
        38,
        24,
        o.dimension === 2 ? "BARS" : "3D GRID BARS",
        19,
      )}
      {o.dimension === 2 ? (
        <>
          {axes2d()}
          {VALUES.map((_, i) => {
            const b = bar2d(i),
              pad = 8;
            return (
              <Entity key={i} id={`bar-${i}`}>
                <Style x={b.x - pad} y={b.y - pad} />
                <Box width={b.width + pad * 2} height={b.height + pad * 2} />
                <Paint
                  source={assetRef("bar-paint")}
                  tint={tint(i, o)}
                  accent={accent(o)}
                  light={light(o)}
                  profile={[i === 1 ? 1 : 0, pad, 0, 0]}
                />
              </Entity>
            );
          })}
        </>
      ) : (
        labels3d()
      )}
      {annotation(o)}
    </>
  );
}
function beam(
  id: string,
  a: V3,
  b: V3,
  color: readonly number[],
  width = 0.009,
) {
  return (
    <Entity id={id} key={id}>
      <Transform
        x={(a[0] + b[0]) / 2}
        y={(a[1] + b[1]) / 2}
        z={(a[2] + b[2]) / 2}
        sx={Math.max(width, Math.abs(a[0] - b[0]))}
        sy={Math.max(width, Math.abs(a[1] - b[1]))}
        sz={Math.max(width, Math.abs(a[2] - b[2]))}
      />
      <MeshInstance source={CUBE} />
      <UnlitMaterial r={color[0]} g={color[1]} b={color[2]} />
    </Entity>
  );
}
function grid3d() {
  const { width: w, height: h, depth: d } = FRAME;
  return (
    <>
      {Array.from({ length: 5 }, (_, i) => {
        const x = (i * w) / 4;
        return (
          <Entity id={`group-gx${i}`} key={`gx${i}`}>
            {beam(`floor-x${i}`, [x, -0.02, 0], [x, -0.02, d], gridColor)}
            {beam(`back-x${i}`, [x, 0, 0], [x, h, 0], gridColor)}
          </Entity>
        );
      })}
      {Array.from({ length: 4 }, (_, i) => {
        const z = (i * d) / 3;
        return (
          <Entity id={`group-gz${i}`} key={`gz${i}`}>
            {beam(`floor-z${i}`, [0, -0.02, z], [w, -0.02, z], gridColor)}
            {beam(`back-z${i}`, [0, 0, z], [0, h, z], gridColor)}
          </Entity>
        );
      })}
      {Array.from({ length: 5 }, (_, i) => {
        const y = (i * h) / 4;
        return (
          <Entity id={`group-gy${i}`} key={`gy${i}`}>
            {beam(`wall-x${i}`, [0, y, 0], [w, y, 0], gridColor)}
            {beam(`wall-z${i}`, [0, y, 0], [0, y, d], gridColor)}
          </Entity>
        );
      })}
      {beam("axis-x", [0, 0, d], [w, 0, d], textColor, 0.012)}
      {beam("axis-y", [0, 0, d], [0, h, d], textColor, 0.012)}
      {beam("axis-z", [0, h, 0], [0, h, d], textColor, 0.012)}
    </>
  );
}
export function chartWorld(o: Options) {
  const a = accent(o);
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
        id="halo"
        recipe={{}}
        parameters={{
          accent: "vec4",
          profile: "vec4",
          dimensions: "vec4",
          h0: "vec4",
          h1: "vec4",
          h2: "vec4",
          h3: "vec4",
          h4: "vec4",
          h5: "vec4",
        }}
      >
        <VertexShader>{BAR_VERTEX}</VertexShader>
        <FragmentShader>{HALO_FRAGMENT}</FragmentShader>
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
        <Camera projection={1} ortho_height={ORTHO} near={0.1} far={200} />
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
      {grid3d()}
      {Array.from({ length: 12 }, (_, i) => {
        const b = bar3d(i),
          halo = halo3d(i);
        return (
          <Entity id={`group-bar-${i}`} key={i}>
            <Entity id={`bar-${i}`}>
              <Transform
                x={b.x}
                y={b.y}
                z={b.z}
                sx={b.width}
                sy={b.height}
                sz={b.depth}
              />
              <MeshInstance source={CUBE} />
              <CustomMaterial
                source={assetRef("bar-mesh")}
                tint={tint(i, o)}
                accent={a}
                light={light(o)}
                profile={[i === 1 ? 1 : 0, 0, 0, 0]}
                dimensions={[b.width, b.height, b.depth, PIXELS_PER_UNIT]}
                eye={[...EYE, 1]}
                alpha_mode={0}
              />
            </Entity>
            <Entity id={`halo-${i}`}>
              <Transform {...halo.transform} />
              <MeshInstance source={CUBE} />
              <CustomMaterial
                source={assetRef("halo")}
                accent={a}
                profile={[i === 1 ? 1 : 0, 0, 0, 0]}
                dimensions={[halo.width, halo.height, 0, 0]}
                {...Object.fromEntries(halo.edges.map((e, j) => [`h${j}`, e]))}
                alpha_mode={2}
              />
            </Entity>
          </Entity>
        );
      })}
    </>
  );
}
