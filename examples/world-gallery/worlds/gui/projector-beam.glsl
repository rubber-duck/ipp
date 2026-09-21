in vec3 beamLocalPosition;
flat in vec3 beamWorldOrigin;
flat in mat3 beamWorldToLocal;

float beamHash(vec2 p) {
  p = fract(p * vec2(443.8975, 397.2973));
  p += dot(p, p.yx + 19.19);
  return fract(p.x * p.y);
}

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
  float angularFootprint = max(length(dFdx(direction)), length(dFdy(direction)));
  float orthographicFootprint = max(length(dFdx(beamLocalPosition)), length(dFdy(beamLocalPosition)));
  vec2 interval = vec2(0.0, distanceToExit);
  vec2 slope = (p_section.zw - p_section.xy) / (p_depth.y - p_depth.x);
  vec2 intercept = p_section.xy - slope * p_depth.x;
  if (!clipBeamPlane(vec3(1.0, 0.0, -slope.x), intercept.x, eye, direction, interval)
    || !clipBeamPlane(vec3(-1.0, 0.0, -slope.x), intercept.x, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, 1.0, -slope.y), intercept.y, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, -1.0, -slope.y), intercept.y, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, 0.0, -1.0), -p_depth.x, eye, direction, interval)
    || !clipBeamPlane(vec3(0.0, 0.0, 1.0), p_depth.y, eye, direction, interval)) discard;
  float stepLength = (interval.y - interval.x) / 8.0;
  if (stepLength <= 0.00001) discard;
  float densityIntegral = 0.0;
  for (int index = 0; index < 8; index++) {
    vec3 point = eye + direction * (interval.x + (float(index) + 0.5) * stepLength);
    float along = (point.z - p_depth.x) / (p_depth.y - p_depth.x);
    vec2 normalized = abs(point.xy / beamHalfSize(point.z));
    float edge = 1.0 - smoothstep(0.68, 1.0, max(normalized.x, normalized.y));
    float ends = smoothstep(0.0, 0.08, along) * (1.0 - smoothstep(0.9, 1.0, along));
    float variation = 0.9 + 0.1 * sin(dot(point, vec3(2.1, 3.3, 1.7)))
      * sin(dot(point, vec3(-1.4, 2.7, 2.2)));
    densityIntegral += edge * ends * variation * mix(1.2, 0.65, along) * stepLength;
  }
  float volume = 1.0 - exp(-densityIntegral * p_energy * 0.2);

  // A small fixed population lives in local 3D space. Its periodic Host phase
  // drifts continuously through the loop; analytic ray distance keeps parallax.
  float dust = 0.0;
  for (int index = 0; index < 18; index++) {
    float seed = float(index) + 1.0;
    float z = mix(p_depth.x + 0.25, p_depth.y - 0.25, beamHash(vec2(seed, 7.0)))
      + 0.08 * sin(p_phase + seed);
    vec2 unit = vec2(beamHash(vec2(seed, 1.0)), beamHash(vec2(seed, 3.0))) * 1.4 - 0.7;
    unit += 0.06 * vec2(sin(p_phase + seed * 1.7), cos(p_phase + seed * 2.3));
    vec3 center = vec3(unit * beamHalfSize(z), z);
    float alongRay = dot(center - eye, direction);
    if (alongRay < interval.x || alongRay > interval.y) continue;
    vec3 offset = center - (eye + direction * alongRay);
    float radius = mix(0.0024, 0.0046, beamHash(vec2(seed, 9.0)));
    float pixelFootprint = u_camera.w > 0.5 ? orthographicFootprint : alongRay * angularFootprint;
    float antialiasedRadius = max(radius, pixelFootprint * 0.4);
    float spot = exp(-dot(offset, offset) / (antialiasedRadius * antialiasedRadius));
    dust += spot * radius * radius / (antialiasedRadius * antialiasedRadius);
  }
  float motes = 1.0 - exp(-dust * p_energy * 2.8);
  float alpha = 1.0 - (1.0 - volume) * (1.0 - motes) * (1.0 - rays);
  vec3 color = p_accent.rgb * mix(0.58, 1.0, clamp(motes + rays, 0.0, 1.0));
  return vec4(color, alpha);
}
