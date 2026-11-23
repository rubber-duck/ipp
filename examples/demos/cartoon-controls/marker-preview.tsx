import { Entity, ShaderAsset, PaintShader, assetRef } from "@ipp/react";
import { Box, Paint, Layout, Style, type GuiHsva } from "@ipp/react/gui";
import {
  Row,
  TextLine,
  Column,
  formatHex,
  linearColor,
} from "@ipp/react/gui-kit";

// The marker uses the colour picker's linear RGB and alpha, over a small
// landscape. A transparent choice reveals the background through its fabric.
const flag = `
vec2 p=position/size;
vec3 scene=mix(vec3(0.16,0.49,0.54),vec3(0.50,0.72,0.60),p.y);
float aa=1.0/size.y;
float ridge=0.75-0.33*(1.0-abs(2.0*fract(p.x*1.25+0.24)-1.0));
scene=mix(scene,vec3(0.075,0.27,0.30),smoothstep(ridge-aa,ridge+aa,p.y));
scene=mix(scene,vec3(0.042,0.19,0.19),smoothstep(0.83+0.07*sin(p.x*5.0)-aa,0.83+0.07*sin(p.x*5.0)+aa,p.y));
vec3 ink=vec3(0.012,0.023,0.045);
float pole=max(abs(position.x-0.36*size.x)-2.5,max(0.17-p.y,p.y-0.87)*size.y);
scene=mix(scene,ink,1.0-smoothstep(-0.6,0.6,pole));
vec2 f=vec2((p.x-0.36)/0.35,(p.y-0.25)/0.37);
float pennant=max(-f.x,max(-f.y+0.13*f.x,f.y-1.0+0.87*f.x));
float cloth=1.0-smoothstep(-aa,aa,pennant);
float inner=1.0-smoothstep(-0.065-aa,-0.065+aa,pennant);
scene=mix(scene,mix(ink,p_flag.rgb,inner),cloth*p_flag.a);
vec2 q=abs(position-size*0.5)-(size*0.5-8.0);
float d=length(max(q,0.0))+min(max(q.x,q.y),0.0)-8.0;
return vec4(mix(ink,scene,1.0-smoothstep(-2.4,-1.4,d)),1.0-smoothstep(-0.8,0.0,d));
`;
export function MarkerPreview({
  value,
  wide,
}: {
  value: GuiHsva;
  wide: boolean;
}) {
  const Group = wide ? Column : Row;
  return (
    <Group gap="label" fitWidth>
      <Entity id="marker-preview">
        <ShaderAsset
          id="marker-paint"
          recipe={{}}
          parameters={{ flag: "vec4" }}
        >
          <PaintShader>{flag}</PaintShader>
        </ShaderAsset>
        <Layout width={140} height={74} />
        <Style />
        <Box width={140} height={74} />
        <Paint source={assetRef("marker-paint")} flag={linearColor(value)} />
      </Entity>
      <Column gap="compact" fitWidth>
        <TextLine text="Waypoint flag" />
        <TextLine text={formatHex(value)} tone="neutral" size="small" />
      </Column>
    </Group>
  );
}
