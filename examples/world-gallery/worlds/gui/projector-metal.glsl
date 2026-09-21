vec4 materialFragment() {
  vec4 baseSample = texture(p_base, v_uv);
  vec3 normal = normalize(ippSurfaceNormal());
  vec3 viewDirection = normalize(u_camera.xyz - v_position);
  float fresnel = pow(1.0 - max(0.0, dot(normal, viewDirection)), 5.0);
  float sky = normal.y * 0.5 + 0.5;
  vec3 ambient = mix(vec3(0.38, 0.41, 0.45), vec3(0.55, 0.6, 0.66), sky);
  vec3 metal = baseSample.rgb * ambient;
  vec3 reflectance = mix(vec3(0.18), baseSample.rgb, 0.45);
  for (int index = 0; index < 8; index++) {
    if (index >= u_light_count) break;
    vec3 direction = ippLightDirection(index, v_position);
    float incidence = max(0.0, dot(normal, direction));
    vec3 halfway = normalize(direction + viewDirection);
    float highlight = pow(max(0.0, dot(normal, halfway)), 96.0);
    vec3 radiance = ippLightRadiance(index, v_position);
    metal += radiance * incidence * (baseSample.rgb * 0.36 + reflectance * highlight * 2.0);
  }
  vec3 emissive = p_accent.rgb * (fresnel * 0.018 * p_energy);
  return vec4(metal + emissive, 1.0);
}
