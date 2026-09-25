out vec2 shieldUv;

void materialVertex() {
  ippDefaultVertex();
  shieldUv = a_position.xy + 0.5;
}
