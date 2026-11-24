import { assetRef, ShaderAsset } from "../assets/declarations.js";
import { FragmentShader, PaintShader, VertexShader } from "../shaders.js";
import type { PlotSkinAppearance } from "../plots/skin-types.js";
import { pencilAxisAsset } from "./pencil-axis.js";
import { cartoonPaperAssets } from "./chart-paper.js";
import { cartoonTokens, cream, grass, ink } from "./palette.js";

// A little warm light lifts the native series pigment. Legend keys use the
// same mix, so colour scales and arbitrary authored series keep their meaning.
const pigment = (source: string) => `
vec3 face = ${source};
face.g += .30 * max(face.r - face.b, 0.) * (1. - face.g);
face = mix(face, p_cream.rgb, .06);`;

const mark = `
${pigment("color.rgb")}
float depth = max(-edge, 0.0);
float aa = max(fwidth(edge), 0.15);
float span = min(size.x, size.y);
float border = clamp(span * .026, .35, 1.05);
float rim = smoothstep(border - aa * .5, border + aa * .5, depth);
// The dark foot follows the band's real lower contour on a curved or leaning mark.
float lower = smoothstep(.62, .92, position.y / max(size.y, .001));
float foot = lower * (1.0 - smoothstep(border + 1.0, border + 3.5, depth));
face = mix(face, face * .72, foot);
return vec4(mix(mix(face, p_ink.rgb, .46), face, rim), color.a);
`;

// Each box is one native block. Size the rails from its width so a fitted cap
// keeps the same joinery as the full crates beneath it.
const crate = `
${pigment("color.rgb")}
vec2 p = position / max(size.x, .001);
float h = size.y / max(size.x, .001);
vec2 aa = max(fwidth(p) * .5, vec2(.0001));
float rail = 1. / 6.;
vec2 inset = min(p, vec2(1., h) - p);
float fullPanel = smoothstep(2. * rail + .01, 2. * rail + .09, h);
vec2 panelMask = smoothstep(vec2(rail) - aa, vec2(rail) + aa, inset);
float panel = panelMask.x * panelMask.y * fullPanel;

// Three upright boards sit behind the two posts. A shallow lip catches the
// light on the upper edge of each rail and casts a shadow onto the boards.
float first = .36 + .018 * (p.y / max(h,.001) - .5);
float second = .66 - .012 * (p.y / max(h,.001) - .5);
float joint = min(abs(p.x-first), abs(p.x-second));
float groove = 1. - smoothstep(.009-aa.x, .009+aa.x, joint);
float tone = .77 + .07 * smoothstep(first-aa.x,first+aa.x,p.x)
                 - .10 * smoothstep(second-aa.x,second+aa.x,p.x);
float recess = smoothstep(rail, rail+.06, min(p.x, p.y));
vec3 boards = face * tone * mix(.79,1.,recess) * mix(1.,.63,groove);
vec3 timber = mix(face, boards, panel);
float top = 1. - smoothstep(.033-aa.y, .033+aa.y, p.y);
float sill = 1. - smoothstep(.018-aa.y, .018+aa.y, abs(p.y - (h-rail)));
timber = mix(timber, mix(face, p_cream.rgb, .20), max(top, sill*fullPanel));
float foot = 1. - smoothstep(.038-aa.y, .038+aa.y, h-p.y);
float side = 1. - smoothstep(.024-aa.x, .024+aa.x, 1.-p.x);
timber = mix(timber, mix(face,p_ink.rgb,.40), max(foot,side*.7));
float readable = smoothstep(4., 10., 1. / max(2.*aa.x,.0001));
return vec4(mix(face,timber,readable), color.a);
`;

const card = `
vec2 halfSize = size * .5;
float radius = min(7.0, min(halfSize.x, halfSize.y) * .38);
vec2 q = abs(position - halfSize) - halfSize + radius;
float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
float aa = max(.5 * fwidth(d), .05);
float cover = 1.0 - smoothstep(-aa, aa, d);
float face = 1.0 - smoothstep(-1.8-aa, -1.8+aa, d);
return vec4(mix(p_ink.rgb, p_body.rgb, face), cover);
`;

