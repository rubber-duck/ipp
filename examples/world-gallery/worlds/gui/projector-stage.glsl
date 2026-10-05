// Baked textures decode to linear colour. Selection multiplies their radiance,
// independently of the bounded RGB tint fields on UnlitMaterial.
vec4 materialFragment() {
  vec4 baked = texture(p_base, v_uv);
  return vec4(baked.rgb * p_gain, baked.a);
}
