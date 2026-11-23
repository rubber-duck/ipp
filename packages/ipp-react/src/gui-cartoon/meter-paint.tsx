import { assetRef, ShaderAsset } from "../assets/declarations.js";
import { PaintShader } from "../shaders.js";
import { ink } from "./palette.js";

// Native box and arc geometry clip these faces, including partial segments,
// the turning spinner and the pulse at the leading edge of a progress meter.
const meterPaint = `
vec2 uv=position/max(size,vec2(0.001));
if(data.x<0.5){
 float upper=1.0-smoothstep(0.18,0.25,uv.y);
 float lower=smoothstep(0.66,0.73,uv.y);
 vec3 face=mix(color.rgb,color.rgb*0.60,lower);
 return vec4(mix(face,mix(color.rgb,vec3(1.0),0.34),upper),color.a);
}
vec2 n=normalize(position-0.5*size+vec2(0.0001));
float lit=smoothstep(0.72,0.82,dot(n,vec2(-0.62,-0.78)));
float bevel=smoothstep(0.35,0.48,dot(n,vec2(0.48,0.88)));
vec3 face=mix(color.rgb,color.rgb*0.58,bevel);
face=mix(face,mix(color.rgb,vec3(1.0),0.45),lit);
float weight=clamp(min(size.x,size.y)*0.020,0.35,1.55);
return vec4(mix(face,p_ink.rgb,smoothstep(-weight-0.40,-weight+0.40,edge)),color.a);
`;
export const meterAsset = (scope: string) => (
  <ShaderAsset
    id={`${scope}/meter`}
    recipe={{ presentation: true }}
    parameters={{ ink: "vec4" }}
  >
    <PaintShader>{meterPaint}</PaintShader>
  </ShaderAsset>
);
export const meterAppearance = (scope: string) => ({
  source: assetRef(`${scope}/meter`),
  ink,
});
