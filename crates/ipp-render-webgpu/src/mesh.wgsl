struct Light {
    position_kind: vec4f,
    direction_inner: vec4f,
    color_range: vec4f,
    cone: vec4f,
}

struct Camera {
    projection: mat4x4f,
    eye: vec4f,
    ambient_count: vec4f,
    lights: array<Light, IPP_MAX_DOMAIN_LIGHTS>,
    shadow_matrices:array<mat4x4f,IPP_MAX_DOMAIN_LIGHTS>,
    shadow_settings:array<vec4f,IPP_MAX_DOMAIN_LIGHTS>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var nearest_repeat: sampler;
@group(2) @binding(0) var<uniform> joints: array<mat4x4f, IPP_MAX_JOINTS>;

struct MeshInput {
    @location(0) position: vec4f,
    @location(1) color_weight: vec4f,
    @location(2) uv: vec4f,
    @location(3) joints: vec4u,
    @location(4) weights: vec4f,
    @location(5) normal: vec4f,
    @location(6) pose_position: vec4f,
    @location(7) pose_normal: vec4f,
    @location(8) model_x: vec4f,
    @location(9) model_y: vec4f,
    @location(10) model_z: vec4f,
    @location(11) model_w: vec4f,
    @location(12) tint: vec4f,
    // Pose weight, skinned, texture weight enabled, solid fallback.
    @location(13) options: vec4f,
    // Metallic, roughness, PBR, supplied normals.
    @location(14) surface: vec4f,
}

struct Raster {
    @builtin(position) clip: vec4f,
    @location(0) color: vec4f,
    @location(1) uv: vec2f,
    @location(2) weight: f32,
    @location(3) world_position: vec3f,
    @location(4) normal: vec3f,
    @location(5) @interpolate(flat) surface: vec4f,
    @location(6) @interpolate(flat) receive_shadow:f32,
}

fn unit(v: vec3f) -> vec3f {
    return v * inverseSqrt(max(dot(v, v), 1e-20));
}

fn normal_direction(m: mat4x4f, n: vec3f) -> vec3f {
    let a = cross(m[1].xyz, m[2].xyz);
    let b = cross(m[2].xyz, m[0].xyz);
    let c = cross(m[0].xyz, m[1].xyz);
    let transformed = mat3x3f(a, b, c) * n;
    return select(transformed, -transformed, dot(m[0].xyz, a) < 0.0);
}

@vertex fn mesh_vertex(v: MeshInput) -> Raster {
    let model = mat4x4f(v.model_x, v.model_y, v.model_z, v.model_w);
    var p = mix(v.position, v.pose_position, v.options.x);
    var n = unit(v.normal.xyz);
    let blended = mix(n, unit(v.pose_normal.xyz), v.options.x);
    if dot(blended, blended) > 1e-12 { n = unit(blended); }
    if v.options.y > 0.0 {
        let j = v.joints;
        let skin = joints[j.x] * v.weights.x + joints[j.y] * v.weights.y
                 + joints[j.z] * v.weights.z + joints[j.w] * v.weights.w;
        p = skin * p;
        let skinned = unit(normal_direction(skin, n));
        if any(skinned != vec3f(0.0)) { n = skinned; }
    }
    let world = model * p;
    var clip = camera.projection * world;
    clip.z = (clip.z + clip.w) * 0.5;
    let rgb = select(v.color_weight.rgb, vec3f(1.0), v.options.w > 0.0);
    return Raster(clip, vec4f(rgb * v.tint.rgb, v.tint.a), v.uv.xy,
        select(1.0, v.color_weight.w, v.options.z > 0.0),
        world.xyz, unit(normal_direction(model, n)), v.surface,select(0.0,1.0,v.surface.z>1.5));
}

@fragment fn mesh_fragment(v: Raster) -> @location(0) vec4f {
    let texel = textureSample(albedo, nearest_repeat, v.uv).rgb;
    let base = clamp(mix(vec3f(1.0), texel, clamp(v.weight, 0.0, 1.0)) * v.color.rgb,
                     vec3f(0.0), vec3f(1.0));
    let face = unit(cross(dpdx(v.world_position), -dpdy(v.world_position)));
    if v.surface.z == 0.0 {
        var alpha=abs(v.color.a);
        if v.color.a < 0.0 { alpha *= 1.0-smoothstep(0.35,0.5,length(v.uv-0.5)); }
        return vec4f(base, alpha);
    }

    let n = select(face, unit(v.normal), v.surface.w != 0.0);
    let view = unit(select(camera.eye.xyz - v.world_position, camera.eye.xyz, camera.eye.w > 0.5));
    let metallic = v.surface.x;
    let roughness = max(v.surface.y, 0.045);
    let a2 = roughness * roughness * roughness * roughness;
    let nv = max(dot(n, view), 1e-5);
    let f0 = mix(vec3f(0.04), base, metallic);
    var radiance = base * camera.ambient_count.rgb;
    for (var i = 0u; i < u32(camera.ambient_count.w); i++) {
        let light = camera.lights[i];
        let delta = light.position_kind.xyz - v.world_position;
        let d2 = max(dot(delta, delta), 0.0001);
        var l = -light.direction_inner.xyz;
        var attenuation = 1.0;
        if light.position_kind.w > 0.5 {
            l = delta * inverseSqrt(d2);
            let ratio = d2 / (light.color_range.w * light.color_range.w);
            attenuation = clamp(1.0 - ratio * ratio, 0.0, 1.0) / d2;
        }
        if light.position_kind.w > 1.5 {
            let cone = clamp((dot(-l, light.direction_inner.xyz) - light.cone.x)
                / max(light.direction_inner.w - light.cone.x, 1e-6), 0.0, 1.0);
            attenuation *= cone * cone;
        }
        let nl = max(dot(n, l), 0.0);
        if nl <= 0.0 || attenuation <= 0.0 { continue; }
        let h = unit(view + l);
        let nh = max(dot(n, h), 0.0);
        let g = max(1.0 - max(dot(view, h), 0.0), 0.0);
        let fresnel = f0 + (vec3f(1.0) - f0) * (g*g*g*g*g);
        let denom = nh*nh*(a2-1.0) + 1.0;
        let distribution = a2 / max(3.14159265 * denom * denom, 1e-8);
        let smith = 0.5 / max(nl * sqrt(nv*nv*(1.0-a2)+a2) + nv * sqrt(nl*nl*(1.0-a2)+a2), 1e-6);
        let diffuse = (vec3f(1.0) - fresnel) * (1.0-metallic) * base / 3.14159265;
        var shadow=1.0;
        if v.receive_shadow>0.0 {shadow=visibility(i,nl,n,v.world_position);}
        radiance += (diffuse + fresnel * distribution * smith) * light.color_range.rgb * (nl*attenuation*shadow);
    }
    return vec4f(clamp(radiance, vec3f(0.0), vec3f(1.0)), v.color.a);
}

struct UnlitRaster {
    @builtin(position) clip:vec4f,
    @location(0) color:vec4f,
    @location(1) uv:vec2f,
    @location(2) weight:f32,
}

@vertex fn rigid_vertex(v: MeshInput)->UnlitRaster {
    let model=mat4x4f(v.model_x,v.model_y,v.model_z,v.model_w);
    var clip=camera.projection*(model*v.position);
    clip.z=(clip.z+clip.w)*0.5;
    let color=select(v.color_weight.rgb,vec3f(1),v.options.w>0.0);
    return UnlitRaster(clip,vec4f(color*v.tint.rgb,v.tint.a),v.uv.xy,select(1.0,v.color_weight.w,v.options.z>0.0));
}

@vertex fn plot_vertex(@location(0) position: vec3f, @location(1) color: vec3f,
                       @location(8) x: vec4f, @location(9) y: vec4f,
                       @location(10) z: vec4f, @location(11) w: vec4f,
                       @location(12) tint: vec4f) -> UnlitRaster {
    var clip = camera.projection * (mat4x4f(x,y,z,w) * vec4f(position,1.0));
    clip.z = (clip.z + clip.w) * 0.5;
    return UnlitRaster(clip, vec4f(color * tint.rgb, tint.a), vec2f(0.0), 1.0);
}

@fragment fn color_fragment(v:UnlitRaster)->@location(0) vec4f {
    return vec4f(clamp(v.color.rgb,vec3f(0),vec3f(1)),v.color.a);
}

@fragment fn textured_fragment(v:UnlitRaster)->@location(0) vec4f {
    let texel=textureSample(albedo,nearest_repeat,v.uv).rgb;
    let base=clamp(mix(vec3f(1),texel,clamp(v.weight,0.0,1.0))*v.color.rgb,vec3f(0),vec3f(1));
    return vec4f(base,v.color.a);
}
