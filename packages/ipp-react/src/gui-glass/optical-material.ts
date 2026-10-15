/**
 * Shared natural glass optics used by the production skin and optical study.
 * Dielectric Fresnel, GGX/Smith direct response and Beer–Lambert absorption.
 * Reflected/refracted rays see an analytic studio, never the scene framebuffer.
 * Positions/directions use +X right, +Y down, +Z toward the viewer.
 */
export const OPTICAL_PARAMETERS = {
  accent: "vec4",
  tint: "vec4",
  optics: "vec4", // IOR, perceptual roughness, thickness, rolled-edge width
  keyLight: "vec4", // local source position / height units, radiance
  eye: "vec4", // local eye position / height units; w reserved
  state: "vec4", // selection, use scene key light, diagnostic probe, reserved
} as const;

/** Shared dielectric response; callers supply the surface normal and view/light vectors. */
export const OPTICAL_RESPONSE = `
vec3 halfVector=V+L;
vec3 H=halfVector*inversesqrt(max(dot(halfVector,halfVector),1e-12));
float NoV=max(dot(N,V),.001),NoL=max(dot(N,L),0.0);
float NoH=max(dot(N,H),0.0),VoH=max(dot(V,H),0.0);
float rough=clamp(max(p_optics.y,length(fwidth(N))*.22),.09,.85);
float eta=clamp(p_optics.x,1.01,2.5);
float F0=pow((eta-1.0)/(eta+1.0),2.0);
float F=F0+(1.0-F0)*pow(1.0-NoV,5.0);
float Fh=F0+(1.0-F0)*pow(1.0-VoH,5.0);

// GGX normal distribution and correlated Smith visibility; finite area emitter
// broadens the lobe without bright pinholes on a pixel-sized bevel.
float alpha=max(rough*rough,.035);
float a2=alpha*alpha;
float denominator=NoH*NoH*(a2-1.0)+1.0;
float D=a2/(3.14159265*denominator*denominator);
float smith=.5/max(NoL*sqrt(NoV*NoV*(1.0-a2)+a2)
                    +NoV*sqrt(NoL*NoL*(1.0-a2)+a2),.001);
float direct=D*smith*Fh*NoL*max(p_keyLight.w,0.0);

// Two finite analytic softboxes sampled with the actual reflected ray.
// Source motion moves their frame; varying V/N moves reflection across the face.
vec3 R=reflect(-V,N);
vec3 studioForward=normalize(p_keyLight.xyz);
vec3 studioAxis=abs(studioForward.y)>.97?vec3(0,0,1):vec3(0,1,0);
vec3 studioRight=normalize(cross(studioAxis,studioForward));
vec3 studioUp=cross(studioForward,studioRight);
float rz=max(dot(R,studioForward),.15);
vec2 rp=vec2(dot(R,studioRight),dot(R,studioUp))/rz;
float blur=.025+rough*rough*.72;
float strip=rp.x+rp.y*.42;
float windowA=(1.0-smoothstep(.13,.13+blur,abs(strip+.34)))
  *(1.0-smoothstep(.75,.75+blur,abs(rp.y-.12)));
float windowB=(1.0-smoothstep(.045,.045+blur,abs(strip-.64)))
  *(1.0-smoothstep(1.15,1.15+blur,abs(rp.y+.2)));
float front= smoothstep(0.0,.2,dot(R,studioForward));
float windowFrame=1.0-smoothstep(.006,.012+blur*.16,abs(strip+.34));
float narrow=1.0-smoothstep(.009,.020+blur*.25,abs(strip-.36));
vec3 environment=vec3(.012,.023,.04)+vec3(.80,.91,1.0)
  *(windowA*(1.0-windowFrame*.70)*2.8+windowB*1.7+narrow*.9)
  *front/(1.0+rough*1.8);
// Off-camera studio cards become visible in the strongly curved edge. They
// form partial reflected highlights, rather than a line drawn around the shape.
float sideCard=pow(max(dot(R,normalize(vec3(-.65,.2,-.72))),0.0),mix(38.0,9.0,rough));
float ceiling=pow(max(dot(R,normalize(vec3(.12,-.85,.51))),0.0),mix(32.0,8.0,rough));
environment+=vec3(.76,.91,1.0)*(sideCard*2.1+ceiling*1.3);

// A finite absorbing slab. Straight-through transmission uses ordinary alpha
// composition; the refracted ray only shifts the analytic internal reflection.
// This intentionally does not sample or blur the scene behind the button.
vec3 T=refract(-V,N,1.0/eta);
float thickness=max(p_optics.z,.01)*(1.0+bevel*.65);
float path=thickness/max(abs(T.z),.15);
vec3 absorption=-log(clamp(p_tint.rgb,vec3(.02),vec3(.99)))*.26;
vec3 transmission=exp(-absorption*path);
float transmitted=dot(transmission,vec3(.2126,.7152,.0722))*(1.0-F);
// Frost is an inexpensive translucent scattering approximation, not backdrop blur.
float haze=rough*rough*.65;
float opacity=clamp(1.0-transmitted+haze,0.0,1.0);
vec3 scatter=mix(p_tint.rgb,vec3(.36,.46,.50),rough)*haze*.16;
vec3 body=scatter+p_accent.rgb*p_state.x*.008*(1.0-F);
// The weak rear-surface reflection separates from the front reflection with
// optical thickness. Absorption and frost suppress the secondary image.
float ghostStrip=strip+T.x*path*.32+T.y*path*.14+.065;
float ghost=(1.0-smoothstep(.13,.13+blur*1.8,abs(ghostStrip+.34)))
  *(1.0-smoothstep(.75,.75+blur,abs(rp.y-.12+T.y*path*.2)));
body+=transmission*transmission*F0*(1.0-F)*ghost*.45*(1.0-rough)*front;
vec3 reflected=environment*F+vec3(.85,.94,1.0)*min(direct,.65);
// Preserve reflected energy with the renderer's bounded straight-alpha output.
opacity=max(opacity,max(reflected.r,max(reflected.g,reflected.b)));
return vec4((body+reflected)/max(opacity,.001),coverage*clamp(opacity,0.0,1.0));
`;

