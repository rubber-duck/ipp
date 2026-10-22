// Textures decode sRGB once. Planet illumination and limb scattering stay linear.
// A single shared program shades the independent environment meshes.
vec4 planetMap(vec2 uv) {
    // The portable sampler is linear/clamp. Blend across longitude's wrap so
    // the rotating surface and cloud coverage share a continuous dateline.
    float width = float(ippTextureSize(p_base, 0).x);
    uv.x = fract(uv.x);
    vec4 value = ippSample(p_base, uv);
    float edge = (0.5 - abs(uv.x - 0.5)) * width;
    if (edge < 0.5) {
        float opposite = uv.x < 0.5 ? 1.0 : 0.0;
        value = mix(ippSample(p_base, vec2(opposite, uv.y)), value, 0.5 + edge);
    }
    return value;
}

vec4 materialFragment() {
    if (p_role > 4.5) {
        // Catalogue magnitude/colour occupy integer UV cells; local coordinates
        // describe a compact, antialiased optical point, never a glowing orb.
        vec2 point = (fract(v_uv) - 0.5) / 0.4;
        float radius = length(point);
        if (radius > 0.9) ippDiscard();
        float magnitude = floor(v_uv.x) / 20.0 - 2.0;
        float colorIndex = floor(v_uv.y) / 20.0 - 0.4;
        vec3 tint = mix(vec3(0.80, 0.88, 1.0), vec3(1.0, 0.97, 0.91), smoothstep(-0.3, 0.7, colorIndex));
        tint = mix(tint, vec3(1.0, 0.77, 0.55), smoothstep(0.7, 2.0, colorIndex) * 0.55);
        float footprint = length(fwidth(point));
        float sigma = max(0.19, footprint * 0.36);
        float profile = exp(-0.5 * radius * radius / (sigma * sigma));
        float exposure = 0.8 * pow(10.0, -0.18 * magnitude);
        return vec4(tint * profile * exposure * p_starGain, 1.0);
    }
    if (p_role > 2.5) {
        vec4 cabin = ippSample(p_base, v_uv);
        if (p_role < 3.5 && cabin.a < 0.5) ippDiscard();
        if (p_role > 3.5) {
            float shade = 0.18 + 0.22 * max(dot(normalize(v_normal), normalize(vec3(-0.3, 0.8, 0.4))), 0.0);
            cabin.rgb *= shade;
        }
        return vec4(cabin.rgb, 1.0);
    }
    if (p_role > 1.5) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    vec3 normal = normalize(v_normal);
    vec3 eye = normalize(u_camera.xyz - v_position);
    float facing = dot(normal, eye);
    if (facing <= 0.0) ippDiscard();
    vec3 light = normalize(vec3(-0.25, 0.65, 0.70));
    float sunlight = dot(normal, light);
    if (p_role > 0.5) {
        // A thin oblate shell brightens the limb and fades at its outer silhouette.
        float path = smoothstep(0.0, 0.06, facing) * (1.0 - smoothstep(0.12, 0.32, facing));
        float lit = 0.30 + 0.70 * smoothstep(-0.35, 0.45, sunlight);
        vec3 color = mix(vec3(0.018, 0.16, 0.62), vec3(0.28, 0.62, 1.0), lit);
        return vec4(color, path * lit * 0.85 * p_intensity);
    }
    // Polar rotation leaves the oblate silhouette unchanged. Registered albedo
    // and city maps wrap together, driven by the Host clock rather than JS ticks.
    vec2 uv = vec2(fract(v_uv.x + p_phase / 6.2831853 + 0.88), v_uv.y);
    vec3 surface = planetMap(uv).rgb;
    // NASA cloud coverage is packed in the albedo's otherwise unused alpha.
    // It keeps its own coordinates: weather drifts relative to terrain/cities.
    vec2 cloudUv = vec2(uv.x + p_cloudPhase / 6.2831853, uv.y);
    float cloud = smoothstep(0.04, 0.88, planetMap(cloudUv).a);
    float ocean = smoothstep(0.008, 0.05, surface.b - max(surface.r, surface.g)) * (1.0 - cloud);
    surface = mix(surface * (1.0 - 0.22 * cloud), vec3(0.88, 0.94, 1.0), cloud);
    float day = smoothstep(-0.13, 0.25, sunlight);
    vec3 color = surface * (0.035 + max(sunlight, 0.0) * 0.95);
    vec3 halfVector = normalize(light + eye);
    float glint = pow(max(dot(normal, halfVector), 0.0), 96.0);
    color += vec3(0.30, 0.43, 0.55) * glint * ocean * day;
    vec3 cities = ippSample(p_cities, uv).rrr * vec3(1.0, 0.72, 0.38);
    color += cities * 0.65 * p_cityGain * (1.0 - day) * (1.0 - cloud * 0.8)
        * (0.98 + 0.02 * cos(p_cityPhase + uv.x * 90.0));
    float rim = pow(1.0 - facing, 3.5);
    color += vec3(0.015, 0.10, 0.28) * rim * p_intensity
        * (0.3 + 0.7 * smoothstep(-0.3, 0.4, sunlight));
    return vec4(color, 1.0);
}
