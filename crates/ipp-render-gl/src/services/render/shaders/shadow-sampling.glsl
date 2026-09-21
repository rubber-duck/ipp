uniform highp sampler2D u_shadow_map;
uniform mat4 u_shadow_matrix[8];
// Per light: atlas tile (-1 disables), depth bias, inverse tile size, grid side.
uniform vec4 u_shadow_settings[8];

bool shadow_contains(vec2 uv) {
    return all(greaterThanEqual(uv, vec2(0.0))) && all(lessThanEqual(uv, vec2(1.0)));
}

float shadow_depth(vec2 uv, vec4 settings) {
    // Beyond the spotlight map there is no recorded occluder; do not repeat its border.
    if (!shadow_contains(uv)) return 1.0;
    vec2 tile = vec2(mod(settings.x, settings.w), floor(settings.x / settings.w));
    // Clamp to texel centres so a tile-edge tap never samples another light.
    uv = clamp(uv, vec2(settings.z * 0.5), vec2(1.0 - settings.z * 0.5));
    return texture(u_shadow_map, (uv + tile) / settings.w).r;
}

vec2 shadow_texel(vec2 uv, vec4 settings) {
    return (floor(uv / settings.z) + 0.5) * settings.z;
}

vec2 shadow_disk(float index, float count) {
    float angle = index * 2.39996323;
    return vec2(cos(angle), sin(angle)) * sqrt((index + 0.5) / count);
}

float shadow_distance(float depth, float near_distance, float far_distance) {
    return near_distance * far_distance / (far_distance - depth * (far_distance - near_distance));
}

// Project two receiver-plane tangents to compensate depth across wide kernels.
// This uses the surface normal, so it is valid inside divergent light branches.
vec2 shadow_gradient(vec3 normal, vec4 projected, mat4 matrix) {
    vec3 tangent = cross(normal, abs(normal.y) < 0.99 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0));
    vec4 a = matrix * vec4(tangent, 0.0);
    vec4 b = matrix * vec4(cross(normal, tangent), 0.0);
    vec3 da = (a.xyz - projected.xyz * (a.w / projected.w)) / projected.w;
    vec3 db = (b.xyz - projected.xyz * (b.w / projected.w)) / projected.w;
    float determinant = da.x * db.y - db.x * da.y;
    if (abs(determinant) < 1e-10) return vec2(0.0);
    return vec2(da.z * db.y - db.z * da.y, da.x * db.z - db.x * da.z) / determinant;
}

float visibility_from_shadow(int index, float nl, vec3 normal) {
    vec4 settings = u_shadow_settings[index];
    if (settings.x < 0.0) return 1.0;
    mat4 matrix = u_shadow_matrix[index];
    vec4 projected = matrix * vec4(v_position, 1.0);
    if (projected.w <= 0.0) return 1.0;
    vec3 uvz = projected.xyz / projected.w * 0.5 + 0.5;
    if (any(lessThanEqual(uvz, vec3(0.0))) || any(greaterThanEqual(uvz, vec3(1.0)))) return 1.0;
    float bias = settings.y * (1.0 + 2.0 * (1.0 - nl));
    vec4 light = u_lights[index * 4 + 3];
    float visible = 0.0;
    vec2 gradient = shadow_gradient(normal, projected, matrix);
    if (light.y <= 0.0) {
        // Radius zero preserves the original small PCF antialiasing footprint.
        for (int y = -1; y <= 1; ++y) {
            for (int x = -1; x <= 1; ++x) {
                vec2 uv = shadow_texel(uvz.xy + vec2(float(x), float(y)) * settings.z, settings);
                float depth = shadow_depth(uv, settings);
                visible += depth >= 1.0 || uvz.z + dot(gradient, uv - uvz.xy) - bias <= depth ? 1.0 : 0.0;
            }
        }
        return visible / 9.0;
    }
    // PCSS: find blockers in linear light-space depth, then estimate the emitter's
    // projected penumbra. Contact remains sharp; separated receivers become softer.
    // The map's diagonal bounds the useful search, not the authored emitter radius.
    float search_radius = min(1.414214, light.y * (projected.w - light.z) / (projected.w * light.z));
    vec2 centre = shadow_texel(uvz.xy, settings);
    float centre_depth = shadow_depth(centre, settings);
    bool centre_blocked = centre_depth < 1.0 && centre_depth < uvz.z + dot(gradient, centre - uvz.xy) - bias;
    float blocker_sum = centre_blocked ? shadow_distance(centre_depth, light.z, light.w) : 0.0;
    float blocker_count = centre_blocked ? 1.0 : 0.0;
    for (int i = 0; i < 32; ++i) {
        float fraction = (float(i) + 0.5) / 32.0;
        // Cover texel-sized contact blockers as well as the full emitter footprint.
        // A uniform wide search can miss the caster directly under this receiver.
        float distance = settings.z * pow(max(1.0, search_radius / settings.z), fraction);
        float angle = float(i) * 2.39996323;
        vec2 uv = shadow_texel(uvz.xy + vec2(cos(angle), sin(angle)) * distance, settings);
        if (!shadow_contains(uv)) continue;
        float depth = shadow_depth(uv, settings);
        // Clear depth is never a blocker, including beyond the receiver plane's far clip.
        if (depth < 1.0 && depth < uvz.z + dot(gradient, uv - uvz.xy) - bias) {
            blocker_sum += shadow_distance(depth, light.z, light.w);
            blocker_count += 1.0;
        }
    }
    if (blocker_count == 0.0) return 1.0;
    float blocker = blocker_sum / blocker_count;
    float radius = min(1.414214, max(settings.z, light.y / projected.w * ((projected.w - blocker) / blocker)));
    for (int i = 0; i < 48; ++i) {
        vec2 uv = shadow_texel(uvz.xy + shadow_disk(float(i), 48.0) * radius, settings);
        float depth = shadow_depth(uv, settings);
        visible += depth >= 1.0 || uvz.z + dot(gradient, uv - uvz.xy) - bias <= depth ? 1.0 : 0.0;
    }
    return visible / 48.0;
}
