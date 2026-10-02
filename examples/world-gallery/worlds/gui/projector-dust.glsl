flat in vec4 dustMote;
in vec3 dustLocalPosition;
flat in vec3 dustWorldOrigin;
flat in mat3 dustWorldToLocal;

// The camera ray through this pixel passes the mote at an analytic distance,
// which keeps parallax exact; a footprint-sized radius keeps sub-pixel motes
// antialiased with the same total brightness.
vec4 materialFragment() {
  vec3 eye = dustWorldToLocal * (u_camera.xyz - dustWorldOrigin);
  bool orthographic = u_camera.w > 0.5;
  vec3 direction = orthographic
    ? normalize(dustWorldToLocal * -u_camera.xyz)
    : normalize(dustLocalPosition - eye);
  float footprint = orthographic
    ? max(length(dFdx(dustLocalPosition)), length(dFdy(dustLocalPosition)))
    : distance(dustMote.xyz, eye) * max(length(dFdx(direction)), length(dFdy(direction)));
  vec3 toMote = dustMote.xyz - dustLocalPosition;
  vec3 offset = toMote - direction * dot(toMote, direction);
  float radius = dustMote.w;
  float antialiasedRadius = max(radius, footprint * 0.4);
  float inverseArea = 1.0 / (antialiasedRadius * antialiasedRadius);
  float dust = exp(-dot(offset, offset) * inverseArea) * radius * radius * inverseArea;
  return vec4(p_accent.rgb, 1.0 - exp(-dust * p_energy * 2.8));
}
