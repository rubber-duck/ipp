#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
{{#if skin}}{{skin_declarations}}{{/if}}
{{#if pose}}{{pose_declarations}}{{/if}}
uniform mat4 u_mvp;

void main() {
    vec3 local_position = a_position;
    {{#if pose}}{{pose_body}}{{/if}}
    {{#if rigid}}vec4 position = vec4(local_position, 1.0);{{/if}}
    {{#if skin}}vec4 position = skinned_position(local_position);{{/if}}
    gl_Position = u_mvp * position;
}
