import { compactSizing } from "../gui-kit/sizing.js";
import { glassConnectedRegions } from "./connected-material.js";
import { GLASS_ARC_PAINT } from "./arc-material.js";
import { COMPONENT_PAINT } from "./sheet-material.js";
import type { GlassColor, GlassLight, GlassMaterial } from "./types.js";

/** Face bands are optional; the curved edge and direct light retain their response. */
function withReflectionStrength(source: string): string {
  return source
    .replace(
      "// A finite absorbing slab.",
      `environment=mix(vec3(.012,.023,.04),environment,
  mix(clamp(p_accent.a,0.0,1.0),1.0,bevel));
// A finite absorbing slab.`,
    )
    .replace("*ghost*.45", "*ghost*.45*clamp(p_accent.a,0.0,1.0)");
}

/** Keep reflected studio lights quiet at rest, including selected and checked parts.
 * Native interaction alone raises the highlight; optical shape and transmission remain.
 */
function withInteractionHighlights(source: string): string {
  return source.replace(
    "opacity=max(opacity,max(reflected.r,max(reflected.g,reflected.b)));",
    `float activity=clamp(max(max(data.x,data.y),step(3.5,data.w)),0.0,1.0);
// Bounded studio radiance preserves curved-edge detail instead of clipping to white.
// Selection keeps its tinted backing; it is not an interaction highlight.
reflected=reflected/(vec3(1.0)+reflected)*mix(.12,.72,activity);
opacity=max(opacity,max(reflected.r,max(reflected.g,reflected.b)));`,
  );
}

/** Shared dielectric sheets. Native rows supply state and part roles; no scene sampling. */
// Backing uses spare lanes so every paint keeps its six-vector parameter budget.
export const GLASS_PARAMETERS = {
  accent: "vec4", // RGB accent, face reflection strength
  glassTint: "vec4", // RGB tint, panel body opacity
  light: "vec4",
  finish: "vec4", // IOR, roughness, optical thickness, polished edge width
  body: "vec4", // family, geometric scale, thickness multiplier, connected region edges
  lighting: "vec4", // scene-light opt-in, linear panel backing RGB
} as const;

export const GLASS_CONTEXT = `\nfloat family=p_body.x;
// Choice optics stay centered inside the native target, including their focus rim.
// Keep the material's local coordinates matched to its smaller visible contour.
vec2 choiceInset=vec2(0.0);
if(family==1.0 || family==11.0)
  choiceInset=vec2(min(size.x,size.y)*${(compactSizing.choiceTarget - compactSizing.choiceIndicator) / (2 * compactSizing.choiceTarget)});
position-=choiceInset;
size=max(size-2.0*choiceInset,vec2(.001));
float minor=min(size.x,size.y);
float unitScale=p_body.y>0.0?p_body.y:clamp(minor/40.0,.25,4.0);
if(minor>72.0 && (family==4.0 || family==6.0)) unitScale=clamp(minor/240.0,.8,1.4);
vec3 pale=mix(vec3(.70,.84,.90),p_accent.rgb,.14);
`;

