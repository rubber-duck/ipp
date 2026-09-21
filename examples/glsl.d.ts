/** Browser builds import GLSL source as text for shader declarations. */
declare module "*.glsl" {
  const source: string;
  export default source;
}
