/** Art-directed light bands. No scene sampling, ray tracing or reflected scene render. */
export const STYLIZED_FACE = `
if(p_profile.z>0.0){
  float strength=p_profile.z;
  float softness=clamp(p_profile.w,.1,1.0);
  // The same field continues across adjacent cube faces; local normals vary its brightness.
  float shift=dot(toLight,normalize(vec3(.85,.12,.50)));
  float bend=sin(surfacePoint.y*3.7+surfacePoint.z*1.9)*.055;
  float stripe=surfacePoint.x*.75+surfacePoint.z*.48+surfacePoint.y*.10+bend;
  float ribbon=exp(-pow((stripe-shift*.85)/(mix(.18,.45,softness)),2.0));
  float second=exp(-pow((stripe+surfacePoint.y*.31+shift*.4-.68)/.44,2.0));
  float pool=exp(-pow((surfacePoint.y-2.30+shift*.32)/.44,2.0));
  float deep=exp(-pow((surfacePoint.y-.23-shift*.10)/.24,2.0));
  float grazing=pow(1.0-max(dot(normal,viewDir),0.0),3.0);
  vec3 sheen=mix(p_accent.rgb*vec3(.65,.76,1.05),pale,.06);
  rgb*=mix(1.0,.48,strength);
  rgb+=sheen*(ribbon*.16+second*.035+pool*.065+deep*.022)*response*strength;
  rgb+=pale*facet*(.045+.055*facetLight)*strength;
  float bottomFacet=pow(max(1.0-(inset.x+(1.0-uv.y)*faceSize.y)/(faceSize.x*.55),0.0),1.7);
  rgb+=sheen*bottomFacet*.035*response*strength;
  rgb+=p_accent.rgb*grazing*ribbon*.036*strength;
  // Concentrate the small glint where the moving ribbon reaches a bevel.
  float glint=pow(ribbon,3.0)*exp(-max(depth,0.0)/1.4);
  rgb+=pale*glint*(.14+.20*bevelLight)*strength;
}
`;

/** A painted contact sheen on one floor patch, not a mirrored scene. */
export const FLOOR_FRAGMENT = `
vec4 materialFragment(){
  vec2 p=v_position.xz;
  float front=max(p.y-.625,0.0),side=max(abs(p.x)-.70,0.0);
  float frontMask=smoothstep(.58,.68,p.y)*(1.0-smoothstep(.70,.84,abs(p.x)));
  float sideMask=smoothstep(.65,.75,abs(p.x))*(1.0-smoothstep(.57,.72,abs(p.y)));
  float fade=exp(-front*5.0)*frontMask+exp(-side*6.0)*sideMask;
  float bands=exp(-pow((abs(p.x)-.68)/.06,2.0))*.11+.022;
  float shift=p_light.x*.08;
  float pool=exp(-pow((p.x-shift)/.45,2.0))*.018;
  return vec4(p_accent.rgb,clamp(fade*(bands+pool)*p_profile.z,0.0,.14));
}
`;
