#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_color;
{{#if texture}}{{texture_declarations}}{{/if}}
{{#if weight}}{{weight_declarations}}{{/if}}
{{#if skin}}{{skin_declarations}}{{/if}}
{{#if pose}}{{pose_declarations}}{{/if}}
uniform mat4 u_mvp;
out vec3 v_color;

void main() {
    vec3 local_position = a_position;
    {{#if pose}}{{pose_body}}{{/if}}
    {{#if rigid}}gl_Position = u_mvp * vec4(local_position, 1.0);{{/if}}
    {{#if skin}}gl_Position = u_mvp * skinned_position(local_position);{{/if}}
    v_color = a_color;
    {{#if texture}}{{texture_body}}{{/if}}
    {{#if weight}}{{weight_body}}{{/if}}
}
