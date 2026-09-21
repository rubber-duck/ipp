float glowHash(vec2 p) {
  p = fract(p * vec2(123.34, 345.45));
  p += dot(p, p + 34.345);
  return fract(p.x * p.y);
}

vec4 materialFragment() {
  vec3 normal = normalize(ippSurfaceNormal());
  vec3 viewDirection = normalize(u_camera.xyz - v_position);
  float fresnel = pow(1.0 - abs(dot(normal, viewDirection)), 2.4);
  float bands = 0.82 + 0.18 * sin((v_uv.y * 96.0 + v_uv.x * 13.0) * 6.283185);
  float sparkle = step(0.985, glowHash(floor(v_uv * vec2(180.0, 120.0))));
  float luminance = p_energy * (0.62 + fresnel * 0.7 + bands * 0.12 + sparkle * 0.35);
  float alpha = clamp(0.64 + fresnel * 0.18 + sparkle * 0.06, 0.0, 0.9);
  return vec4(p_accent.rgb * luminance, alpha);
}