export const OPTICAL_PAINT = `
vec2 uv=position/size;
float unit=max(size.y/80.0,.001);
float radius=12.0*unit;
vec2 q=abs(position-size*.5)-(size*.5-radius);
float contour=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
float aa=max(fwidth(contour),.45);
float coverage=1.0-smoothstep(-aa*.5,aa*.5,contour);
float depth=max(-contour,0.0)/unit;

// A flat sheet with a rounded perimeter and a polished, rolled cross-section.
// No corner shards, painted rim, outline or emissive border.
vec2 outward=sign(position-size*.5);
vec2 edgeNormal=q.x>q.y?vec2(outward.x,0):vec2(0,outward.y);
if(min(q.x,q.y)>0.0)edgeNormal=normalize(q)*outward;
float bevel=clamp(1.0-depth/max(5.5*p_optics.w,.1),0.0,1.0);
float slope=sin(bevel*1.48);
vec3 N=vec3(edgeNormal*slope,sqrt(max(1.0-slope*slope,.001)));
vec3 point=vec3((position-size*.5)/size.y,0.0);
vec3 V=normalize(p_eye.xyz-point);
vec3 L=normalize(p_keyLight.xyz-point);
${OPTICAL_RESPONSE}
`;

/** Same optical model, with the real presenting view and opt-in scene key light. */
export const OPTICAL_PRESENTED_PAINT = OPTICAL_PAINT.replace(
  "vec3 V=normalize(p_eye.xyz-point);",
  "vec3 V=viewDirection; V.z=abs(V.z);",
)
  .replace(
    "vec3 L=normalize(p_keyLight.xyz-point);",
    `
vec3 L=normalize(p_keyLight.xyz-point);
float sceneEnergy=1.0;
if(p_state.y>.5 && sceneLightCount>0) {
    L=sceneLightDirection(0);
    sceneEnergy=dot(sceneLightRadiance(0),vec3(.2126,.7152,.0722));
}
if(p_state.z>2.5) return vec4(sceneLightRadiance(0)*.1,1);
if(p_state.z>1.5) return vec4(sceneLightDirection(0)*.5+.5,1);
if(p_state.z>.5) return vec4(viewDirection*.5+.5,1);
`,
  )
  .replace("*max(p_keyLight.w,0.0)", "*max(p_keyLight.w,0.0)*sceneEnergy")
  .replace("normalize(p_keyLight.xyz);", "L;");
