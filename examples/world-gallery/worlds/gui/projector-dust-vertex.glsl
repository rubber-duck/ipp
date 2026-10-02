// Dust motes are camera-facing sprites, so only the few pixels around each
// mote are shaded. The dust entity draws the beam's frustum mesh purely as a
// vertex source: its indices are sequential (build_projector.py checks this),
// so vertices 6m..6m+5 form the two triangles of mote m's quad and the
// remaining vertices collapse outside the clip volume.
flat out vec4 dustMote;
out vec3 dustLocalPosition;
flat out vec3 dustWorldOrigin;
flat out mat3 dustWorldToLocal;

const int MOTES = 18;

float dustHash(vec2 p) {
  p = fract(p * vec2(443.8975, 397.2973));
  p += dot(p, p.yx + 19.19);
  return fract(p.x * p.y);
}

void materialVertex() {
  int mote = gl_VertexID / 6;
  if (mote >= MOTES) {
    gl_Position = vec4(0.0, 0.0, 2.0, 1.0);
    return;
  }
  // Two counter-clockwise triangles: (-1,-1) (1,-1) (1,1) and (-1,-1) (1,1) (-1,1).
  int corner = gl_VertexID - mote * 6;
  vec2 offset = vec2(
    corner == 1 || corner == 2 || corner == 4 ? 1.0 : -1.0,
    corner == 2 || corner == 4 || corner == 5 ? 1.0 : -1.0
  );

  // A small fixed population lives in the beam's local space. Its periodic
  // Host phase drifts continuously through the loop.
  float seed = float(mote) + 1.0;
  float z = mix(p_depth.x + 0.25, p_depth.y - 0.25, dustHash(vec2(seed, 7.0)))
    + 0.08 * sin(p_phase + seed);
  vec2 unit = vec2(dustHash(vec2(seed, 1.0)), dustHash(vec2(seed, 3.0))) * 1.4 - 0.7;
  unit += 0.06 * vec2(sin(p_phase + seed * 1.7), cos(p_phase + seed * 2.3));
  vec2 halfSize = mix(p_section.xy, p_section.zw, (z - p_depth.x) / (p_depth.y - p_depth.x));
  // The entity origin sits at the beam's middle depth, so blended sorting
  // draws the dust after the beam when the camera is on the panel side.
  vec3 centre = vec3(unit * halfSize, z - 0.5 * (p_depth.x + p_depth.y));
  float radius = mix(0.0024, 0.0046, dustHash(vec2(seed, 9.0)));

  // Rows 0 and 1 of the model-view-projection transform are the view's right
  // and up axes in local space, scaled by the projection. The quad covers the
  // mote, and at least 1.2% of the view height so antialiased motes keep a
  // few pixels at any distance.
  vec3 right = vec3(u_mvp[0][0], u_mvp[1][0], u_mvp[2][0]);
  vec3 up = vec3(u_mvp[0][1], u_mvp[1][1], u_mvp[2][1]);
  float depth = (u_mvp * vec4(centre, 1.0)).w;
  float size = max(3.0 * radius, 0.012 * depth / length(up));
  dustLocalPosition = centre + (offset.x * normalize(right) + offset.y * normalize(up)) * size;
  gl_Position = u_mvp * vec4(dustLocalPosition, 1.0);
  dustMote = vec4(centre, radius);
  dustWorldOrigin = u_model[3].xyz;
  dustWorldToLocal = inverse(mat3(u_model));
}
