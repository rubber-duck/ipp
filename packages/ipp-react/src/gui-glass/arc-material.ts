import { OPTICAL_PRESENTED_PAINT } from "./optical-material.js";

/** A cut glass annulus. Native arc coverage retains the exact sweep and width. */
export const GLASS_ARC_PAINT = OPTICAL_PRESENTED_PAINT.replace(
  /vec2 uv=position\/size;[\s\S]*?vec3 point=/,
  `vec2 uv=position/size;
float unit=max(min(size.x,size.y)/80.0,.001);
vec2 radial=position-size*.5;
float radialLength=length(radial);
float depth=max(min(size.x,size.y)*.5-radialLength,0.0)/unit;
float coverage=1.0;
// A rolled annular cross-section reflects on both sides of the glass. Its
// optical profile is approximate; native coverage owns the exact inner cut,
// outside edge and arc ends, so paint cannot change the completion amount.
vec2 edgeNormal=radial/max(radialLength,.001);
float crossSection=clamp(depth/10.0,0.0,1.0);
float slope=cos(crossSection*3.14159265)*.97;
float bevel=abs(slope);
vec3 N=vec3(edgeNormal*slope,sqrt(max(1.0-slope*slope,.001)));
vec3 point=`,
)
  .replace(
    "// A finite absorbing slab.",
    `// Use the same broad studio emitters as the small glass lenses. Their
// reflections move with the light and view across the rolled glass edge.
float rimCard=pow(max(dot(R,normalize(vec3(-.68,-.28,-.68)+vec3(L.xy*.20,0))),0.0),12.0);
float topCard=pow(max(dot(R,normalize(vec3(.25,-.78,-.56)+vec3(L.xy*.15,0))),0.0),14.0);
float faceCard=pow(max(dot(R,normalize(vec3(-.45,-.60,.66)+vec3(L.xy*.20,0))),0.0),8.0);
environment+=vec3(.78,.91,1.0)*(rimCard*4.5+topCard*3.5+faceCard*4.0);
// A finite absorbing slab.`,
  )
  .replace(
    "vec3 body=scatter+p_accent.rgb*p_state.x*.008*(1.0-F);",
    `float litArc=step(.5,data.w);
vec3 body=scatter+color.rgb*litArc*.19*(1.0-F);
opacity=max(opacity,mix(.18,.48,litArc));`,
  )
  .replace(
    "opacity=max(opacity,max(reflected.r,max(reflected.g,reflected.b)));",
    `reflected*=mix(.42,1.8,litArc);
opacity=max(opacity,max(reflected.r,max(reflected.g,reflected.b)));`,
  )
  .replaceAll("p_optics", "arcOptics")
  .replaceAll("p_keyLight", "arcLight")
  .replaceAll("p_state", "arcState")
  .replaceAll("p_tint", "arcTint");
