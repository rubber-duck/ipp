import { GLASS_CHART_OPTICS, GLASS_CHART_PARAMETERS } from "./chart-finish.js";

export const GLASS_CHART_PAINT_PARAMETERS = GLASS_CHART_PARAMETERS;
export const GLASS_CHART_LIGHT = [0.08, 0.08, 5, 0.65] as const;
export const GLASS_CHART_FINISH = [1.5, 0.2, 0.8, 1] as const;

/** Rolled glass follows native box/path contours, including curved line strokes.
 * Only the finish changes: coverage, data colour, bounds and picking stay native.
 */
export const GLASS_CHART_PAINT = `
vec2 faceSize=max(size,vec2(.001));
vec2 uv=position/faceSize;
vec3 hue=color.rgb/max(max(color.r,color.g),max(color.b,.001));
vec4 chartAccent=vec4(mix(hue,p_accent.rgb,clamp(p_accent.a,0.0,1.0)),1);
vec4 chartTint=vec4(mix(p_tint.rgb,chartAccent.rgb,.5),1);
float depth=max(-edge,0.0);
// Recover the local contour normal from derivatives; no rectangular imitation
// of a pie sector or smooth line, and no new renderer geometry is needed.
vec2 dx=dFdx(position),dy=dFdy(position);
float determinant=dx.x*dy.y-dx.y*dy.x;
vec2 gradient=vec2(dFdx(edge)*dy.y-dFdy(edge)*dx.y,
                  dFdy(edge)*dx.x-dFdx(edge)*dy.x);
gradient*=sign(determinant);
vec2 outward=gradient*inversesqrt(max(dot(gradient,gradient),1e-12));
float edgeWidth=max(min(min(faceSize.x,faceSize.y)*.12,2.4)*p_finish.w,.2);
float bevel=clamp(1.0-depth/edgeWidth,0.0,1.0);
float slope=sin(bevel*1.48);
vec3 N=vec3(outward*slope,sqrt(max(1.0-slope*slope,.001)));
vec3 V=viewDirection; V.z=abs(V.z);
vec3 L=normalize(vec3((p_light.xy-.5)*6.0-(uv-.5)*vec2(faceSize.x/faceSize.y,1),p_light.z));
float energy=max(p_light.w,0.0);
if(p_lighting.x>.5 && sceneLightCount>0) {
  L=sceneLightDirection(0);
  energy*=dot(sceneLightRadiance(0),vec3(.2126,.7152,.0722));
}
${GLASS_CHART_OPTICS}
`;
