// Bindings supplied by the generated portable material interface.
struct MaterialScope {
    float3 a_position, a_color;
    float2 a_uv;
    float a_weight;
    float3 a_normal;
    float3 v_position, v_color, v_normal;
    float2 v_uv;
    float v_weight;
    float4 ipp_position;
    float2 ipp_fragment_coord;
    bool ipp_front_facing;
    float4x4 u_model, u_mvp, u_normal;
    constant Lighting *lighting;
    float4 u_camera;
    float3 u_ambient;
    int u_light_count;
    depth2d<float> shadows;
    bool receives_shadows;

    /* IPP_FIELDS */

    void ippDefaultVertex() thread {}
    float3 ippSafeNormalize(float3 v) thread { return unit(v); }

    float3 ippSurfaceNormal() thread {
        float3 n = dot(v_normal, v_normal) > 1e-20f
            ? v_normal : cross(dfdx(v_position), -dfdy(v_position));
        return unit(n);
    }

    float3 ippLightDirection(int index, float3 position) thread {
        if (index < 0 || index >= u_light_count) return float3(0.0, 0.0, 1.0);
        constant PunctualLight &light = lighting->lights[index];
        return unit(light.position_kind.w < 0.5
            ? -light.direction_inner.xyz : light.position_kind.xyz - position);
    }

    float3 ippLightRadiance(int index, float3 position) thread {
        if (index < 0 || index >= u_light_count) return float3(0.0);
        constant PunctualLight &light = lighting->lights[index];
        float attenuation = 1.0;
        if (light.position_kind.w > 0.5) {
            float3 delta = light.position_kind.xyz - position;
            float distance_squared = max(dot(delta, delta), 0.0001);
            float ratio = sqrt(distance_squared) / light.color_range.w;
            attenuation = clamp(1.0 - pow(ratio, 4.0), 0.0, 1.0) / distance_squared;
        }
        if (light.position_kind.w > 1.5) {
            float3 direction = ippLightDirection(index, position);
            float outer = light.cone.x;
            float cone = clamp((dot(-direction, light.direction_inner.xyz) - outer)
                / max(light.direction_inner.w - outer, 1e-6), 0.0, 1.0);
            attenuation *= cone * cone;
        }
        return light.color_range.rgb * attenuation;
    }

    float ippShadowVisibility(int index, float nl) thread {
        if (index < 0 || index >= u_light_count || !receives_shadows) return 1.0;
        return visibility_from_shadow(shadows, *lighting, uint(index), nl, ippSurfaceNormal(), v_position);
    }

    float ippShadowVisibility(float nl) thread {
        for (int i = 0; i < u_light_count; ++i) {
            if (lighting->shadow_settings[i].x >= 0.0) return ippShadowVisibility(i, nl);
        }
        return 1.0;
    }

    /* IPP_VERTEX */
    /* IPP_FRAGMENT */
};

vertex RasterVertex custom_vertex(uint index [[vertex_id]],
                                  uint instance [[instance_id]],
                                  const device MeshVertex *mesh [[buffer(0)]],
                                  const device Material *materials [[buffer(1)]],
                                  const device MeshVertex *pose [[buffer(2)]],
                                  constant float4x4 *palette [[buffer(3)]],
                                  constant uint4 *parameters [[buffer(4)]]
                                  /* IPP_TEXTURE_ARGUMENTS */) {
    const device Material &material = materials[instance];
    MeshVertex input_vertex = mesh_input_vertex(mesh, index, material.options.y);
    RasterVertex result = mesh_default_vertex(index, input_vertex, material, pose, palette);
    MaterialScope scope;
    scope.a_position = input_vertex.position.xyz;
    scope.a_color = input_vertex.color_weight.rgb;
    scope.a_weight = input_vertex.color_weight.w;
    scope.a_uv = input_vertex.uv.xy;
    scope.a_normal = input_vertex.normal.xyz;
    scope.u_model = material.world_from_mesh;
    scope.u_mvp = material.clip_from_mesh;
    float3x3 linear = linear_part(scope.u_model);
    float3x3 cofactors(cross(linear[1], linear[2]), cross(linear[2], linear[0]), cross(linear[0], linear[1]));
    float sign = dot(linear[0], cofactors[0]) < 0.0 ? -1.0 : 1.0;
    scope.u_normal = float4x4(float4(sign * cofactors[0], 0.0), float4(sign * cofactors[1], 0.0),
                             float4(sign * cofactors[2], 0.0), float4(0.0, 0.0, 0.0, 1.0));
    scope.ipp_position = result.position;
    scope.ipp_position.z = 2.0 * scope.ipp_position.z - scope.ipp_position.w;
    scope.v_position = result.world_position;
    scope.v_normal = result.world_normal;
    scope.v_uv = result.uv;
    scope.v_color = input_vertex.color_weight.rgb;
    scope.v_weight = result.weight;
    /* IPP_PARAMETERS */
    scope.materialVertex();
    result.position = scope.ipp_position;
    result.position.z = (result.position.z + result.position.w) * 0.5;
    result.world_position = scope.v_position;
    result.world_normal = scope.v_normal;
    result.uv = scope.v_uv;
    result.color.rgb = scope.v_color;
    result.weight = scope.v_weight;
    /* IPP_STORE_VARYINGS */
    return result;
}

#if IPP_PASS_SHADOW
fragment void custom_fragment(RasterVertex raster [[stage_in]],
#else
fragment float4 custom_fragment(RasterVertex raster [[stage_in]],
#endif
                                bool front_facing [[front_facing]],
                                constant Lighting &lighting [[buffer(0)]],
                                constant uint4 *parameters [[buffer(4)]],
                                depth2d<float> shadows [[texture(1)]]
                                /* IPP_TEXTURE_ARGUMENTS */) {
    MaterialScope scope;
    scope.v_position = raster.world_position;
    scope.v_normal = raster.world_normal;
    scope.v_uv = raster.uv;
    scope.v_color = raster.color.rgb;
    scope.v_weight = raster.weight;
    scope.ipp_fragment_coord = raster.position.xy;
    scope.ipp_front_facing = front_facing;
    scope.lighting = &lighting;
#if IPP_PASS_SHADOW
    scope.u_camera = float4(0.0);
    scope.u_ambient = float3(0.0);
    scope.u_light_count = 0;
#else
    scope.u_camera = lighting.camera;
    scope.u_ambient = lighting.ambient_count.rgb;
    scope.u_light_count = raster.effects.y != 0.0 ? int(lighting.ambient_count.w) : 0;
#endif
    scope.shadows = shadows;
    scope.receives_shadows = raster.effects.x != 0.0;
    /* IPP_PARAMETERS */
    /* IPP_LOAD_VARYINGS */
    float4 color = scope.materialFragment();
    if (raster.effects.z == 1.0 && color.a < raster.effects.w) discard_fragment();
#if !IPP_PASS_SHADOW
    color = clamp(color, 0.0, 1.0);
    if (raster.effects.z != 2.0) color.a = 1.0;
    return color;
#endif
}
