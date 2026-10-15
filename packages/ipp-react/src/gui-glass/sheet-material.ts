import {
  OPTICAL_PARAMETERS,
  OPTICAL_PRESENTED_PAINT,
} from "./optical-material.js";

/** The same slab optics with size-independent corner and edge dimensions.
 * Native data lanes: hover, press, disabled, and part role (body/filled/lens/focus).
 * Feedback pins comparison samples without manufacturing runtime interaction.
 */
export const COMPONENT_PARAMETERS = {
  ...OPTICAL_PARAMETERS,
  geometry: "vec4", // corner radius, polished edge width, fill, lens
  feedback: "vec4", // pinned hover, press, disabled, focus
  continuation: "vec4", // shared sheet extension above/below this part; zw reserved
} as const;

export const COMPONENT_PAINT = OPTICAL_PRESENTED_PAINT.replace(
  "vec2 uv=position/size;",
  `// Adjacent native parts can sample one sheet instead of beveling their seam.
vec2 sheetPosition=position+vec2(0.0,p_continuation.x);
vec2 sheetSize=vec2(size.x,size.y+p_continuation.x+p_continuation.y);
vec2 uv=sheetPosition/sheetSize;
float hover=max(data.x,p_feedback.x);
float pressed=max(data.y,p_feedback.y);
float disabled=max(data.z,p_feedback.z);
float focus=max(step(3.5,data.w),p_feedback.w);
float selected=max(p_geometry.z,clamp(data.w,0.0,1.0)*(1.0-step(1.5,data.w)));
float lens=max(p_geometry.w,step(1.5,data.w)*(1.0-step(3.5,data.w)));
float insert=clamp(-data.w,0.0,1.0);
float radioDot=p_geometry.w<-.5 && p_geometry.w> -1.5 ? selected*(1.0-smoothstep(.19,.22,length((position-size*.5)/size.y))) : 0.0;
if(p_geometry.w<-.5) selected=0.0;
float finish=clamp(max(p_optics.y,lens*.36)+selected*.16+insert*.28+pressed*.10-hover*.05,.09,.85);`,
)
  .replace("float unit=max(size.y/80.0,.001);", "float unit=1.0;")
  .replace(
    "float radius=12.0*unit;",
    "float radius=min(p_geometry.x,min(size.x,size.y)*.5);",
  )
  .replace(
    "vec2 q=abs(position-size*.5)-(size*.5-radius);",
    `vec2 bodySize=sheetSize-vec2(0.0,p_geometry.w< -1.5?18.0:0.0);
vec2 q=abs(sheetPosition-bodySize*.5)-(bodySize*.5-radius);`,
  )
  .replace(
    "float aa=max(fwidth(contour),.45);",
    `float tailY=position.y-(size.y-22.0);
float tailContour=max((abs(position.x-size.x*.5)+tailY-20.0)*.7071068,-tailY)-1.0;
bool tailEdge=p_geometry.w< -1.5 && tailContour<contour;
if(p_geometry.w< -1.5) contour=min(contour,tailContour);
float aa=max(fwidth(contour),.45);`,
  )
  .replace(
    "vec2 outward=sign(position-size*.5);",
    "vec2 outward=sign(sheetPosition-bodySize*.5);",
  )
  .replace(
    "float bevel=clamp",
    "if(tailEdge)edgeNormal=normalize(vec2(sign(position.x-size.x*.5),1.0));\nfloat bevel=clamp",
  )
  .replace("max(5.5*p_optics.w,.1)", "max(min(p_geometry.y,size.y*.23),.1)")
  .replace(
    "vec3 point=vec3((position-size*.5)/size.y,0.0);",
    `vec2 lensXY=(position-size*.5)/max(size*.5,vec2(1));
if(lens>.5) N=normalize(mix(normalize(vec3(lensXY*.75,sqrt(max(1.0-dot(lensXY,lensXY)*.55,.02)))),N,smoothstep(.15,.80,bevel)));
vec3 point=vec3((sheetPosition-sheetSize*.5)/sheetSize.y,0.0);`,
  )
  .replace(
    "max(p_optics.y,length(fwidth(N))*.22)",
    "max(finish,length(fwidth(N))*.22)",
  )
  .replace(
    "float strip=rp.x+rp.y*.42;",
    "float strip=rp.x+rp.y*.42-hover*.13+pressed*.08;",
  )
  .replace(
    "// A finite absorbing slab.",
    `// Broad studio cards reveal the curved polished edge without a drawn rim.
float rimCard=pow(max(dot(R,normalize(vec3(-.68,-.28,-.68)+vec3(L.xy*.20,0))),0.0),12.0);
float topCard=pow(max(dot(R,normalize(vec3(.25,-.78,-.56)+vec3(L.xy*.15,0))),0.0),14.0);
environment+=vec3(.78,.91,1.0)*(rimCard*4.5+topCard*3.5);
// Small lenses resolve broad emitter reflections; fine window mullions would
// alias into scratches on a thumb only a few dozen pixels across.
float lensCard=pow(max(dot(R,normalize(vec3(-.45,-.60,.66)+vec3(L.xy*.20,0))),0.0),8.0);
environment=mix(environment,vec3(.02,.04,.05)+vec3(.8,.91,1.0)*(lensCard*3.5+rimCard*2.5+topCard*2.0),lens);
// A finite absorbing slab.`,
  )
  .replace(
    "*ghost*.45*(1.0-rough)*front",
    "*ghost*.45*(1.0-rough)*front*(1.0-lens)",
  )
  .replace(
    "vec3 reflected=environment*F+vec3(.85,.94,1.0)*min(direct,.65);",
    `vec3 reflected=environment*F*(1.0+hover*.3+focus*.7)+vec3(.85,.94,1.0)*min(direct,.65)*(1.0-lens*.85);
// A restrained tinted backing provides readable selected controls. The same
// rounded dielectric surface continues to reflect light over the backing.
body+=vec3(.17,.34,.39)*selected*(.72+.20*(1.0-uv.y));
body+=vec3(.055,.09,.105)*lens;
body+=vec3(.035,.047,.053)*insert;
body+=vec3(.30,.65,.72)*radioDot;
opacity=max(opacity,max(selected*.88,lens*.32));
opacity=max(opacity,max(radioDot,insert*.42));
// Focus uses another optical edge at the identical contour, never a stroked box.
if(data.w>3.5) coverage*=1.0-smoothstep(1.0,p_geometry.y+1.0,depth);
body*=1.0-disabled*.4;
reflected*=1.0-disabled*.7;
opacity*=1.0-disabled*.35;`,
  );
