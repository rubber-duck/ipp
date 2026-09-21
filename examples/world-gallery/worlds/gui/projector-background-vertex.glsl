out vec2 studioUv;

void materialVertex() {
  ippDefaultVertex();
  studioUv = a_position.xy + 0.5;
  gl_Position = vec4(a_position.xy * 2.0, 0.9999, 1.0);
}
