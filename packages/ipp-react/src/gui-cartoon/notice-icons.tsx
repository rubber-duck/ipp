import { ShaderAsset, assetRef } from "../assets/declarations.js";
import type { GuiKitContract } from "../gui-kit/kit.js";
import type { GuiSkinTheme } from "../gui/skin-types.js";
import { PaintShader } from "../shaders.js";
import { cartoonTokens, ink } from "./palette.js";

// Work in a 24-unit square so the ink weight and the punctuation scale together.
const noticeIcon = `
vec2 p = (position - 0.5 * size) * (24.0 / max(min(size.x, size.y), 0.001));
bool warning = data.x > 0.5;
float edgeDistance = length(p) - 10.8;
if (warning) {
  vec2 q = vec2(abs(p.x), p.y);
  vec2 apex = vec2(0.0, -8.2);
  vec2 corner = vec2(8.2, 6.5);
  vec2 side = corner - apex;
  float along = clamp(dot(q - apex, side) / dot(side, side), 0.0, 1.0);
  float sideDistance = length(q - apex - along * side);
  float baseDistance = length(vec2(max(q.x - corner.x, 0.0), q.y - corner.y));
  bool inside = q.y <= corner.y && (q.y - apex.y) * side.x >= q.x * side.y;
  edgeDistance = (inside ? -1.0 : 1.0) * min(sideDistance, baseDistance) - 1.9;
}
float aa = max(0.001, 0.5 * fwidth(edgeDistance));
float cover = 1.0 - smoothstep(-aa, aa, edgeDistance);
float face = 1.0 - smoothstep(-1.7 - aa, -1.7 + aa, edgeDistance);
float bevel = smoothstep(4.7, 6.0, p.y);
vec3 fill = mix(color.rgb, color.rgb * 0.61, bevel);
vec2 stemRange = warning ? vec2(-3.9, 0.7) : vec2(-0.8, 4.0);
float stem = length(vec2(p.x, p.y - clamp(p.y, stemRange.x, stemRange.y))) - 1.45;
float dotDistance = length(p - vec2(0.0, warning ? 4.65 : -4.8)) - 1.5;
float mark = min(stem, dotDistance);
float markAA = max(0.001, 0.5 * fwidth(mark));
float letter = 1.0 - smoothstep(-markAA, markAA, mark);
return vec4(mix(p_ink.rgb, fill, face * (1.0 - letter)), color.a * cover);
`;

export const noticeIconAsset = (scope: string) => (
  <ShaderAsset
    id={`${scope}/notice-icon`}
    recipe={{ presentation: true }}
    parameters={{ ink: "vec4" }}
  >
    <PaintShader>{noticeIcon}</PaintShader>
  </ShaderAsset>
);

export function cartoonNoticeIcons(
  contract: GuiKitContract,
  themes: Record<string, GuiSkinTheme>,
) {
  const icons = {
    information: "icon/information",
    warning: "icon/warning",
  } as const;
  const background = contract.guiPaintPartIndex({ part: "background" });
  for (const icon of ["information", "warning"] as const) {
    themes[icons[icon]] = {
      em: cartoonTokens.em,
      rows: [
        {
          part: background,
          color: [1, 1, 1, 1],
          paint: true,
          paint_data: [icon === "warning" ? 1 : 0, 0, 0, 0],
        },
      ],
      paint: (scope) => ({ source: assetRef(`${scope}/notice-icon`), ink }),
    };
  }
  return icons;
}
