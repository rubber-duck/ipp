/** Rounded glass tube; native arc geometry owns sweep, end caps and coverage. */
export const LIQUID_ARC_PAINT = `
vec2 radial=position-size*.5;
float outerRadius=min(size.x,size.y)*.5;
vec2 outward=radial/max(length(radial),.001);
float tubeWidth=max(min(size.x,size.y)*(family==7.0?.0625:.11),1.0);
float across=clamp((outerRadius-length(radial))/tubeWidth,0.0,1.0);
float slope=cos(across*3.14159265)*1.32;
vec3 N=vec3(outward*sin(slope),cos(slope));
vec3 V=viewDirection; V.z=abs(V.z);
vec3 L=normalize(vec3((p_light.xy-.5)*6.0,p_light.z));
float energy=p_light.w;
if(p_lighting.x>.5 && sceneLightCount>0) {
  L=sceneLightDirection(0);
  energy*=clamp(dot(sceneLightRadiance(0),vec3(.2126,.7152,.0722)),0.0,2.0);
}
float eta=clamp(p_finish.x,1.01,2.5);
float f0=pow((eta-1.0)/(eta+1.0),2.0);
float fresnel=f0+(1.0-f0)*pow(1.0-clamp(dot(N,V),0.0,1.0),5.0);
vec3 R=reflect(-V,N);
float key=pow(max(dot(R,normalize(vec3(L.xy*.65,-.65))),0.0),5.0);
float sky=pow(max(dot(R,normalize(vec3(L.x*.35,-.72,.25))),0.0),3.0);
float activity=clamp(max(data.x,data.y),0.0,1.0);
float lit=step(.5,data.w);
float shoulder=exp(-pow((across-.30)/.18,2.0));
vec3 tint=mix(vec3(.024,.032,.043),color.rgb,lit*.42);
vec3 body=tint*(1.0-shoulder*.25);
float reflected=(fresnel*(.018+key*.26)+sky*.018)
  *(.65+energy*.35)*(1.0+activity*.6);
float alpha=mix(.48,.86,lit);
vec3 premult=body*alpha+vec3(.78,.86,1.0)*reflected;
alpha=max(alpha,max(premult.r,max(premult.g,premult.b)));
return vec4(premult/max(alpha,.001),alpha*(1.0-data.z*.35));
`;
