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

// The same signed curved cap as the authored endpoint, expressed at the sample's projected radius.
float beamAlong(vec3 point) {
  float rho2 = point.x * point.x + p_curvature.y * point.y * point.y;
  float k = p_curvature.x;
  float capZ = p_depth.y - k * rho2 / (1.0 + sqrt(max(0.0, 1.0 - k * k * rho2)));
  return (point.z - p_depth.x) / (capZ - p_depth.x);
}

float projectionRays() {
  // Basis UVs follow corresponding near/far perimeter vertices in every pose.
  float phase = v_uv.x * 96.0;
  float distance = abs(fract(phase + 0.5) - 0.5);
  float footprint = max(fwidth(phase), 0.025);
  return (1.0 - smoothstep(0.035, 0.035 + footprint, distance)) * min(1.0, 0.1 / footprint);
}

// Cylinder/Sphere endpoint: k*(x*x [+ y*y] + z*z) + 2*z = 0,
// with z relative to the panel centre. Mesh geometry remains Blender-authored;
// this intersection only stops the density integral at that geometry's cap.
bool clipBeamCap(vec3 origin, vec3 direction, inout vec2 interval) {
  float k = p_curvature.x;
  if (abs(k) < 0.00001)
    return clipBeamPlane(vec3(0.0, 0.0, 1.0), p_depth.y, origin, direction, interval);
  origin.z -= p_depth.y;
  vec3 axes = vec3(1.0, p_curvature.y, 1.0);
  float a = k * dot(axes * direction, direction);
  float b = 2.0 * (k * dot(axes * origin, direction) + direction.z);
  float c = k * dot(axes * origin, origin) + 2.0 * origin.z;
  if (abs(a) < 0.000001) {
    if (abs(b) < 0.000001) return c <= 0.0;
    float root = -c / b;
    if (b > 0.0) interval.y = min(interval.y, root);
    else interval.x = max(interval.x, root);
  } else {
    float discriminant = b * b - 4.0 * a * c;
    if (discriminant < 0.0) return a < 0.0;
    float root = sqrt(max(0.0, discriminant));
    vec2 roots = vec2((-b - root) / (2.0 * a), (-b + root) / (2.0 * a));
    roots = vec2(min(roots.x, roots.y), max(roots.x, roots.y));
    if (a > 0.0) {
      interval.x = max(interval.x, roots.x);
      interval.y = min(interval.y, roots.y);
    } else if (interval.y <= roots.x + 0.00001) {
      interval.y = min(interval.y, roots.x);
    } else {
      interval.x = max(interval.x, roots.y);
    }
  }
  return interval.y > interval.x;
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
  // These side planes enclose the loft even when outside curvature brings
  // the far ring toward the lens. The actual curved cap clips the chord below.
  float extent = length(vec2(p_section.z, p_curvature.y * p_section.w));
  float sagitta = abs(p_curvature.x) < 0.00001 ? 0.0
    : (1.0 - cos(p_curvature.x * extent)) / abs(p_curvature.x);
  float nearFar = p_depth.y - sagitta;
  vec2 slope = (p_section.zw - p_section.xy) / (nearFar - p_depth.x);
  vec2 intercept = p_section.xy - slope * p_depth.x;
  bool chord = clipBeamPlane(vec3(1.0, 0.0, -slope.x), intercept.x, eye, direction, interval)
    && clipBeamPlane(vec3(-1.0, 0.0, -slope.x), intercept.x, eye, direction, interval)
    && clipBeamPlane(vec3(0.0, 1.0, -slope.y), intercept.y, eye, direction, interval)
    && clipBeamPlane(vec3(0.0, -1.0, -slope.y), intercept.y, eye, direction, interval)
    && clipBeamPlane(vec3(0.0, 0.0, -1.0), -p_depth.x, eye, direction, interval)
    && clipBeamPlane(vec3(0.0, 0.0, 1.0), p_depth.y + sagitta, eye, direction, interval)
    && clipBeamCap(eye, direction, interval);
  // The mesh's visible rays survive a zero-length volume chord at its edge.
  float stepLength = chord ? max(0.0, interval.y - interval.x) * 0.25 : 0.0;

  // Four midpoint samples of the soft-edged density along the visible chord
  // carry the view dependence; the slow spatial variation is taken once, at
  // the chord's middle.
  float densityIntegral = 0.0;
  for (int index = 0; index < 4; index++) {
    vec3 point = eye + direction * (interval.x + (float(index) + 0.5) * stepLength);
    float along = beamAlong(point);
    vec2 normalized = abs(point.xy / mix(p_section.xy, p_section.zw, clamp(along, 0.0, 1.0)));
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
