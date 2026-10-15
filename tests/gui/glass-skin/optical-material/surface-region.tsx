import { assetRef } from "@ipp/react";
import { Paint, Skin } from "@ipp/react/gui";
import type { GuiKitContract } from "@ipp/react/gui-kit";

export const REGION_PARAMETERS = {
  surface: "vec4",
  divisions: "vec4",
} as const;

/** State treatment on an existing sheet, without another optical surface.
 * Surface = local origin within the sheet, then its full width/height.
 * Divisions = parent radius, leading vertical rule, bottom rule, reserved.
 * Native data = hover, press, disabled, selection (or 4 for focus).
 */
export const REGION_PAINT = `
vec2 sheetPosition=position+p_surface.xy;
float radius=p_divisions.x;
vec2 q=abs(sheetPosition-p_surface.zw*.5)-(p_surface.zw*.5-radius);
float contour=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
float aa=max(fwidth(contour),.45);
// Preserve the parent polished perimeter. Internal partitions have no bevel.
float interior=smoothstep(0.0,4.5,-contour);
float coverage=(1.0-smoothstep(-aa*.5,aa*.5,contour))*interior;
float selected=clamp(data.w,0.0,1.0);
float hover=clamp(data.x,0.0,1.0),pressed=clamp(data.y,0.0,1.0);
vec2 ruleWidth=max(fwidth(position),vec2(.75));
float vertical=(1.0-smoothstep(0.0,ruleWidth.x,position.x))*p_divisions.y;
float bottom=(1.0-smoothstep(0.0,ruleWidth.y,size.y-position.y))*p_divisions.z;
float tint=selected*.20+hover*.065+pressed*.04;
float alpha=max(tint,max(vertical,bottom)*.22);
vec3 regionColor=mix(vec3(.25,.40,.44),vec3(.43,.63,.68),max(vertical,bottom));
if(data.w>3.5) {
  float line=1.0-smoothstep(.65,.65+ruleWidth.y,abs(position.y-(size.y-4.0)));
  float ends=smoothstep(8.0,16.0,min(position.x,size.x-position.x));
  alpha=line*ends*.70;
  regionColor=vec3(.55,.78,.82);
}
return vec4(regionColor,coverage*alpha*(1.0-clamp(data.z,0.0,1.0)*.6));
`;

export function SurfaceRegion({
  contract,
  surface,
  leadingDivider = false,
  bottomDivider = false,
  radius = 12,
}: {
  contract: GuiKitContract;
  surface: readonly [number, number, number, number];
  leadingDivider?: boolean;
  bottomDivider?: boolean;
  radius?: number;
}) {
  const rows = ["background", "focusRing"].map((part) => ({
    part: contract.guiPaintPartIndex({ part }),
    paint: true,
  }));
  return (
    <>
      <Skin
        theme="component-theme-region"
        parts={contract.GuiSkin.encodeParts({
          nextSlot: rows.length,
          rows: new Map(rows.map((row, index) => [index, row])),
        })}
      />
      <Paint
        source={assetRef("component-region")}
        surface={surface}
        divisions={[radius, Number(leadingDivider), Number(bottomDivider), 0]}
      />
    </>
  );
}
