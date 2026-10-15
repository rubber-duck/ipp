// Registered exterior masks are sampled as linear RGB by the sRGB texture device.
// One opaque pass preserves the original silhouette and camera framing.
vec4 materialFragment() {
    vec3 base = texture(p_base, v_uv).rgb;
    float seed = fract(sin(dot(floor(v_uv * p_imageSize),
        vec2(12.9898, 78.233))) * 43758.5453) * 6.2831853;
    float atmosphereMotion = 0.96 + 0.04 * cos(p_phase + v_uv.x * 18.0);
    float cityMotion = 0.98 + 0.02 * cos(p_cityPhase + seed);
    float starMotion = 0.92 + 0.08 * cos(p_starPhase + seed);
    vec3 emission = texture(p_atmosphere, v_uv).rgb * p_intensity * atmosphereMotion
        + texture(p_cities, v_uv).rgb * p_cityGain * cityMotion
        + texture(p_stars, v_uv).rgb * p_starGain * starMotion;
    return vec4(base + emission, 1.0);
}