// The native UV family identifies continuous height sheets; their real triangle
// planes catch the key light individually. Pie walls retain their radial normals.
const mesh = `
vec4 materialFragment() {
    vec3 normal = normalize(v_normal);
    if (v_uv.x >= 6.0 && v_uv.x < 8.0) {
        vec3 facet = cross(dFdx(v_position), dFdy(v_position));
        if (dot(facet, facet) > 1e-14) normal = normalize(facet) * sign(dot(facet, normal));
    }
    ${pigment("v_color")}
    if (v_uv.y >= 1.5) face = p_selected.rgb;
    float v = v_uv.y - (v_uv.y >= 1.5 ? 2. : 0.);
    if (v_uv.x >= 12. && v_uv.x <= 13.) {
        // A narrow tonal seam separates adjoining square rims.
        float aa = max(fwidth(v)*.55, .001);
        face *= mix(.70, 1., smoothstep(.075-aa, .075+aa, v));
    } else if (v_uv.x >= 8. && v_uv.x <= 11.) {
        bool lid = v_uv.x >= 10.;
        vec2 uv = vec2(v_uv.x - (lid ? 10. : 8.), v);
        // Slightly tapered cuts leave the centre board wider than its neighbours.
        float firstCut = .275 + .030 * uv.y;
        float secondCut = .677 - .022 * uv.y;
        float groove = min(abs(uv.x - firstCut), abs(uv.x - secondCut));
        float px = max(fwidth(uv.x), .0001);
        float seam = 1. - smoothstep(.009-px*.5, .009+px*.5, groove);
        float contact = smoothstep(0., .16, 1. - v);
        float boardTone = .05 * step(firstCut, uv.x) - .085 * step(secondCut, uv.x);
        vec3 planks = face * ((lid ? .97 : .78) + boardTone);
        planks *= (1. - .32 * seam) * (lid ? 1. : .85 + .15 * contact);
        if (lid) {
            // Two painted cleats hold the three lid boards together.
            float end = min(v, 1. - v);
            float aa = max(fwidth(v)*.55, .0001);
            float cleat = 1. - smoothstep(.12-aa,.12+aa,end);
            float joint = 1. - smoothstep(.009-aa,.009+aa,abs(end-.135));
            planks = mix(planks * (1.-.22*joint), mix(face,p_cream.rgb,.11),cleat);
        }
        face = planks;
    }
    float light = dot(normal, normalize(vec3(-.42, .84, .52)));
    float shade = .63 + .39 * max(light, 0.0) + .17 * light;
    vec3 lit = face * shade;
    return vec4(mix(lit, p_cream.rgb, .17 * smoothstep(.54, 1.0, light)), 1.0);
}
`;

export function cartoonChartAssets(scope: string) {
  return (
    <>
      {cartoonPaperAssets(scope)}
      {pencilAxisAsset(scope)}
      <ShaderAsset
        id={`${scope}/chart-mark`}
        recipe={{}}
        parameters={{ cream: "vec4", ink: "vec4" }}
      >
        <PaintShader>{mark}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id={`${scope}/chart-crate`}
        recipe={{}}
        parameters={{ cream: "vec4", ink: "vec4" }}
      >
        <PaintShader>{crate}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id={`${scope}/chart-card`}
        recipe={{}}
        parameters={{ ink: "vec4", body: "vec4" }}
      >
        <PaintShader>{card}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id={`${scope}/chart-mesh`}
        recipe={{ normals: true }}
        parameters={{ cream: "vec4", selected: "vec4" }}
      >
        <VertexShader requiredAttributes={7}>
          {"void materialVertex() { ippDefaultVertex(); }"}
        </VertexShader>
        <FragmentShader requiredAttributes={7}>{mesh}</FragmentShader>
      </ShaderAsset>
    </>
  );
}

export function cartoonPlots(scope: string): PlotSkinAppearance {
  const text = cartoonTokens.text;
  const grid = cartoonTokens.accent;
  const frame = {
    red: text[0],
    green: text[1],
    blue: text[2],
    alpha: 1,
    grid_red: grid[0],
    grid_green: grid[1],
    grid_blue: grid[2],
    grid_alpha: 0.27,
  };
  const paint = { source: assetRef(`${scope}/chart-mark`), cream, ink };
  const panel = {
    source: assetRef(`${scope}/chart-card`),
    ink,
    body: cartoonTokens.surface,
  };
  return {
    paint,
    chartPaints: {
      PlotBars2d: { source: assetRef(`${scope}/chart-crate`), cream, ink },
    },
    axisPaint: { source: assetRef(`${scope}/pencil-axis`), pressure: 0.7 },
    marks: {
      PlotBars2d: { stacked_blocks: true, block_offset: 0.048 },
      PlotGridBars3d: { stacked_blocks: true, block_yaw: (9 * Math.PI) / 180 },
      PlotLine2d: { line_width: 4.5, marker_size: 9 },
      PlotHeightSurface3d: { wireframe: false },
      PlotPoints3d: { marker_size: 0.41, marker_shape: 1 },
    },
    paperPaint: { source: assetRef(`${scope}/chart-paper`) },
    baseMaterial: { source: assetRef(`${scope}/chart-base`) },
    material: {
      source: assetRef(`${scope}/chart-mesh`),
      alpha_mode: 0,
      receives_light: false,
      receives_shadows: false,
      casts_shadows: false,
      conservative_bounds: true,
      cream,
      selected: grass,
    },
    frame2d: {
      ...frame,
      axis_width_scale: 4.2,
      highlight_red: grass[0],
      highlight_green: grass[1],
      highlight_blue: grass[2],
      highlight_alpha: 1,
    },
    frame3d: {
      ...frame,
      axis_width_scale: 6,
      bar_bevel: 0,
      mark_outlines: false,
    },
    callout: {
      padding_x: 0.68,
      padding_y: 0.36,
      radius: 0.4,
      border_width: 0,
      red: cartoonTokens.surface[0],
      green: cartoonTokens.surface[1],
      blue: cartoonTokens.surface[2],
      alpha: 1,
      text_red: text[0],
      text_green: text[1],
      text_blue: text[2],
      text_alpha: 1,
      leader_red: grid[0],
      leader_green: grid[1],
      leader_blue: grid[2],
      leader_alpha: 0.95,
      leader_width: 0.07,
    },
    calloutPaint: panel,
    legend: { textColor: text, paint: panel, swatchPaint: paint },
  };
}
