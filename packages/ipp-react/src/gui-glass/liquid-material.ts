/** Pixel area under the local tangent; shared by the panel and optical samples. */
const LIQUID_COVERAGE = `
float major=max(max(gradient.x,gradient.y),.00001);
float minorGradient=min(gradient.x,gradient.y);
float support=(major+minorGradient)*.5;
float tail=max(support-abs(d),0.0);
tail=tail*tail/max(2.0*major*minorGradient,.00000001);
float coverage=abs(d)<=(major-minorGradient)*.5
  ? clamp(.5-d/major,0.0,1.0) : (d>0.0?tail:1.0-tail);
`;

/** Liquid-glass optical model: a rounded meniscus over a smoky, non-emissive body.
 * Shared by the study and production Glass skin. No backdrop sampling.
 */
export const LIQUID_PAINT = `
vec2 sheetSize=size+vec2(0.0,p_continuation.x+p_continuation.y);
vec2 sheetPosition=position+vec2(0.0,p_continuation.x);
float minor=min(sheetSize.x,sheetSize.y);
if(p_geometry.w<-.5) {
  sheetPosition-=(sheetSize-vec2(minor))*.5;
  sheetSize=vec2(minor);
}
float radius=min(p_geometry.x,minor*.5);
vec2 q=abs(sheetPosition-sheetSize*.5)-(sheetSize*.5-radius);
float d=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
vec2 pixelX=dFdx(sheetPosition),pixelY=dFdy(sheetPosition);
vec2 gradient=abs(vec2(dFdx(d),dFdy(d)));
${LIQUID_COVERAGE}
// The renderer applies its native contour after the paint. Supply only the
// additional silhouette mask, rather than attenuating a shared edge twice.
float nativePixel=max(length(vec2(dFdx(edge),dFdy(edge))),.00001);
float nativeCoverage=clamp(.5-edge/nativePixel,0.0,1.0);
coverage=clamp(coverage/max(nativeCoverage,.00001),0.0,1.0);
float depth=max(-d,0.0);
if(coverage<=0.0) return vec4(0.0);
vec3 V=viewDirection; V.z=abs(V.z);
vec3 L=normalize(p_keyLight.xyz);
float energy=p_keyLight.w;
if(p_state.y>.5 && sceneLightCount>0) {
  L=sceneLightDirection(0);
  energy*=clamp(dot(sceneLightRadiance(0),vec3(.2126,.7152,.0722)),.0,2.0);
}
float hover=max(data.x,p_feedback.x),pressed=max(data.y,p_feedback.y);
float disabled=max(data.z,p_feedback.z),focus=max(step(3.5,data.w),p_feedback.w);
float activity=clamp(max(hover,focus),0.0,1.0);
float selected=max(p_geometry.z,clamp(data.w,0.0,1.0)*(1.0-step(1.5,data.w)));
float lens=max(p_geometry.w,step(1.5,data.w)*(1.0-step(3.5,data.w)));
float eta=clamp(p_optics.x,1.01,2.5);
float f0=pow((eta-1.0)/(eta+1.0),2.0);
// A pixel contains changing normals as well as a fractional silhouette. Average
// premultiplied radiance with coverage, so a thin highlight contributes even when
// the pixel center misses it. Flat interiors retain a single optical evaluation.
float rimWidth=clamp(minor*.04,1.25,5.0)*p_optics.w;
float grid=depth<6.0*rimWidth+length(pixelX)+length(pixelY)?4.0:1.0;
vec4 integrated=vec4(0.0);
for(int sy=0;sy<4;sy++) {
  if(float(sy)>=grid) break;
  for(int sx=0;sx<4;sx++) {
    if(float(sx)>=grid) break;
    vec2 offset=(vec2(float(sx),float(sy))+.5)/grid-.5;
    vec2 samplePosition=sheetPosition+pixelX*offset.x+pixelY*offset.y;
    vec2 q=abs(samplePosition-sheetSize*.5)-(sheetSize*.5-radius);
    float d=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
    float depth=max(-d,0.0);
    vec2 outward=sign(samplePosition-sheetSize*.5);
    vec2 edgeNormal=q.x>q.y?vec2(outward.x,0):vec2(0,outward.y);
    if(min(q.x,q.y)>0.0) edgeNormal=normalize(max(q,vec2(.0001)))*outward;
    vec2 gradient=abs(vec2(dot(edgeNormal,pixelX),dot(edgeNormal,pixelY)))/grid;
    ${LIQUID_COVERAGE}
    float edgeWidth=rimWidth;
    // Rounded meniscus: nearly flat interior, steep tangent only at the perimeter.
    float roll=exp(-depth/max(edgeWidth,.1));
    float slope=roll*1.46;
    vec3 N=vec3(edgeNormal*sin(slope),cos(slope));
    float NoV=clamp(dot(N,V),.0,1.0);
    float fresnel=f0+(1.0-f0)*pow(1.0-NoV,5.0);
    vec3 R=reflect(-V,N);
    // Smooth, finite area lights. No sharp studio stripes or white face streaks.
    float key=pow(max(dot(R,normalize(vec3(L.xy*.65,-.65))),0.0),5.0);
    float sky=pow(max(dot(R,normalize(vec3(L.x*.35,-.72,.25))),0.0),3.0);
    float fill=pow(max(dot(R,normalize(vec3(-L.xy*.45,-.72))),0.0),6.0);
    float reflected=(fresnel*(.014+key*.60+fill*.12)+sky*.020*roll)
    *(.65+energy*.35)*(1.0+activity*.65);
    // A narrow bright tangent and a darker inner shoulder read as thickness,
    // rather than an emissive outline. Both remain inside actual shape coverage.
    // Filter narrow bands by the pixel's variance, preserving their integrated energy.
    // This keeps a crisp rim from sparkling as its size or pixel phase changes.
    float pixelVariance=dot(gradient,gradient)/6.0;
    float tangentWidth=sqrt(.35*.35+pixelVariance);
    float tangent=exp(-pow((depth-.5)/tangentWidth,2.0))*.35/tangentWidth;
    float direction=.12+.88*pow(max(dot(edgeNormal,normalize(L.xy+vec2(.001))),0.0),2.0);
    reflected+=tangent*direction*.09*(1.0+activity*.45);
    float shoulderWidth=sqrt(.65*.65+pixelVariance);
    float shoulder=exp(-pow((depth-1.5)/shoulderWidth,2.0))*.65/shoulderWidth*.30;
    vec2 uv=samplePosition/sheetSize;
    float cloud=exp(-dot((uv-vec2(.25+L.x*.15,.08))*vec2(1.3,1.7),(uv-vec2(.25+L.x*.15,.08))*vec2(1.3,1.7)));
    vec3 tint=mix(vec3(.023,.028,.037),p_tint.rgb,.014);
    // Face reflections are optional; the meniscus and direct light keep their shape.
    vec3 body=tint+vec3(.014,.016,.022)*cloud*clamp(p_accent.a,0.0,1.0);
    body*=1.0-shoulder;
    body+=p_accent.rgb*selected*.035+vec3(.018)*activity;
    body*=1.0-pressed*.12;
    float opacity=clamp(.68+p_optics.y*.16+selected*.06+pressed*.03,.0,.94);
    // Small native thumbs are gently milky lenses, with no clipping-white gloss.
    body=mix(body,vec3(.34,.40,.44),clamp(lens,0.0,1.0)*.65);
    opacity=mix(opacity,.93,clamp(lens,0.0,1.0));
    float radio=p_geometry.w<-.5?selected*(1.0-smoothstep(.15,.19,length((samplePosition-sheetSize*.5)/sheetSize.y))):0.0;
    body=mix(body,mix(vec3(.68,.80,.86),p_accent.rgb,.35),radio);
    if(minor<18.0 && p_geometry.z>.5) { body=mix(vec3(.56,.65,.71),p_accent.rgb,.35); opacity=1.0; }
    if(data.w>3.5) coverage*=1.0-smoothstep(1.2,3.5,depth);
    vec3 premult=body*opacity+vec3(.78,.86,1.0)*reflected*(1.0-disabled*.65);
    opacity=max(opacity,max(premult.r,max(premult.g,premult.b)));
    float weight=coverage*(1.0-disabled*.35)/(grid*grid);
    integrated+=vec4(premult,opacity)*weight;
  }
}
return vec4(integrated.rgb/max(integrated.a,.00001),
  integrated.a/max(nativeCoverage,.00001));
`;
