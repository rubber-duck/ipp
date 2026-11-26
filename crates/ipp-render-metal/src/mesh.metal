#include <metal_stdlib>
using namespace metal;

struct MeshVertex {
    float4 position;
    float4 color_weight;
    float4 uv;
    uint4 joints;
    float4 joint_weights;
    float4 normal;
};

struct Material {
    float4x4 clip_from_mesh;
    float4x4 world_from_mesh;
    float4 color;
    // Pose weight, vertex mode (0 rigid, 1 skinned, -1 plot color, -2 plot material), texture weight, ignore color.
    float4 options;
    // Metallic, roughness, use direct lighting, use authored normals.
    float4 surface;
    float4 effects;
};

struct PunctualLight {
    float4 position_kind;
    float4 direction_inner;
    float4 color_range;
    float4 cone;
};

struct Lighting {
    float4 camera;
    float4 ambient_count;
    PunctualLight lights[IPP_MAX_DOMAIN_LIGHTS];
    float4x4 shadow_matrices[IPP_MAX_DOMAIN_LIGHTS];
    float4 shadow_settings[IPP_MAX_DOMAIN_LIGHTS];
};

struct RasterVertex {
    float4 position [[position]];
    float4 color;
    float2 uv;
    float weight;
    float sprite;
    float3 world_position;
    float3 world_normal;
    float4 surface [[flat]];
    float4 effects [[flat]];
    float4 extra0, extra1, extra2, extra3;
};

static float3 unit(float3 v) {
    return v * rsqrt(max(dot(v, v), 1e-20f));
}

static float3 normal_direction(float3x3 linear, float3 normal) {
    float3x3 cofactors(cross(linear[1], linear[2]), cross(linear[2], linear[0]), cross(linear[0], linear[1]));
    float3 transformed = cofactors * normal;
    return dot(linear[0], cofactors[0]) < 0.0f ? -transformed : transformed;
}

static float3x3 linear_part(float4x4 transform) {
    return float3x3(transform[0].xyz, transform[1].xyz, transform[2].xyz);
}

struct PlotColorVertex { packed_float3 position, color; };
struct PlotMaterialVertex { packed_float3 position, color; packed_float2 uv; packed_float3 normal; };

static MeshVertex mesh_input_vertex(const device MeshVertex *mesh, uint index, float mode) {
    if (mode >= 0.0f) return mesh[index];
    MeshVertex v = {};
    if (mode == -1.0f) {
        const device PlotColorVertex &p = reinterpret_cast<const device PlotColorVertex *>(mesh)[index];
        v.position = float4(float3(p.position), 1.0);
        v.color_weight = float4(float3(p.color), 1.0);
    } else {
        const device PlotMaterialVertex &p = reinterpret_cast<const device PlotMaterialVertex *>(mesh)[index];
        v.position = float4(float3(p.position), 1.0);
        v.color_weight = float4(float3(p.color), 1.0);
        v.uv = float4(float2(p.uv), 0.0, 0.0);
        v.normal = float4(float3(p.normal), 0.0);
    }
    return v;
}

static RasterVertex mesh_default_vertex(uint index, MeshVertex v,
                                        const device Material &material,
                                        const device MeshVertex *pose,
                                        constant float4x4 *palette) {
    float4 p = v.position;
    float3 normal = float3(0.0f);
    if (material.options.x != 0.0f) p = mix(p, pose[index].position, material.options.x);
    if (material.surface.w != 0.0f) {
        normal = unit(v.normal.xyz);
        float3 blend = normal;
        if (material.options.x != 0.0f) blend = mix(normal, unit(pose[index].normal.xyz), material.options.x);
        if (dot(blend, blend) > 1e-12f) normal = unit(blend);
    }
    if (material.options.y > 0.0f) {
        uint4 j = v.joints;
        float4 w = v.joint_weights;
        float4x4 skin = w.x * palette[j.x] + w.y * palette[j.y] + w.z * palette[j.z] + w.w * palette[j.w];
        p = skin * p;
        float3 skinned = normal_direction(linear_part(skin), normal);
        if (any(skinned != float3(0.0f))) normal = skinned;
    }
    float4 clip = material.clip_from_mesh * p;
    clip.z = (clip.z + clip.w) * 0.5f;
    float3 color = material.options.w != 0.0f ? float3(1.0f) : v.color_weight.rgb;
    float weight = material.options.z != 0.0f ? v.color_weight.w : 1.0f;
    float3 world_position = (material.world_from_mesh * p).xyz;
    float3 world_normal = unit(normal_direction(linear_part(material.world_from_mesh), normal));
    return {clip, float4(color * material.color.rgb, material.color.a), v.uv.xy, weight, signbit(material.color.a) ? 1.0f : 0.0f,
            world_position, world_normal, material.surface, material.effects,
            float4(0.0), float4(0.0), float4(0.0), float4(0.0)};
}

