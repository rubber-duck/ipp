#version 300 es
precision highp float;
uniform vec3 u_material;
out vec4 out_color;

void main() {
    vec3 linear_rgb = clamp(u_material, 0.0, 1.0);
    out_color = vec4(linear_rgb, 1.0);
}
