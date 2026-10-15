import { STYLIZED_FACE } from "./reflections.js";

/** Shared opaque face, with independent outward halo adapters for Canvas and meshes. */
const face = `
vec2 inset=min(uv,1.0-uv)*faceSize;
float depth=min(inset.x,inset.y);
float aa=max(fwidth(depth),0.4);
float selected=p_profile.x;
vec3 pale=mix(vec3(1.0),p_accent.rgb,0.13);
vec2 edgeSign=step(vec2(0.5),uv)*2.0-1.0;
vec3 bevel=normalize(normal+(inset.x<inset.y?tangent*edgeSign.x:bitangent*edgeSign.y)*0.8);
float bevelLight=pow(max(dot(bevel,halfLight),0.0),14.0)*p_light.w;
vec2 corner=vec2(inset.x,uv.y*faceSize.y);
float triangle=1.0-(corner.x+corner.y)/max(faceSize.x*0.47,1.0);
float facet=smoothstep(0.0,0.025,triangle)*pow(max(triangle,0.0),1.5);
vec3 facetNormal=normalize(normal-tangent*edgeSign.x*0.45-bitangent*0.35);
float facetLight=pow(max(dot(facetNormal,halfLight),0.0),9.0)*p_light.w;
float gradient=mix(0.36,1.10,pow(1.0-uv.y,0.7));
vec3 rgb=p_tint.rgb*gradient*shade;
rgb+=p_accent.rgb*selected*0.10*pow(1.0-uv.y,2.0)*response;
rgb+=p_accent.rgb*facet*(0.014+0.065*facetLight);
rgb+=pale*facet*facetLight*0.045;
float reflection=sin(uv.x*9.0+sin(uv.y*15.0)*.8+p_light.x*.05)*sin(uv.y*11.0-uv.x*3.0);
rgb+=p_accent.rgb*pow(max(reflection,0.0),3.0)*.018*(.2+.8*facetLight);
float slash=exp(-pow((uv.x*0.55+uv.y-0.28)/0.10,2.0));
rgb+=p_accent.rgb*slash*0.012*response;
${STYLIZED_FACE}
float broad=exp(-max(depth,0.0)/7.0);
float narrow=exp(-max(depth,0.0)/1.7);
rgb+=p_accent.rgb*(broad*(0.016+selected*0.11)+narrow*(0.045+selected*0.10))*response;
float top=exp(-uv.y*faceSize.y/1.1);
float rim=1.0-smoothstep(0.65-aa*0.5,0.65+aa*0.5,depth);
vec3 rimColor=mix(p_accent.rgb*(.30+.48*top),pale,clamp(bevelLight*0.20+selected*0.92+exp(-(inset.x+inset.y)/3.0)*0.23*response,0.0,1.0));
rgb+=pale*selected*.16*exp(-inset.x/1.25)*exp(-pow((uv.y-.25)/.28,2.0))*response;
rgb=mix(rgb,rimColor,rim);
rgb+=p_accent.rgb*top*(0.20+selected*0.12)*response;
`;
export const PARAMETERS = {
  tint: "vec4",
  accent: "vec4",
  light: "vec4",
  profile: "vec4",
} as const;
export const BAR_PAINT = `
float pad=p_profile.y;
vec2 faceSize=size-vec2(2.0*pad);
vec2 q=position-vec2(pad);
vec2 outside=max(-q,q-faceSize);
float distance=length(max(outside,vec2(0)))+min(max(outside.x,outside.y),0.0);
if(distance>0.0){
  float glow=exp(-distance/1.15)*(0.012+p_profile.x*0.12);
  return vec4(p_accent.rgb,glow*(1.0-smoothstep(pad-2.0,pad,distance)));
}
vec2 uv=clamp(q/faceSize,0.0,1.0);
vec3 normal=vec3(0,0,1),tangent=vec3(1,0,0),bitangent=vec3(0,1,0);
vec3 toLight=normalize(vec3((p_light.xy-uv)*vec2(faceSize.x/faceSize.y,1),p_light.z));
vec3 viewDir=vec3(0,0,1);
vec3 halfLight=normalize(toLight+viewDir);
vec3 surfacePoint=vec3((uv.x-.5)*1.4,(1.0-uv.y)*2.7,0);
float response=0.4+0.6*max(toLight.z,0.0)*p_light.w;
float shade=1.0;
${face}
return vec4(rgb,1.0);
`;
export const PANEL_PAINT = `
float pad=p_profile.w;
vec2 q=position-vec2(pad),extent=size-vec2(2.0*pad);
vec2 uv=q/extent;
vec2 inset=min(q,extent-q);
float cut=p_profile.y;
float cornerDistance=p_profile.z>.5?max(cut-q.x-q.y,cut-(extent.x-q.x)-(extent.y-q.y)):cut-inset.x-inset.y;
float d=max(max(-inset.x,-inset.y),cornerDistance*0.70710678);
if(p_profile.z>1.5){
  float diamond=(abs(q.x-extent.x*.5)+abs(q.y-extent.y*.5)-extent.x*.5)*0.70710678;
  vec3 rgb=mix(p_accent.rgb,vec3(.8,.97,1),exp(-abs(diamond)/.65));
  return vec4(rgb,1.0-smoothstep(-.4,.45,diamond));
}
float topLeft=exp(-length(q-vec2(cut,0.0))/34.0);
float bottomRight=exp(-length(q-vec2(extent.x-cut,extent.y))/28.0);
float glint=max(topLeft,bottomRight);
if(d>0.0) return vec4(p_accent.rgb,p_profile.x>.5?0.0:exp(-d/1.1)*(0.004+glint*.11)*(1.0-smoothstep(pad-1.0,pad,d)));
float depth=-d;
vec3 rgb=p_tint.rgb;
float atmosphere=exp(-pow((uv.x*.6+uv.y*.4-.28)/.34,2.0));
rgb+=p_accent.rgb*atmosphere*(p_profile.z>.5?.002:.0007);
float corner=max(1.0-(inset.x+inset.y)/(cut*2.8),0.0);
rgb+=p_accent.rgb*corner*.025;
float innerSeam=exp(-abs((inset.x+inset.y)-cut*2.4)/.6)*corner;
rgb+=p_accent.rgb*innerSeam*.055;
float rim=1.0-smoothstep(.2,.85,depth);
vec3 edgeColor=mix(p_accent.rgb*.20,vec3(.77,.96,1),clamp(glint*.95+p_profile.z*.15,0.0,1.0));
rgb=mix(rgb,edgeColor,rim);
// Repaint the decorative corners above an attached camera without an opaque rectangle.
if(p_profile.x>.5){
  float coverage=max(corner,rim);
  vec3 ornament=p_tint.rgb+p_accent.rgb*.025+p_accent.rgb*innerSeam*.055/max(corner,.001);
  return vec4(mix(ornament,edgeColor,rim),coverage);
}
return vec4(rgb,1.0);
`;
export const LINE_PAINT = `
vec2 a=p_light.xy,b=p_light.zw,delta=b-a;
float t=clamp(dot(position-a,delta)/max(dot(delta,delta),.001),0.0,1.0);
float d=length(position-a-t*delta);
float alpha=1.0-smoothstep(p_profile.x-.4,p_profile.x+.4,d);
return vec4(p_tint.rgb,alpha);
`;
export const BAR_VERTEX = `
out vec3 chartLocal;
out vec3 chartNormal;
void materialVertex(){ippDefaultVertex();chartLocal=a_position;chartNormal=a_normal;}
`;
export const BAR_FRAGMENT = `
in vec3 chartLocal;
in vec3 chartNormal;
vec4 materialFragment(){
  vec3 normal=normalize(v_normal),n=abs(chartNormal);
  vec2 uv,faceSize;vec3 tangent,bitangent;
  if(n.y>.5){uv=vec2(chartLocal.x+.5,chartLocal.z+.5);faceSize=p_dimensions.xz*p_dimensions.w;tangent=vec3(1,0,0);bitangent=vec3(0,0,1);}
  else if(n.x>.5){uv=vec2(chartLocal.z+.5,.5-chartLocal.y);faceSize=p_dimensions.zy*p_dimensions.w;tangent=vec3(0,0,1);bitangent=vec3(0,-1,0);}
  else{uv=vec2(chartLocal.x+.5,.5-chartLocal.y);faceSize=p_dimensions.xy*p_dimensions.w;tangent=vec3(1,0,0);bitangent=vec3(0,-1,0);}
  vec3 toLight=normalize(p_light.xyz-v_position),viewDir=normalize(p_eye.xyz-v_position),halfLight=normalize(toLight+viewDir);
  vec3 surfacePoint=v_position;
  float response=.40+.60*max(dot(normal,toLight),0.0)*p_light.w;
  float shade=(n.y>.5?1.5:(n.x>.5?.44:.7))*(.70+.45*max(dot(normal,toLight),0.0));
  ${face}
  if(n.y>.5){
    float capFacet=smoothstep(-.14,.14,uv.x-uv.y)*pow(max(1.0-uv.x*.65-uv.y*.35,0.0),1.4);
    rgb+=p_accent.rgb*(.11+selected*.055+capFacet*.075*(.35+facetLight))*response*(1.0-rim);
    rgb+=pale*capFacet*.045*(.35+facetLight)*(1.0-rim);
    float capCorner=exp(-length((1.0-uv)*faceSize)/4.0);
    rgb+=pale*capCorner*.40*response;
  }
  return vec4(rgb,1.0);
}
`;
export const HALO_FRAGMENT = `
in vec3 chartLocal;
in vec3 chartNormal;
vec4 materialFragment(){
 vec2 p=vec2(chartLocal.x+.5,.5-chartLocal.y)*p_dimensions.xy;
 vec4 segments[6]=vec4[6](p_h0,p_h1,p_h2,p_h3,p_h4,p_h5);
 float side=-1e6,closest=1e6;
 for(int i=0;i<6;i++){
  vec2 a=segments[i].xy,b=segments[i].zw,v=b-a;
  side=max(side,dot(vec2(v.y,-v.x),p-a)/length(v));
  float t=clamp(dot(p-a,v)/dot(v,v),0.0,1.0);
  closest=min(closest,length(p-a-t*v));
 }
 if(side<0.0||closest>8.0)discard;
 return vec4(p_accent.rgb,exp(-closest/1.05)*(.012+p_profile.x*.14)*(1.0-smoothstep(6.0,8.0,closest)));
}
`;
export const BACK_FRAGMENT = `
vec4 materialFragment(){return vec4(p_tint.rgb,1.0);}
`;
