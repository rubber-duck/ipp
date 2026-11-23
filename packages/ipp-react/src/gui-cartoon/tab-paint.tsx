import { assetRef, ShaderAsset } from "../assets/declarations.js";
import { PaintShader } from "../shaders.js";
import { cartoonTokens, cream, gold, honey, ink, leaf } from "./palette.js";
import { compactSizing } from "../gui-kit/sizing.js";
import type { GuiKitColor } from "../gui-kit/kit.js";

const rgb = (color: GuiKitColor) => `vec3(${color.slice(0, 3).join(",")})`;

// The native motion sampler blends selection, stroke width, stroke tint and
// stripe strength. Both ends of the transition use this same rounded contour.
const tabPaint = `
float unit = size.y / ${compactSizing.controlHeight.toFixed(1)};
bool ring = data.x < 0.0;
float selected = ring ? 1.0 : clamp(data.x, 0.0, 1.0);
vec2 halfSize = max(0.5 * size + mix(p_tilt.zw, p_shape.xy, selected) * unit, vec2(0.001));
float angle = selected * sign(p_tilt.x) * min(radians(abs(p_tilt.x)), p_shape.w * unit / halfSize.x);
float c = cos(angle), s = sin(angle);
vec2 point = mat2(c, s, -s, c) * (position - 0.5 * size);
float radius = min(p_shape.z * unit, min(halfSize.x, halfSize.y));
vec2 q = abs(point) - halfSize + radius;
float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
float aa = max(0.001, 0.5 * fwidth(d));
float coverage = 1.0 - smoothstep(-aa, aa, d);
float interior = 1.0 - smoothstep(-data.y * unit - aa, -data.y * unit + aa, d);
if (ring) return vec4(${rgb(gold)}, coverage - interior);
vec3 stroke = mix(${rgb(ink)}, ${rgb(cream)}, max(data.z, 0.0));
stroke = mix(stroke, ${rgb(cartoonTokens.line)}, max(-data.z, 0.0));
float band = mix(7.0, 4.5 + p_tilt.y * clamp(point.x / halfSize.x, -1.0, 1.0), selected);
float stripe = smoothstep(-aa, aa, point.y - halfSize.y + band * unit);
vec3 restingStripe = mix(${rgb(honey)}, ${rgb(gold)}, max(data.z, 0.0));
vec3 face = mix(color.rgb, mix(restingStripe, ${rgb(leaf)}, selected), stripe * data.w);
return vec4(mix(stroke, face, interior / max(coverage, 0.0001)), color.a * coverage);
`;

export const tabAsset = (scope: string) => (
  <ShaderAsset
    id={`${scope}/tab-paint`}
    recipe={{ presentation: true }}
    parameters={{ shape: "vec4", tilt: "vec4" }}
  >
    <PaintShader>{tabPaint}</PaintShader>
  </ShaderAsset>
);

export const tabAppearance =
  (
    shape: readonly [number, number, number, number],
    angle: number,
    skew: number,
    inset: readonly [number, number],
  ) =>
  (scope: string) => ({
    source: assetRef(`${scope}/tab-paint`),
    shape,
    tilt: [angle, skew, ...inset] as const,
  });
