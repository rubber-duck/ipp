#version 300 es
precision highp float;
in vec3 v_color;
{{#if texture}}{{texture_declarations}}{{/if}}
{{#if weight}}{{weight_declarations}}{{/if}}
uniform vec3 u_material;
out vec4 out_color;

void main() {
    vec3 sampled = vec3(1.0);
    {{#if texture}}{{texture_body}}{{/if}}
    {{#if weight}}{{weight_body}}{{/if}}
    vec3 linear_rgb = clamp(sampled * {{#if vertex_color}}v_color{{/if}}{{#if solid}}vec3(1.0){{/if}} * u_material, 0.0, 1.0);
    out_color = vec4(linear_rgb, 1.0);
}
