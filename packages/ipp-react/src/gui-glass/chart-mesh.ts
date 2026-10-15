import {
  GLASS_CHART_RESPONSE,
  GLASS_CHART_PARAMETERS,
} from "./chart-finish.js";

export const GLASS_CHART_MESH_RECIPE = {
  normals: true,
  lighting: true,
} as const;
export const GLASS_CHART_MESH_ATTRIBUTES = 7;
export const GLASS_CHART_MESH_PARAMETERS = GLASS_CHART_PARAMETERS;

export const GLASS_CHART_MESH_VERTEX = `
out vec3 chartOrigin;
out vec3 chartLight;
void materialVertex() {
  ippDefaultVertex();
  chartOrigin=(u_model*vec4(0,0,0,1)).xyz;
  chartLight=mat3(u_model)*p_light.xyz;
}
`;

/** Native mesh faces share the same optics as GUI sheets. Rounded shading rolls
 * over their real edges; triangulated height surfaces have no artificial seams.
 */
export const GLASS_CHART_MESH_FRAGMENT = `
in vec3 chartOrigin;
in vec3 chartLight;
vec4 materialFragment() {
  vec3 normal=normalize(v_normal);
  vec3 eye=u_camera.w>.5?normalize(u_camera.xyz):normalize(u_camera.xyz-v_position);
  float frontFace=step(0.0,dot(normal,eye));
  normal*=mix(-1.0,1.0,frontFace);
  vec2 uv=v_uv;
  float highlighted=step(1.5,uv.y);
  uv.y-=highlighted*2.0;
  bool triangle=uv.x>1.5&&uv.x<3.5;
  bool sheet=uv.x>5.5;
  bool radial=uv.x>3.5&&!sheet;
  uv.x-=sheet?6.0:(radial?4.0:(triangle?2.0:0.0));
  // Open height sheets retain both sides. Volumes approximate the weak rear
  // optical image procedurally, avoiding sharp far-side wires through the slab.
  if(sheet) frontFace=1.0;
  vec3 dx=dFdx(v_position),dy=dFdy(v_position);
  vec2 ux=dFdx(uv),uy=dFdy(uv);
  float determinant=ux.x*uy.y-ux.y*uy.x;
  float invDet=sign(determinant)/max(abs(determinant),1e-10);
  vec3 du=(dx*uy.y-dy*ux.y)*invDet;
  vec3 dv=(dy*ux.x-dx*uy.x)*invDet;
  vec3 fallback=abs(normal.y)>.9?vec3(1,0,0):normalize(cross(vec3(0,1,0),normal));
  vec3 tangent=length(du)>.00001?normalize(du):fallback;
  vec3 orthogonal=dv-tangent*dot(dv,tangent);
  vec3 bitangent=length(orthogonal)>.00001?normalize(orthogonal):cross(normal,tangent);
  vec2 faceSize=max(vec2(length(du),length(dv)),vec2(.001));
  vec2 inset=min(uv,1.0-uv)*faceSize;
  if(radial) inset.y=(1.0-uv.y)*faceSize.y;
  float depth=max(min(inset.x,inset.y),0.0);
  float edgeWidth=max(min(min(faceSize.x,faceSize.y)*.12,.09)*p_finish.w,.001);
  float bevel=(triangle||sheet)?0.0:clamp(1.0-depth/edgeWidth,0.0,1.0);
  vec2 direction=sign(uv-.5);
  if(radial) direction.y=1.0;
  vec2 weights=max(vec2(0),vec2(1)-inset/edgeWidth);
  vec2 outward=direction*weights/max(length(weights),.0001);
  // Adjacent volume faces each roll halfway into the same polished edge.
  // A sheet's nearly 90-degree roll on both faces creates two separate rims.
  float slope=sin(bevel*.78539816);
  vec3 N=normalize(tangent*outward.x*slope+bitangent*outward.y*slope
    +normal*sqrt(max(1.0-slope*slope,.001)));
  vec3 worldLight=p_lighting.y>.5
    ?normalize(chartLight)
    :normalize(p_light.xyz-v_position);
  float energy=max(p_light.w,0.0);
  if(p_lighting.x>.5 && u_light_count>0) {
    worldLight=ippLightDirection(0,v_position);
    energy*=dot(ippLightRadiance(0,v_position),vec3(.2126,.7152,.0722));
  }
  vec3 V=eye;
  vec3 L=worldLight;
  vec3 hue=v_color/max(max(v_color.r,v_color.g),max(v_color.b,.001));
  vec4 chartAccent=vec4(mix(hue,p_accent.rgb,clamp(p_accent.a,0.0,1.0)),1);
  vec4 chartTint=vec4(mix(p_tint.rgb,chartAccent.rgb,.72),1);
  ${GLASS_CHART_RESPONSE.replace("abs(T.z)", "abs(dot(T,normal))")
    .replace(
      "float blur=.025+rough*rough*.72;",
      "float blur=.16+rough*rough*.9;",
    )
    .replace(
      "// A finite absorbing slab.",
      `// A finite analytic studio enclosure gives flat orthographic faces real