export const GLASS_SHADER =
  GLASS_CONTEXT +
  `
if(family==12.0 || (family==7.0 && abs(data.w)>.5 && data.w<1.5)) {
  vec4 arcOptics=vec4(p_finish.xyz,1.0);
  vec4 arcLight=vec4((p_light.xy-.5)*6.0,p_light.zw);
  vec4 arcState=vec4(0.0,p_lighting.x,0.0,0.0);
  vec4 arcTint=vec4(mix(p_glassTint.rgb,p_accent.rgb,.35),1.0);
  ${withReflectionStrength(withInteractionHighlights(GLASS_ARC_PAINT))}
}
${glassConnectedRegions()}
float roundness=(family==2.0||family==3.0||family==5.0||family==7.0||family==8.0||family==11.0)?10000.0:8.0*unitScale;
vec4 geometry=vec4(roundness,3.0*unitScale*p_finish.w,0.0,0.0);
if(family==7.0) { geometry.x=minor*.39; geometry.w=1.0; }
if(family==1.0) geometry.x=5.0*unitScale;
if(family==11.0) geometry.w=-1.0;
if(data.w>1.5 && data.w<3.5) geometry.x=10000.0;
if(family==8.0) geometry.z=1.0;
vec4 optical=vec4(p_finish.xyz,1.0);
optical.z*=p_body.z;
if(family==4.0) optical.y=max(optical.y,.25);
if(family==6.0 && p_body.w>5.5) { optical.y=max(optical.y,.40); optical.z=max(optical.z,5.0); }
vec4 keyLight=vec4((p_light.xy-.5)*6.0,p_light.zw);
vec4 opticalState=vec4(0.0,p_lighting.x,0.0,0.0);
vec4 tint=vec4(mix(vec3(.30,.57,.65),p_glassTint.rgb,.35),1.0);
// Joined backgrounds already receive their complete sheet rectangle from layout.
vec4 continuation=vec4(0.0);
vec4 feedback=vec4(0.0);
` +
  withReflectionStrength(withInteractionHighlights(COMPONENT_PAINT))
    .replaceAll("p_geometry", "geometry")
    .replace(
      "float depth=max(-contour,0.0)/unit;",
      `// Flat sheets keep the same rounded contour and antialiasing as the controls.
// Scroll thumbs still use their lens optics.
if(family==4.0 && !(data.w>1.5 && data.w<3.5))
  return vec4(p_lighting.yzw,coverage*clamp(p_glassTint.a,0.0,1.0));
float depth=max(-contour,0.0)/unit;`,
    )
    .replace(
      "vec2 sheetPosition=position+vec2(0.0,p_continuation.x);",
      "vec2 sheetPosition=position+vec2(0.0,p_continuation.x); if(family==7.0) sheetPosition-=size*.11;",
    )
    .replace(
      "vec2 sheetSize=vec2(size.x,size.y+p_continuation.x+p_continuation.y);",
      "vec2 sheetSize=vec2(size.x,size.y+p_continuation.x+p_continuation.y); if(family==7.0) sheetSize*=.78;",
    )
    .replaceAll("p_feedback", "feedback")
    .replaceAll("p_continuation", "continuation")
    .replaceAll("p_optics", "optical")
    .replaceAll("p_keyLight", "keyLight")
    .replaceAll("p_state", "opticalState")
    .replaceAll("p_tint", "tint")
    .replace(
      "if(geometry.w<-.5) selected=0.0;",
      "if(geometry.w<-.5 || family==11.0) selected=0.0;",
    )
    .replace(
      "vec3(.17,.34,.39)*selected",
      "mix(vec3(.17,.34,.39),p_accent.rgb*.48,.5)*selected",
    )
    .replace(
      "body+=vec3(.055,.09,.105)*lens;",
      "body+=vec3(.055,.09,.105)*lens; body+=vec3(.02,.035,.045)*pressed;",
    )
    .replace("vec3(.30,.65,.72)*radioDot", "p_accent.rgb*radioDot")
    .replace(
      "opacity=max(opacity,max(radioDot,insert*.42));",
      "opacity=max(opacity,max(radioDot,insert*.42)); if(family==8.0) opacity=1.0; if(family==6.0 && p_body.w>5.5) { body+=vec3(.012,.024,.028); opacity=max(opacity,.985); }",
    );

/** Flat panel bodies are slightly transparent by default; controls retain their optics. */
export function glassPanelBacking(
  background: GlassColor,
  opacity = 0.98,
): GlassColor {
  if (!Number.isFinite(opacity) || opacity < 0 || opacity > 1)
    throw new RangeError("Glass panelOpacity must be between 0 and 1");
  return [background[0], background[1], background[2], opacity];
}

export function glassReflectionStrength(strength = 1): number {
  if (!Number.isFinite(strength) || strength < 0 || strength > 1)
    throw new RangeError("Glass reflectionStrength must be between 0 and 1");
  return strength;
}

/** Family profiles share one material; only geometry and optical depth differ. */
export function glassBody(
  theme: string,
): readonly [number, number, number, number] {
  if (theme === "checkbox") return [1, 0, 1, 0];
  if (theme === "switch") return [2, 0, 1, 0];
  if (theme === "slider") return [3, 0, 1, 0];
  if (theme === "panel") return [4, 0, 1, 0];
  if (theme === "progress") return [5, 0, 1, 0];
  return [0, 0, 1, 0];
}

/** Authored source in normalized face XY and face-height Z units. */
export const GLASS_LIGHT: GlassLight = Object.freeze({
  x: 0.08,
  y: 0.08,
  height: 5,
  intensity: 0.65,
});
export const GLASS_MATERIAL: GlassMaterial = Object.freeze({
  ior: 1.5,
  roughness: 0.2,
  thickness: 0.8,
  edge: 1,
});

export function glassLight(input: Partial<GlassLight> = {}): GlassLight {
  const light = { ...GLASS_LIGHT, ...input };
  if (
    !Object.values(light).every(Number.isFinite) ||
    light.height <= 0 ||
    light.intensity < 0
  )
    throw new RangeError(
      "Glass light requires finite values, positive height and nonnegative intensity",
    );
  return light;
}

export function glassMaterial(
  input: Partial<GlassMaterial> = {},
): GlassMaterial {
  const material = { ...GLASS_MATERIAL, ...input };
  if (
    !Object.values(material).every(Number.isFinite) ||
    material.ior < 1.01 ||
    material.ior > 2.5 ||
    material.roughness < 0.09 ||
    material.roughness > 0.85 ||
    material.thickness <= 0 ||
    material.edge <= 0
  )
    throw new RangeError(
      "Glass material requires IOR 1.01..2.5, roughness .09..0.85 and positive thickness and edge",
    );
  return material;
}
