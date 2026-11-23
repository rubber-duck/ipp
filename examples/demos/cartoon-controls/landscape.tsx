import { Entity, ShaderAsset, PaintShader, assetRef } from "@ipp/react";
import { Box, Paint, Layout, Style } from "@ipp/react/gui";

// A single native paint draws the distant ridges and pines. The landscape is
// stationary while the gallery scrolls and keeps its proportions at any width.
const landscape = `
vec2 uv = position / max(size, vec2(1.0));
vec2 p = position / max(size.y,1.0);
vec3 sky = mix(vec3(0.12,0.48,0.56),vec3(0.58,0.79,0.69),uv.y);
float aa=max(fwidth(uv.y),0.0001);
float sun=length((uv-vec2(0.86,0.18))*vec2(size.x/size.y,1.0));
sky=mix(sky,vec3(0.97,0.81,0.45),1.0-smoothstep(0.062-aa,0.062+aa,sun));
float peaks=0.62-0.26*(1.0-abs(2.0*fract(p.x*1.46+0.16)-1.0))-0.05*(1.0-abs(2.0*fract(p.x*3.7)-1.0));
vec3 col=mix(sky,vec3(0.18,0.44,0.47),smoothstep(peaks-aa,peaks+aa,p.y));
float ridge=0.7+0.08*sin(5.6*p.x+0.6)+0.035*sin(13.0*p.x);
col=mix(col,vec3(0.09,0.32,0.34),smoothstep(ridge-aa,ridge+aa,p.y));
for(int layer=0;layer<2;layer++){
 float l=float(layer);
 float spacing=mix(0.05,0.085,l);
 float x=p.x/spacing+0.33*l;
 float cell=floor(x);
 float seed=fract(sin(cell*12.9898+l*4.1)*4375.5453);
 float base=0.78+l*0.16+0.045*sin(cell*spacing*4.5+l*2.5);
 float treeHeight=(0.055+0.115*seed)*(1.0+0.18*l);
 float y=(base-p.y)/treeHeight;
 float lateral=abs(fract(x)-0.5)*2.0;
 float crown = y>0.62 ? (1.0-y)*1.45 : y>0.33 ? (0.89-y)*1.39 : (0.67-y)*1.55;
 float tree=(y>0.0 && y<1.0)?1.0-smoothstep(crown-0.04,crown+0.04,lateral*1.43):0.0;
 float groundY=0.78+l*0.16+0.045*sin(p.x*4.5+l*2.5);
 float ground=smoothstep(groundY-aa,groundY+aa,p.y);
 col=mix(col,mix(vec3(0.054,0.24,0.24),vec3(0.027,0.141,0.16),l),max(ground,tree));
}
return vec4(col,1.0);
`;
export function Landscape({
  width,
  height,
}: {
  width: number;
  height: number;
}) {
  return (
    <Entity id="trail-landscape">
      <ShaderAsset id="trail-landscape-paint" recipe={{}} parameters={{}}>
        <PaintShader>{landscape}</PaintShader>
      </ShaderAsset>
      <Layout width={width} height={height} align_x={-1} align_y={-1} />
      <Style />
      <Box width={width} height={height} />
      <Paint source={assetRef("trail-landscape-paint")} />
    </Entity>
  );
}
