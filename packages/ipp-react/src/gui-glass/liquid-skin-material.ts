import { GLASS_CONTEXT } from "./material.js";
import { glassConnectedRegions } from "./connected-material.js";
import { LIQUID_PAINT } from "./liquid-material.js";
import { LIQUID_ARC_PAINT } from "./liquid-arc-material.js";

/** One liquid material for the full GUI kit. Native shapes retain exact colour
 * fields and arc coverage; parent backings stay flat and connected regions
 * tint one shared sheet instead of introducing independent bevels.
 */
export const LIQUID_GLASS_SHADER =
  GLASS_CONTEXT +
  `
if(family==12.0 || (family==7.0 && abs(data.w)>.5 && data.w<1.5)) {
  ${LIQUID_ARC_PAINT}
}
${glassConnectedRegions(true)}
vec4 geometry=vec4(10000.0,0.0,0.0,0.0);
if(family==1.0) geometry.x=6.0*unitScale;
if(family==4.0) geometry.x=8.0*unitScale;
// Expanded sheets use modest corners over the complete joined rectangle.
if(family==6.0 && p_body.w>5.5) geometry.x=18.0*unitScale;
if(family==7.0) geometry.x=minor*.39;
if(family==8.0 || (family==3.0 && data.w>.5 && data.w<1.5)) geometry.z=1.0;
if(family==11.0) geometry.w=-1.0;
if(data.w>1.5 && data.w<3.5) geometry.x=10000.0;
vec4 optical=vec4(p_finish.xyz,p_finish.w*unitScale);
vec4 keyLight=vec4((p_light.xy-.5)*6.0,p_light.zw);
vec4 opticalState=vec4(0.0,p_lighting.x,0.0,0.0);
vec4 sheetTint=p_glassTint;
vec4 continuation=vec4(0.0),feedback=vec4(0.0);
${LIQUID_PAINT.replaceAll("p_geometry", "geometry")
  .replaceAll("p_optics", "optical")
  .replaceAll("p_keyLight", "keyLight")
  .replaceAll("p_state", "opticalState")
  .replaceAll("p_tint", "sheetTint")
  .replaceAll("p_continuation", "continuation")
  .replaceAll("p_feedback", "feedback")
  .replace("float minor=min(sheetSize.x,sheetSize.y);", "")
  .replace(
    "vec2 sheetSize=size+vec2(0.0,continuation.x+continuation.y);",
    "vec2 sheetSize=size+vec2(0.0,continuation.x+continuation.y); if(family==7.0 && !(data.w>1.5 && data.w<3.5)) sheetSize*=.78;",
  )
  .replace(
    "vec2 sheetPosition=position+vec2(0.0,continuation.x);",
    "vec2 sheetPosition=position+vec2(0.0,continuation.x); if(family==7.0 && !(data.w>1.5 && data.w<3.5)) sheetPosition-=size*.11;",
  )
  .replace(
    "float depth=max(-d,0.0);",
    `
if(family==4.0 && !(data.w>1.5 && data.w<3.5))
  return vec4(p_lighting.yzw,coverage*clamp(p_glassTint.a,0.0,1.0));
float depth=max(-d,0.0);`,
  )
  .replace(
    "// Small native thumbs",
    `if(family==6.0 && p_body.w>5.5) opacity=max(opacity,.96);
// Small native thumbs`,
  )}
`;
