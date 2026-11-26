import { ShaderAsset } from "../assets/declarations.js";
import { FragmentShader, PaintShader, VertexShader } from "../shaders.js";

// Both surfaces print the same four-square engineering grid. Fade a line family
// as its projected spacing approaches a pixel, especially on an oblique tile.
const grid = `
vec2 spacing = max(fwidth(squares), vec2(.001));
vec2 d = abs(fract(squares + .5) - .5) / spacing;
vec2 fine = (1.0 - smoothstep(vec2(.10), vec2(.88), d)) * smoothstep(vec2(2.), vec2(5.), 1. / spacing);
vec2 majorDistance = abs(fract(squares * .25 + .5) - .5) * 4. / spacing;
vec2 major = (1.0 - smoothstep(vec2(.20), vec2(1.05), majorDistance)) * smoothstep(vec2(2.), vec2(5.), 4. / spacing);
vec3 paper = mix(vec3(.021, .093, .215), vec3(.046, .180, .336), wash);
paper = mix(paper, vec3(.40, .67, .84), .051 * max(fine.x, fine.y) + .10 * max(major.x, major.y));
`;

export function cartoonPaperAssets(scope: string) {
  return (
    <>
      <ShaderAsset id={`${scope}/chart-paper`} recipe={{}} parameters={{}}>
        <PaintShader backend="ipp">{`
          vec2 squares = position / 18.;
          float wash = clamp(1. - position.y / max(size.y, 1.), 0., 1.);
          ${grid}
          float inset = min(min(position.x, size.x-position.x), min(position.y, size.y-position.y));
          return vec4(mix(vec3(.012, .045, .108), paper, smoothstep(1., 2., inset)), color.a);
        `}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id={`${scope}/chart-base`}
        recipe={{ normals: true }}
        parameters={{}}
      >
        <VertexShader backend="ipp" requiredAttributes={7}>{`
          out vec3 paperLocal;
          out float paperFace;
          void materialVertex() {
            ippDefaultVertex();
            paperLocal = a_position;
            paperFace = a_normal.y;
          }
        `}</VertexShader>
        <FragmentShader backend="ipp" requiredAttributes={7}>{`
          in vec3 paperLocal;
          in float paperFace;
          vec4 materialFragment() {
            if (!ippFrontFacing() || paperFace < -.5) ippDiscard();
            vec2 squares = paperLocal.xz * 2.;
            float wash = .3 + .65 * (1. - v_uv.y);
            ${grid}
            return vec4(paperFace > .5 ? paper : vec3(.019, .071, .150), 1.);
          }
        `}</FragmentShader>
      </ShaderAsset>
    </>
  );
}
