#version 450
layout(location=0) in vec2 vertex_local;
layout(location=1) flat in uint slot;
layout(set=0,binding=0) readonly buffer Paints { vec4 paints[]; };
layout(set=2,binding=0) readonly buffer Paths { vec4 paths[]; };

uint mask_index() {
    vec4 rect = paints[slot + 2u];
    uvec2 pixel = uvec2(gl_FragCoord.xy) - uvec2(rect.xy);
    return uint(rect.w) + pixel.y * uint(rect.z) + pixel.x;
}

#ifdef IPP_WRITE_MASK
layout(set=1,binding=0) buffer Coverage { float samples[]; };

vec4 record_at(uint i) {
    return paths[i];
}

// IPP_PATH_COVERAGE

void main() {
    vec4 options = paints[slot + 6u];
    samples[mask_index()] = path_coverage(uint(paints[slot + 21u].x), vertex_local,
        options.y, options.w, dFdx(vertex_local), dFdy(vertex_local));
}
#else
layout(location=0) out vec4 out_color;

void main() {
    uint i = mask_index();
    float coverage = paths[i / 4u][i % 4u];
    vec4 tint = paints[slot];
    vec4 bounds = paints[slot + 17u];
    vec2 footprint = max(abs(dFdx(vertex_local)) + abs(dFdy(vertex_local)), vec2(1.0 / 65536.0));
    vec2 inside = min(vertex_local - bounds.xy, bounds.zw - vertex_local);
    float clip = clamp(min(inside.x / footprint.x, inside.y / footprint.y) + 0.5, 0.0, 1.0);
    float alpha = tint.a * coverage;
    out_color = vec4(tint.rgb * alpha, alpha) * clip;
}
#endif
