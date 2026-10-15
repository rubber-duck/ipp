export { LIQUID_PAINT } from "../../../../packages/ipp-react/src/gui-glass/liquid-material.js";

/** A fixed studio backdrop makes transmission visible without imitating blur. */
export const LIQUID_BACKDROP = `
vec2 uv=position/size;
float sky=exp(-dot((uv-vec2(.12,.0))*vec2(2,3),(uv-vec2(.12,.0))*vec2(2,3)));
float beam=exp(-pow((uv.x+uv.y*.25-.86)/.025,2.0));
float beamSoft=exp(-pow((uv.x+uv.y*.25-.86)/.08,2.0));
float horizon=exp(-pow((uv.y-.78)/.025,2.0));
vec3 backdrop=vec3(.003,.007,.012)+vec3(.006,.018,.038)*sky
  +vec3(.17,.19,.21)*beam+vec3(.025,.033,.04)*beamSoft
  +vec3(.01,.022,.027)*horizon;
return vec4(backdrop,1);
`;
