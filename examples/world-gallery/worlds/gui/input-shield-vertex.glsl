out vec2 shieldUv;
out vec2 shieldSize;
out float shieldFace;

void materialVertex() {
  ippDefaultVertex();
  shieldFace = a_normal.z;
  if (abs(a_normal.x) > 0.5) {
    shieldUv = a_position.zy + 0.5;
    shieldSize = p_size.zy;
  } else if (abs(a_normal.y) > 0.5) {
    shieldUv = a_position.xz + 0.5;
    shieldSize = p_size.xz;
  } else {
    shieldUv = a_position.xy + 0.5;
    shieldSize = p_size.xy;
  }
}