// reflection parallax. It contains only softbox cards, never scene geometry.
vec3 studioPoint=(v_position-chartOrigin)*.12-vec3(.6,.6,.6);
// Each softbox is a finite card on a fixed studio plane. Summing their
// smooth windows avoids discontinuities where a cubemap chooses another wall.
float ceilingTravel=(.9-studioPoint.y)/max(R.y,.0001);
vec2 ceilingCard=(studioPoint.xz+R.xz*ceilingTravel)*.5;
float ceilingStrip=ceilingCard.x+ceilingCard.y*.42+dot(L,vec3(.28,.14,.18));
float cardA=exp(-pow((ceilingStrip+.25)/(.13+blur),2.0))
  *(1.0-smoothstep(.95,1.5,abs(ceilingCard.y+.55)))
  *smoothstep(0.0,.18,R.y);
float sideTravel=(2.0-studioPoint.z)/max(R.z,.0001);
vec2 sideCardPoint=(studioPoint.xy+R.xy*sideTravel)*.5;
float sideStrip=sideCardPoint.x+sideCardPoint.y*.42+dot(L,vec3(.18,.24,.08));
float cardB=exp(-pow((sideStrip-.18)/(.055+blur*.7),2.0))
  *(1.0-smoothstep(.65,1.0,abs(sideCardPoint.y)))
  *smoothstep(0.0,.18,R.z);
environment+=vec3(.80,.91,1.0)*(cardA*3.8+cardB*2.6)*energy/(1.0+rough*1.8);
// A finite ring of three studio softboxes surrounds the slab. Intersecting
// the reflected ray with that enclosure gives wide faces spatial detail too.
float roomA=max(dot(R.xz,R.xz),.0001);
float roomB=dot(studioPoint.xz,R.xz);
float roomC=dot(studioPoint.xz,studioPoint.xz)-4.0;
float roomTravel=max((-roomB+sqrt(max(roomB*roomB-roomA*roomC,0.0)))/roomA,0.0);
vec3 roomHit=studioPoint+R*roomTravel;
float roomAngle=atan(roomHit.z,roomHit.x)+dot(L,vec3(.26,.15,.23));
float roomCards=exp(-pow(sin(roomAngle*1.5+.35)/(.20+rough*.3),2.0))
  *(1.0-smoothstep(1.2,1.8,abs(roomHit.y)));
// The surrounding cards describe volume walls. Open height sheets already
// receive the ceiling and side cards; adding this ring washes out their peaks.
environment+=vec3(.78,.89,1.0)*roomCards*(sheet?0.0:8.0)*energy/(1.0+rough);
// A finite absorbing slab.`,
    )
    .replace(
      "vec3 body=scatter",
      `// Selection lights the whole tinted slab so it remains legible even
// when the polished rim faces away from the studio cards.
vec3 selectionTint=mix(chartAccent.rgb,vec3(.82,.94,1.0),.16);
vec3 body=chartAccent.rgb*(1.0-exp(-thickness*.55))*.14*(.55+.65*NoL)*(1.0-F)
  +selectionTint*highlighted*.24*(.7+.3*NoL)*(1.0-F)+scatter`,
    )
    .replace(
      "float thickness=max(p_finish.z,.01)",
      "float thickness=max(p_finish.z*3.0,.01)",
    )
    .replace(
      "// Preserve reflected energy",
      `// Strengthen the existing optical reflection, retaining its partial,
// view-dependent rim instead of drawing a constant outline around the mark.
reflected*=1.0+highlighted*.65;
reflected*=mix(.035,1.0,frontFace);
// Preserve reflected energy`,
    )
    .replace(
      "return vec4((body+reflected)/max(opacity,.001),",
      "opacity*=frontFace; body*=frontFace; reflected*=frontFace;\nreturn vec4((body+reflected)/max(opacity,.001),",
    )
    .replace("coverage*clamp(opacity,0.0,1.0)", "clamp(opacity,0.0,1.0)")}
}
`;