vertex RasterVertex mesh_vertex(uint index [[vertex_id]],
                                uint instance [[instance_id]],
                                device const MeshVertex *mesh [[buffer(0)]],
                                const device Material *materials [[buffer(1)]],
                                device const MeshVertex *pose [[buffer(2)]],
                                constant float4x4 *palette [[buffer(3)]]) {
    const device Material &material = materials[instance];
    return mesh_default_vertex(index, mesh_input_vertex(mesh, index, material.options.y), material, pose, palette);
}

// Native shadow sampling follows the GLES emitter and receiver-plane equations.
bool shadow_contains(float2 uv) {
    return all(uv >= float2(0.0)) && all(uv <= float2(1.0));
}

float shadow_depth(depth2d<float> image, float2 uv, float4 settings) {
    // Beyond the spotlight map there is no recorded occluder; do not repeat its border.
    if (!shadow_contains(uv)) return 1.0;
    float2 tile = float2(fmod(settings.x, settings.w), floor(settings.x / settings.w));
    // Clamp to texel centres so a tile-edge tap never samples another light.
    uv = clamp(uv, float2(settings.z * 0.5), float2(1.0 - settings.z * 0.5));
    constexpr sampler texels(coord::normalized, address::clamp_to_edge, filter::nearest);
    return image.sample(texels, (float2(uv.x, 1.0 - uv.y) + tile) / settings.w);
}

float2 shadow_texel(float2 uv, float4 settings) {
    return (floor(uv / settings.z) + 0.5) * settings.z;
}

float2 shadow_disk(float index, float count) {
    float angle = index * 2.39996323;
    return float2(cos(angle), sin(angle)) * sqrt((index + 0.5) / count);
}

float shadow_distance(float depth, float near_distance, float far_distance) {
    return near_distance * far_distance / (far_distance - depth * (far_distance - near_distance));
}

// Project two receiver-plane tangents to compensate depth across wide kernels.
// This uses the surface normal, so it is valid inside divergent light branches.
float2 shadow_gradient(float3 normal, float4 projected, float4x4 matrix) {
    float3 tangent = cross(normal, abs(normal.y) < 0.99 ? float3(0.0, 1.0, 0.0) : float3(1.0, 0.0, 0.0));
    float4 a = matrix * float4(tangent, 0.0);
    float4 b = matrix * float4(cross(normal, tangent), 0.0);
    float3 da = (a.xyz - projected.xyz * (a.w / projected.w)) / projected.w;
    float3 db = (b.xyz - projected.xyz * (b.w / projected.w)) / projected.w;
    float determinant = da.x * db.y - db.x * da.y;
    if (abs(determinant) < 1e-10) return float2(0.0);
    return float2(da.z * db.y - db.z * da.y, da.x * db.z - db.x * da.z) / determinant;
}

float visibility_from_shadow(depth2d<float> image, constant Lighting &lighting,
                             uint index, float nl, float3 normal, float3 position) {
    float4 settings = lighting.shadow_settings[index];
    if (settings.x < 0.0) return 1.0;
    float4x4 matrix = lighting.shadow_matrices[index];
    float4 projected = matrix * float4(position, 1.0);
    if (projected.w <= 0.0) return 1.0;
    float3 uvz = projected.xyz / projected.w * 0.5 + 0.5;
    if (any(uvz <= float3(0.0)) || any(uvz >= float3(1.0))) return 1.0;
    float bias = settings.y * (1.0 + 2.0 * (1.0 - nl));
    float4 light = lighting.lights[index].cone;
    float visible = 0.0;
    float2 gradient = shadow_gradient(normal, projected, matrix);
    if (light.y <= 0.0) {
        // Radius zero preserves the original small PCF antialiasing footprint.
        for (int y = -1; y <= 1; ++y) {
            for (int x = -1; x <= 1; ++x) {
                float2 uv = shadow_texel(uvz.xy + float2(float(x), float(y)) * settings.z, settings);
                float depth = shadow_depth(image, uv, settings);
                visible += depth >= 1.0 || uvz.z + dot(gradient, uv - uvz.xy) - bias <= depth ? 1.0 : 0.0;
            }
        }
        return visible / 9.0;
    }
    // PCSS: find blockers in linear light-space depth, then estimate the emitter's
    // projected penumbra. Contact remains sharp; separated receivers become softer.
    // The map's diagonal bounds the useful search, not the authored emitter radius.
    float search_radius = min(1.414214, light.y * (projected.w - light.z) / (projected.w * light.z));
    float2 centre = shadow_texel(uvz.xy, settings);
    float centre_depth = shadow_depth(image, centre, settings);
    bool centre_blocked = centre_depth < 1.0 && centre_depth < uvz.z + dot(gradient, centre - uvz.xy) - bias;
    float blocker_sum = centre_blocked ? shadow_distance(centre_depth, light.z, light.w) : 0.0;
    float blocker_count = centre_blocked ? 1.0 : 0.0;
    for (int i = 0; i < 32; ++i) {
        float fraction = (float(i) + 0.5) / 32.0;
        // Cover texel-sized contact blockers as well as the full emitter footprint.
        // A uniform wide search can miss the caster directly under this receiver.
        float distance = settings.z * pow(max(1.0, search_radius / settings.z), fraction);
        float angle = float(i) * 2.39996323;
        float2 uv = shadow_texel(uvz.xy + float2(cos(angle), sin(angle)) * distance, settings);
        if (!shadow_contains(uv)) continue;
        float depth = shadow_depth(image, uv, settings);
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
        float2 uv = shadow_texel(uvz.xy + shadow_disk(float(i), 48.0) * radius, settings);
        float depth = shadow_depth(image, uv, settings);
        visible += depth >= 1.0 || uvz.z + dot(gradient, uv - uvz.xy) - bias <= depth ? 1.0 : 0.0;
    }
    return visible / 48.0;
}

