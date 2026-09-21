#version 300 es
precision highp float;
uniform sampler2D u_texture;
in vec2 v_uv;
out vec4 out_color;
void main() {
    vec4 color = texture(u_texture, v_uv);
    vec3 low = 12.92 * color.rgb;
    vec3 high = 1.055 * pow(color.rgb, vec3(1.0 / 2.4)) - 0.055;
    out_color = vec4(mix(high, low, lessThanEqual(color.rgb, vec3(0.0031308))), color.a);
}
