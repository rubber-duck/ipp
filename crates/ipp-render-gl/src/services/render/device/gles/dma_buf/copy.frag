#version 300 es
precision highp float;

uniform sampler2D u_texture;
in vec2 v_uv;
out vec4 o_color;

void main() {
    vec4 color = texture(u_texture, v_uv);
    vec3 straight = color.a > 0.0 ? clamp(color.rgb / color.a, 0.0, 1.0) : vec3(0.0);
    vec3 linear = mix(
        straight / 12.92,
        pow((straight + 0.055) / 1.055, vec3(2.4)),
        step(vec3(0.04045), straight)
    );
    o_color = vec4(linear, color.a);
}
