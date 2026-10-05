out vec3 beamLocalPosition;
flat out vec3 beamWorldOrigin;
flat out mat3 beamWorldToLocal;

void materialVertex() {
  ippDefaultVertex();
  beamWorldOrigin = u_model[3].xyz;
  beamWorldToLocal = inverse(mat3(u_model));
  // ippDefaultVertex owns MeshPose interpolation. Shading uses its actual
  // output, including the curved endpoint, rather than the flat Basis.
  beamLocalPosition = beamWorldToLocal * (v_position - beamWorldOrigin);
}
