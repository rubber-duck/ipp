out vec3 beamLocalPosition;
flat out vec3 beamWorldOrigin;
flat out mat3 beamWorldToLocal;

void materialVertex() {
  ippDefaultVertex();
  beamLocalPosition = a_position;
  beamWorldOrigin = u_model[3].xyz;
  beamWorldToLocal = inverse(mat3(u_model));
}