static float3 direct_light(RasterVertex input, float3 base, constant Lighting &lighting, depth2d<float> shadows) {
    constexpr float pi = 3.141592653589793f;
    float3 n = input.surface.w != 0.0f
        ? unit(input.world_normal)
        : unit(cross(dfdx(input.world_position), -dfdy(input.world_position)));
    float3 v = unit(lighting.camera.w > 0.5f ? lighting.camera.xyz
                                             : lighting.camera.xyz - input.world_position);
    float metallic = input.surface.x;
    float roughness = max(input.surface.y, 0.045f);
    float alpha = roughness * roughness;
    float a2 = alpha * alpha;
    float nv = max(dot(n, v), 1e-5f);
    float3 f0 = mix(float3(0.04f), base, metallic);
    float3 radiance = base * lighting.ambient_count.rgb;
    for (uint i = 0; i < uint(lighting.ambient_count.w); ++i) {
        constant PunctualLight &light = lighting.lights[i];
        float4 position = light.position_kind;
        float4 direction = light.direction_inner;
        float3 delta = position.xyz - input.world_position;
        float distance_squared = max(dot(delta, delta), 0.0001f);
        float3 l = position.w < 0.5f ? -direction.xyz : delta * rsqrt(distance_squared);
        float attenuation = 1.0f;
        if (position.w > 0.5f) {
            float ratio_squared = distance_squared / (light.color_range.w * light.color_range.w);
            attenuation = clamp(1.0f - ratio_squared * ratio_squared, 0.0f, 1.0f) / distance_squared;
        }
        if (position.w > 1.5f) {
            float outer = light.cone.x;
            float cone = clamp((dot(-l, direction.xyz) - outer) / max(direction.w - outer, 1e-6f), 0.0f, 1.0f);
            attenuation *= cone * cone;
        }
        float nl = max(dot(n, l), 0.0f);
        if (nl <= 0.0f || attenuation <= 0.0f) continue;
        float3 h = unit(v + l);
        float nh = max(dot(n, h), 0.0f);
        float vh = max(dot(v, h), 0.0f);
        float grazing = max(1.0f - vh, 0.0f);
        float grazing_squared = grazing * grazing;
        float3 fresnel = f0 + (1.0f - f0) * (grazing_squared * grazing_squared * grazing);
        float denom = nh * nh * (a2 - 1.0f) + 1.0f;
        float distribution = a2 / max(pi * denom * denom, 1e-8f);
        float visibility = 0.5f / max(nl * sqrt(nv * nv * (1.0f - a2) + a2)
            + nv * sqrt(nl * nl * (1.0f - a2) + a2), 1e-6f);
        float3 diffuse = (1.0f - fresnel) * (1.0f - metallic) * base / pi;
        float visibility_shadow = input.effects.x == 0.0f ? 1.0f
            : visibility_from_shadow(shadows, lighting, i, nl, n, input.world_position);
        radiance += (diffuse + fresnel * distribution * visibility) * light.color_range.rgb * (nl * attenuation * visibility_shadow);
    }
    return clamp(radiance, 0.0f, 1.0f);
}

fragment float4 mesh_fragment(RasterVertex input [[stage_in]],
                              constant Lighting &lighting [[buffer(0)]],
                              texture2d<float> albedo [[texture(0)]],
                              depth2d<float> shadows [[texture(1)]]) {
    constexpr sampler mesh_sampler(coord::normalized, address::repeat, filter::nearest);
    float3 sampled = float3(1.0f);
    if (input.effects.w != 0.0f) {
        sampled = mix(sampled, albedo.sample(mesh_sampler, input.uv).rgb,
                      clamp(input.weight, 0.0f, 1.0f));
    }
    float3 color = clamp(sampled * input.color.rgb, 0.0f, 1.0f);
    if (input.surface.z != 0.0f) color = direct_light(input, color, lighting, shadows);
    float alpha = abs(input.color.a);
    if (input.sprite != 0.0f) {
        alpha *= 1.0f - smoothstep(0.35f, 0.5f, length(input.uv - 0.5f));
    }
    return float4(color, alpha);
}
