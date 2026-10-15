import { OPTICAL_RESPONSE } from "./optical-material.js";

/** The GUI sheet's exact dielectric response, composited over a tinted backing.
 * Planar marks use a coloured backing; spatial glass uses alpha transmission.
 * There is no reflection target, backdrop sample, blur pass or shader clock.
 */
export const GLASS_CHART_RESPONSE = OPTICAL_RESPONSE.replaceAll(
  "p_optics",
  "p_finish",
)
  .replaceAll("p_tint", "chartTint")
  .replaceAll("p_accent", "chartAccent")
  .replaceAll("p_state.x", "0.0")
  .replaceAll("p_keyLight.w", "energy")
  .replace("normalize(p_keyLight.xyz)", "L");

export const GLASS_CHART_OPTICS = GLASS_CHART_RESPONSE.replace(
  "return vec4((body+reflected)/max(opacity,.001),coverage*clamp(opacity,0.0,1.0));",
  `// A dark coloured backing keeps data series legible through the slab.
vec3 backing=mix(vec3(.008,.017,.022),chartAccent.rgb*.36,.72);
vec3 result=body+reflected+backing*transmission*(1.0-F);
return vec4(result,1.0);`,
);

export const GLASS_CHART_PARAMETERS = {
  accent: "vec4",
  tint: "vec4",
  light: "vec4",
  finish: "vec4", // IOR, roughness, optical thickness, polished edge width
  lighting: "vec4", // scene-light opt-in
} as const;
