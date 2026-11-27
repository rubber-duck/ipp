#include <metal_stdlib>
using namespace metal;

struct PresentationVertex {
    float4 position [[position]];
    float2 uv;
};

vertex PresentationVertex present_vertex(uint id [[vertex_id]]) {
    float2 uv = float2((id << 1) & 2, id & 2);
    return {float4(uv * float2(2, -2) + float2(-1, 1), 0, 1), uv};
}

fragment float4 present_fragment(PresentationVertex in [[stage_in]], texture2d<float> source [[texture(0)]]) {
    constexpr sampler linear(coord::normalized, address::clamp_to_edge, filter::linear);
    return source.sample(linear, in.uv);
}
