// Conservative scheduling bounds only. The fragment shader computes the
// visible surface and its depth.
uniform vec4 u_intersection; // kind (0 off, 1 cylinder, 2 sphere), curvature, near branch, domain radius
uniform mat4 u_intersection_projection;
uniform mat4 u_intersection_inverse;


float intersection_sinc(float angle) {
    return abs(angle) < 0.0001 ? 1.0 - angle * angle / 6.0 : sin(angle) / angle;
}

vec3 intersection_sample(vec2 chart) {
    float k = u_intersection.y;
    float r = u_intersection.x < 1.5 ? abs(chart.x) : length(chart);
    float a = k * r;
    float sinc = intersection_sinc(a);
    float half_sinc = intersection_sinc(a * 0.5);
    float z = -r*(a*0.5)*half_sinc*half_sinc;
    return vec3(chart.x*sinc, u_intersection.x < 1.5 ? chart.y : chart.y*sinc, z);
}

// The two ends of a short circular arc and the intersection of their tangents
// enclose the arc. Extrude this triangle through the primitive's full height.
vec4 cylinder_vertex(vec2 corner, vec3 centre, vec2 half_extent, float centre_angle, float half_angle) {
    vec3 normal = vec3(sin(centre_angle), 0.0, cos(centre_angle));
    vec3 tangent = vec3(normal.z, 0.0, -normal.x);
    float half_sinc = intersection_sinc(half_angle * 0.5);
    // Signed distance from arc midpoint to chord, stable at zero curvature.
    float sagitta = half_extent.x * (half_angle * 0.5) * half_sinc * half_sinc;
    vec3 chord = centre - normal * sagitta;
    vec3 half_chord = tangent * (half_extent.x * intersection_sinc(half_angle));
    vec3 apex = centre + normal * (sagitta / cos(half_angle));

    vec4 mid = u_intersection_projection * vec4(chord, 1.0);
    vec4 side = u_intersection_projection * vec4(half_chord, 0.0);
    vec4 tip = u_intersection_projection * vec4(apex, 1.0);
    vec4 height = u_intersection_projection[1] * half_extent.y;
    vec4 clip[6] = vec4[6](mid - side - height, mid - side + height,
                           mid + side - height, mid + side + height,
                           tip - height, tip + height);
    vec2 viewport = max(u_viewport.xy, vec2(1.0));
    vec2 point[6];
    for (int i = 0; i < 6; ++i) {
        if (clip[i].w <= 0.000001) return vec4(corner * 2.0 - 1.0, 0.0, 1.0);
        point[i] = (clip[i].xy / clip[i].w + 1.0) * 0.5 * viewport;
    }

    // Fit the upper or lower edge to the two ends, then shift it out to
    // include all six points. With positive clip w their projected hull
    // contains the projected surface, even for a mirrored or rear view.
    bool upper = corner.y > 0.5;
    vec2 left = ((point[0].y > point[1].y) == upper) ? point[0] : point[1];
    vec2 right = ((point[2].y > point[3].y) == upper) ? point[2] : point[3];
    vec2 edge = right - left;
    float slope = abs(edge.x) > 1.0 ? clamp(edge.y / edge.x, -2.0, 2.0) : 0.0;
    vec2 direction = corner * 2.0 - 1.0;
    vec2 bound = direction * vec2(point[0].x, point[0].y - slope * point[0].x);
    for (int i = 1; i < 6; ++i) {
        bound = max(bound, direction * vec2(point[i].x, point[i].y - slope * point[i].x));
    }
    // Expand by two physical pixels in both axes, including the contribution
    // of horizontal padding to the sloped edge. Clip the sloped edge in GL.
    float x = clamp(direction.x * (bound.x + 2.0), 0.0, viewport.x);
    float y = slope * x + direction.y * (bound.y + 2.0 * (1.0 + abs(slope)));
    return vec4(vec2(x, y) * 2.0 / viewport - 1.0, 0.0, 1.0);
}

vec4 intersection_vertex(vec2 corner, vec4 rect) {
    mat4 content = u_intersection_inverse * u_mvp;
    vec2 chart_centre = mat2(content[0].xy,content[1].xy)*((rect.xy+rect.zw)*0.5)+content[3].xy;
    vec2 half_extent = mat2(abs(content[0].xy),abs(content[1].xy))*abs(rect.zw-rect.xy)*0.5;
    vec3 centre = intersection_sample(chart_centre);
    float half_angle = u_intersection.y * half_extent.x;
    // Keep the tangent intersection well conditioned. Broad arcs and spheres
    // retain the projected box enclosure below.
    if (u_intersection.x < 1.5 && abs(half_angle) < 1.0) {
        return cylinder_vertex(corner, centre, half_extent, u_intersection.y * chart_centre.x, half_angle);
    }
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
