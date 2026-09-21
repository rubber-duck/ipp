in vec2 studioUv;

vec4 materialFragment() {
  if (p_visible < 0.5) discard;
  float height = smoothstep(0.0, 1.0, studioUv.y);
  vec2 offset = (studioUv - vec2(0.46, 0.58)) * vec2(1.0, 0.8);
  float halo = exp(-dot(offset, offset) * 5.0) * 0.014;
  vec3 gray = mix(vec3(0.023, 0.028, 0.032), vec3(0.055, 0.063, 0.071), height);
  return vec4(gray + halo, 1.0);
}
