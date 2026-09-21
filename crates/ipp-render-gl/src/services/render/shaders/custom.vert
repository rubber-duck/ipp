// CUSTOM_DECLARATIONS
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_color;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in float a_weight;
layout(location = 4) in vec3 a_normal;
{{#if skin}}{{skin_declarations}}{{/if}}
{{#if pose}}{{pose_declarations}}{{/if}}
{{#if pose}}{{#if normals}}{{pose_normal_declaration}}{{/if}}{{/if}}
uniform mat4 u_mvp;
uniform mat4 u_model;
uniform mat4 u_normal;
out vec3 v_position;
out vec3 v_normal;
out vec3 v_color;
out vec2 v_uv;
out float v_weight;
vec3 ippSafeNormalize(vec3 v) { return v * inversesqrt(max(dot(v, v), 1e-20)); }
void ippDefaultVertex() {
    vec3 local_position = a_position;
    {{#if pose}}{{pose_body}}{{/if}}
    {{#if rigid}}vec4 position = vec4(local_position, 1.0);{{/if}}
    {{#if skin}}vec4 position = skinned_position(local_position);{{/if}}
    gl_Position = u_mvp * position;
    v_position = (u_model * position).xyz;
    v_color = a_color;
    v_uv = a_uv;
    v_weight = a_weight;
    vec3 authored = vec3(0.0);
    {{#if normals}}authored = a_normal;{{/if}}
    {{#if pose}}{{#if normals}}{{pose_normal_body}}{{/if}}{{/if}}
    {{#if skin}}authored = skinned_normal(authored);{{/if}}
    v_normal = ippSafeNormalize((u_normal * vec4(authored, 0.0)).xyz);
}
