/** Assemble renderer shader sources for device probes, as the Rust build does. */
async function browserShaderFile(name: string): Promise<string> {
  const response = await fetch(
    `/crates/ipp-render-gl/src/services/render/shaders/${name}`,
  );
  if (!response.ok) throw new Error(`Missing shader ${name}`);
  return response.text();
}

export async function renderShaderSource(
  name: string,
  read: (name: string) => Promise<string> = browserShaderFile,
): Promise<string> {
  let source = await read(name);
  for (const [marker, file] of [
    ["// SURFACE_INTERSECTION_VERTEX", "surface-intersection-vertex.glsl"],
    ["// SURFACE_INTERSECTION_FRAGMENT", "surface-intersection-fragment.glsl"],
  ] as const) {
    if (source.includes(marker))
      source = source.replaceAll(marker, await read(file));
  }
  return source;
}
