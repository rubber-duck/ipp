#version 300 es
precision highp float;
in vec3 v_position;
in vec3 v_color;
{{#if normals}}in vec3 v_normal;{{/if}}
{{#if texture}}{{texture_declarations}}{{/if}}
{{#if weight}}{{weight_declarations}}{{/if}}
uniform vec3 u_material;
uniform vec4 u_camera;
uniform vec3 u_surface;
uniform vec3 u_ambient;
uniform vec4 u_lights[32];
uniform int u_light_count;
{{#if shadow}}{{shadow_declarations}}{{/if}}
out vec4 out_color;
const float PI = 3.141592653589793;

vec3 safe_normalize(vec3 v) {
    return v * inversesqrt(max(dot(v, v), 1e-20));
}

void main() {
    {{#if flat_normals}}
    // Derivatives of world position preserve flat face normals under nonuniform scale.
    vec3 n = safe_normalize(cross(dFdx(v_position), dFdy(v_position)));
    {{/if}}
    {{#if normals}}
    vec3 interpolated = v_normal / max(max(abs(v_normal.x), abs(v_normal.y)), max(abs(v_normal.z), 1e-30));
    vec3 n = safe_normalize(interpolated);
    {{/if}}
    vec3 v = safe_normalize(u_camera.w > 0.5 ? u_camera.xyz : u_camera.xyz - v_position);
    vec3 sampled = vec3(1.0);
    {{#if texture}}{{texture_body}}{{/if}}
    {{#if weight}}{{weight_body}}{{/if}}
    vec3 base = clamp(v_color * u_material * sampled, 0.0, 1.0);
    float metallic = u_surface.x;
    float roughness = max(u_surface.y, 0.045);
    float alpha = roughness * roughness;
    float a2 = alpha * alpha;
    float nv = max(dot(n, v), 1e-5);
    vec3 f0 = mix(vec3(0.04), base, metallic);
    vec3 radiance = base * u_ambient;
    for (int i = 0; i < 8; ++i) {
        if (i >= u_light_count) break;
        vec4 position = u_lights[i * 4];
        vec4 direction = u_lights[i * 4 + 1];
        vec4 color_range = u_lights[i * 4 + 2];
        vec3 delta = position.xyz - v_position;
        float distance_squared = max(dot(delta, delta), 0.0001);
        vec3 l = position.w < 0.5 ? -direction.xyz : delta * inversesqrt(distance_squared);
        float attenuation = 1.0;
        if (position.w > 0.5) {
            float ratio = sqrt(distance_squared) / color_range.w;
            attenuation = clamp(1.0 - pow(ratio, 4.0), 0.0, 1.0) / distance_squared;
        }
        if (position.w > 1.5) {
            float outer = u_lights[i * 4 + 3].x;
            float cone = clamp((dot(-l, direction.xyz) - outer) / max(direction.w - outer, 1e-6), 0.0, 1.0);
            attenuation *= cone * cone;
        }
        float nl = max(dot(n, l), 0.0);
        if (nl <= 0.0 || attenuation <= 0.0) continue;
        vec3 h = safe_normalize(v + l);
        float nh = max(dot(n, h), 0.0);
        float vh = max(dot(v, h), 0.0);
        vec3 fresnel = f0 + (1.0 - f0) * pow(1.0 - vh, 5.0);
        float denom = nh * nh * (a2 - 1.0) + 1.0;
        float distribution = a2 / max(PI * denom * denom, 1e-8);
        float visibility = 0.5 / max(nl * sqrt(nv * nv * (1.0 - a2) + a2) + nv * sqrt(nl * nl * (1.0 - a2) + a2), 1e-6);
        vec3 diffuse = (1.0 - fresnel) * (1.0 - metallic) * base / PI;
        float shadow = 1.0;
        {{#if shadow}}{{shadow_body}}{{/if}}
        radiance += (diffuse + fresnel * distribution * visibility) * color_range.rgb * (nl * attenuation * shadow);
    }
    // Direct-light subset uses unit exposure and clips before the standard sRGB transfer.
    vec3 rgb = clamp(radiance, 0.0, 1.0);
    out_color = vec4(rgb, 1.0);
}
