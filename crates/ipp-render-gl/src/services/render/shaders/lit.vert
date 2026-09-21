#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_color;
{{#if skin}}{{skin_declarations}}{{/if}}
{{#if pose}}{{pose_declarations}}{{/if}}
{{#if texture}}{{texture_declarations}}{{/if}}
{{#if weight}}{{weight_declarations}}{{/if}}
uniform mat4 u_mvp;
uniform mat4 u_model;
{{#if normals}}
layout(location = 4) in vec3 a_normal;
{{#if pose}}{{pose_normal_declaration}}{{/if}}
uniform mat4 u_normal;
out vec3 v_normal;
{{/if}}
out vec3 v_position;
out vec3 v_color;

void main() {
    vec3 local_position = a_position;
    {{#if pose}}{{pose_body}}{{/if}}
    {{#if rigid}}vec4 position = vec4(local_position, 1.0);{{/if}}
    {{#if skin}}vec4 position = skinned_position(local_position);{{/if}}
    gl_Position = u_mvp * position;
    v_position = (u_model * position).xyz;
    v_color = a_color;
    {{#if texture}}{{texture_body}}{{/if}}
    {{#if weight}}{{weight_body}}{{/if}}
    {{#if normals}}
    vec3 authored = a_normal / max(max(abs(a_normal.x), abs(a_normal.y)), abs(a_normal.z));
    authored *= inversesqrt(dot(authored, authored));
    {{#if pose}}
    {{pose_normal_body}}
    {{/if}}
    {{#if skin}}authored = skinned_normal(authored);{{/if}}
    vec3 transformed = (u_normal * vec4(authored, 0.0)).xyz;
    transformed /= max(max(abs(transformed.x), abs(transformed.y)), abs(transformed.z));
    v_normal = transformed * inversesqrt(dot(transformed, transformed));
    {{/if}}
}
