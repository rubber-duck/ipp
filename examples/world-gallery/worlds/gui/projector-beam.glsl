in vec3 beamLocalPosition;
flat in vec3 beamWorldOrigin;
flat in mat3 beamWorldToLocal;

bool clipBeamPlane(vec3 normal, float limit, vec3 origin, vec3 direction, inout vec2 interval) {
  float distance = limit - dot(normal, origin);
  float slope = dot(normal, direction);
  if (abs(slope) < 0.00001) return distance >= 0.0;
  float crossing = distance / slope;
  if (slope > 0.0) interval.y = min(interval.y, crossing);
  else interval.x = max(interval.x, crossing);
  return interval.y > interval.x;
}

vec2 beamHalfSize(float z) {
  return mix(p_section.xy, p_section.zw, clamp((z - p_depth.x) / (p_depth.y - p_depth.x), 0.0, 1.0));
}

float projectionRays() {
  vec2 perimeter = beamLocalPosition.xy / beamHalfSize(beamLocalPosition.z);
  vec2 square = perimeter / max(abs(perimeter.x), abs(perimeter.y));
  float coordinate;
  if (abs(square.x) > abs(square.y)) {
    coordinate = square.x > 0.0
      ? (square.y + 1.0) * 0.125
      : 0.5 + (1.0 - square.y) * 0.125;
  } else {
    coordinate = square.y > 0.0
      ? 0.25 + (1.0 - square.x) * 0.125
      : 0.75 + (1.0 + square.x) * 0.125;
  }
  float phase = coordinate * 96.0;
  float distance = abs(fract(phase + 0.5) - 0.5);
  float footprint = max(fwidth(phase), 0.025);
  return (1.0 - smoothstep(0.035, 0.035 + footprint, distance)) * min(1.0, 0.1 / footprint);
}

// The beam shades every pixel it covers, so it keeps per-pixel work to the
// projection rays and a short density integral. Dust motes are separate
// sprites (projector-dust-vertex.glsl) rather than a per-pixel search.
vec4 materialFragment() {
  float rays = projectionRays() * p_energy * 0.075;
  // The authored open mesh has inward winding: its front faces are the exit
  // boundary, so each camera ray contributes once rather than at both walls.
  if (!gl_FrontFacing) discard;
  vec3 eye = beamWorldToLocal * (u_camera.xyz - beamWorldOrigin);
  vec3 delta = beamLocalPosition - eye;
  float distanceToExit = length(delta);
  if (distanceToExit < 0.0001) discard;
  vec3 direction = delta / distanceToExit;
  if (u_camera.w > 0.5) {
    direction = normalize(beamWorldToLocal * -u_camera.xyz);
    distanceToExit = (p_depth.y - p_depth.x) * 4.0;
    eye = beamLocalPosition - direction * distanceToExit;
  }
  vec2 interval = vec2(0.0, distanceToExit);
  vec2 slope = (p_section.zw - p_section.xy) / (p_depth.y - p_depth.x);
  vec2 intercept = p_section.xy - slope * p_depth.x;
  if (!clipBeamPlane(vec3(1.0, 0.0, -slope.x), intercept.x, eye, direction, interval)
    || !clipBeamPlane(vec3(-1.0, 0.0, -slope.x), intercept.x, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, 1.0, -slope.y), intercept.y, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, -1.0, -slope.y), intercept.y, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, 0.0, -1.0), -p_depth.x, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, 0.0, 1.0), p_depth.y, eye, direction, interval)) discard;
  float stepLength = (interval.y - interval.x) * 0.25;
  if (stepLength <= 0.00001) discard;

  // Four midpoint samples of the soft-edged density along the visible chord
  // carry the view dependence; the slow spatial variation is taken once, at
  // the chord's middle.
  float densityIntegral = 0.0;
  for (int index = 0; index < 4; index++) {
    vec3 point = eye + direction * (interval.x + (float(index) + 0.5) * stepLength);
    float along = (point.z - p_depth.x) / (p_depth.y - p_depth.x);
    vec2 normalized = abs(point.xy / beamHalfSize(point.z));
    float edge = 1.0 - smoothstep(0.68, 1.0, max(normalized.x, normalized.y));
    float ends = smoothstep(0.0, 0.08, along) * (1.0 - smoothstep(0.9, 1.0, along));
    densityIntegral += edge * ends * mix(1.2, 0.65, along);
  }
  vec3 middle = eye + direction * (0.5 * (interval.x + interval.y));
  float variation = 0.9 + 0.1 * sin(dot(middle, vec3(2.1, 3.3, 1.7)))
    * sin(dot(middle, vec3(-1.4, 2.7, 2.2)));
  float volume = 1.0 - exp(-densityIntegral * stepLength * variation * p_energy * 0.2);
  float alpha = 1.0 - (1.0 - volume) * (1.0 - rays);
  vec3 color = p_accent.rgb * mix(0.58, 1.0, clamp(rays, 0.0, 1.0));
  return vec4(color, alpha);
}
