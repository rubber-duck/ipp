uniform vec4 u_camera;
uniform vec3 u_ambient;
uniform vec4 u_lights[32];
uniform int u_light_count;
// Directional/point/spot light records share the built-in material interface.
vec3 ippSurfaceNormal() {
    vec3 n = dot(v_normal, v_normal) > 1e-20 ? v_normal : cross(dFdx(v_position), dFdy(v_position));
    return n * inversesqrt(max(dot(n,n), 1e-20));
}
vec3 ippLightDirection(int i, vec3 position) {
    vec4 light = u_lights[i * 4];
    vec3 delta = light.w < 0.5 ? -u_lights[i * 4 + 1].xyz : light.xyz - position;
    return delta * inversesqrt(max(dot(delta, delta), 1e-20));
}
vec3 ippLightRadiance(int i, vec3 position) {
    vec4 light = u_lights[i * 4];
    vec4 colorRange = u_lights[i * 4 + 2];
    float attenuation = 1.0;
    if (light.w > 0.5) {
        vec3 delta = light.xyz - position;
        float distanceSquared = max(dot(delta, delta), 0.0001);
        float ratio = sqrt(distanceSquared) / colorRange.w;
        attenuation = clamp(1.0 - pow(ratio, 4.0), 0.0, 1.0) / distanceSquared;
    }
    if (light.w > 1.5) {
        vec4 direction = u_lights[i * 4 + 1];
        float outer = u_lights[i * 4 + 3].x;
        float cone = clamp((dot(-ippLightDirection(i, position), direction.xyz) - outer) / max(direction.w - outer, 1e-6), 0.0, 1.0);
        attenuation *= cone * cone;
    }
    return colorRange.rgb * attenuation;
}
