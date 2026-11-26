import { ShaderAsset } from "../assets/declarations.js";
import { PaintShader } from "../shaders.js";

/** A dry pencil stroke in axis-local coordinates, in Canvas units or metres. */
export function pencilAxisAsset(scope: string) {
  return (
    <ShaderAsset
      id={`${scope}/pencil-axis`}
      recipe={{}}
      parameters={{ pressure: "f32" }}
    >
      <PaintShader backend="ipp">{`
        bool horizontal = size.x >= size.y;
        vec2 point = horizontal ? position : position.yx;
        float width = max(min(size.x, size.y), .00001);
        point = vec2(point.x, point.y - width * .5) * (5. / width);
        float grain = fract(52.9829189 * fract(dot(floor(point * 1.4), vec2(.06711056, .00583715))));
        float phase = horizontal ? .7 : 2.3;
        float along = point.x;
        float pressure = .44 * sin(along * .084 + phase) + .19 * sin(along * .29 + phase);
        float weight = 1.06 + p_pressure * (.26 * sin(along * .13 - phase) + .15 * (grain - .5));
        float strokeDistance = abs(point.y - pressure * p_pressure) - weight;
        float aa = max(.55 * fwidth(point.y), .05);
        float coverage = 1. - smoothstep(-aa, aa, strokeDistance);
        float tooth = grain * grain * (1. - smoothstep(.85, 2., fwidth(point.x)));
        return vec4(color.rgb * (1. - .09 * tooth), color.a * coverage);
      `}</PaintShader>
    </ShaderAsset>
  );
}
