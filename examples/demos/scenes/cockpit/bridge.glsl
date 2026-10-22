// Scalar contribution is stored in alpha to retain linear values when sampled.
// The base contains physical unpowered diffusers; color belongs to the light.
vec4 materialFragment() {
    vec4 base = cockpitBase(v_uv);
    if (base.a < 0.5) ippDiscard();
    float contribution = cockpitContribution(v_uv).a;
    float intensity = p_illumination * (0.78 + 0.22 * cos(p_cabinPhase));
    return vec4(base.rgb / base.a + p_cabinTint * contribution * intensity, 1.0);
}
