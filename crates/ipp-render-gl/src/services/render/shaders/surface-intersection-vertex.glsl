// Conservative scheduling bounds only. The visible surface is intersected in
// the fragment shader, so these boxes introduce no geometric approximation.
uniform vec4 u_intersection; // kind (0 off, 1 cylinder, 2 sphere), curvature, near branch, domain radius
uniform mat4 u_intersection_projection;
uniform mat4 u_intersection_inverse;
flat out vec4 v_intersection_chart; // content origin.xy and scale.xy in centred metres

vec3 intersection_sample(vec2 chart) {
    float k = u_intersection.y;
    float r = u_intersection.x < 1.5 ? abs(chart.x) : length(chart);
    float a = k * r;
    float sinc = abs(a) < 0.0001 ? 1.0 - a*a/6.0 : sin(a)/a;
    float half_sinc = abs(a) < 0.0001 ? 1.0-a*a/24.0 : sin(a*0.5)/(a*0.5);
    float z = -r*(a*0.5)*half_sinc*half_sinc;
    return vec3(chart.x*sinc, u_intersection.x < 1.5 ? chart.y : chart.y*sinc, z);
}

vec4 intersection_vertex(vec2 corner, vec4 rect) {
    mat4 content = u_intersection_inverse * u_mvp;
    v_intersection_chart = vec4(content[3].xy, content[0].x, content[1].y);
    vec2 lo = rect.xy * v_intersection_chart.zw + v_intersection_chart.xy;
    vec2 hi = rect.zw * v_intersection_chart.zw + v_intersection_chart.xy;
    vec2 chart_centre = (lo+hi)*0.5;
    vec2 half_extent = abs(hi-lo)*0.5;
    vec3 centre = intersection_sample(chart_centre);
    // Both chart maps have differential norm <= 1 on their valid domains.
    // Thus every mapped point lies in this ball, also at the chart's centre.
    vec3 radius = vec3(length(half_extent));
    if (u_intersection.x < 1.5) {
        // Cylinder height is affine and independent of its bend. In X, the
        // derivatives are cos(k*x) and -sin(k*x); bound each over this chart
        // interval. Keep narrow glyphs and wide, short paths out of a sphere
        // sized by their longest dimension, without approximating coverage.
        float slope = min(1.0, abs(u_intersection.y)*(abs(chart_centre.x)+half_extent.x));
        radius = vec3(half_extent.x, half_extent.y, half_extent.x*slope);
    }
    vec2 low = vec2(1.0), high = vec2(-1.0);
    bool crosses_eye = false;
    for (int i=0; i<8; ++i) {
        vec3 side = vec3(float(i&1), float((i>>1)&1), float((i>>2)&1))*2.0-1.0;
        vec4 p = u_intersection_projection * vec4(centre+side*radius, 1.0);
        crosses_eye = crosses_eye || p.w <= 0.000001;
        vec2 ndc = p.xy / max(p.w, 0.000001);
        low = min(low, ndc);
        high = max(high, ndc);
    }
    if (crosses_eye) { low=vec2(-1.0); high=vec2(1.0); }
    vec2 pad = vec2(4.0)/max(u_viewport.xy,vec2(1.0));
    return vec4(mix(max(low-pad,vec2(-1.0)),min(high+pad,vec2(1.0)),corner),0.0,1.0);
}
