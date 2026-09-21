vec4 materialFragment() {
  vec3 normal = normalize(ippSurfaceNormal());
  vec3 viewDirection = normalize(u_camera.xyz - v_position);
  float rim = pow(1.0 - abs(dot(normal, viewDirection)), 2.2);
  float pulse = 0.78 + 0.16 * sin(p_time * 2.094395) +
    0.06 * sin(p_time * 4.188790);
  float sparkle = pow(max(0.0, sin(p_time * 4.188790)), 12.0);
  vec3 core = p_color.rgb * (pulse + rim * 1.15 + sparkle * 0.7);
  return vec4(core, clamp(0.48 + rim * 0.42, 0.0, 1.0));
}
