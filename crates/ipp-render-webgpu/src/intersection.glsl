#ifdef IPP_ANALYTIC
// The bounding quad schedules fragments. Actual cylindrical or spherical intersections
// determine local paint coordinates, the presentation basis, coverage and depth.
mat4 table_matrix(uint offset) {
    return mat4(paint_at(offset),paint_at(offset+1u),paint_at(offset+2u),paint_at(offset+3u));
}
vec2 intersect_surface(uint mapping, uint draw, out float coverage,out float depth) {
    vec4 parameters=paint_at(mapping), chart_to_root=paint_at(mapping+13u);
    mat4 projection=table_matrix(mapping+1u), world=table_matrix(mapping+9u);
    vec4 clip_edges=paint_at(draw+3u);
    vec2 chart;
    vec3 right,down,point;
    float discriminant,projected_depth;
    bool valid_distance,valid_chart;
        vec4 n = vertex_world;
        vec4 m = n + paint_at(mapping + 7u);
        vec3 origin = n.xyz / n.w;
        vec3 direction = unit_direction(m.xyz / m.w - origin);
        bool cylinder = parameters.x < 1.5;
        vec3 axes = cylinder ? vec3(1, 0, 1) : vec3(1);
        float k = parameters.y;
        float a = k * dot(direction * axes, direction);
        float b = k * dot(origin * axes, direction) + direction.z;
        float c = k * dot(origin * axes, origin) + 2.0 * origin.z;
        discriminant = b * b - a * c;
        float root = sqrt(max(discriminant, 0.0));
        float q = -b - (b < 0.0 ? -root : root);
        float safe_a = a != 0.0 ? a : 1.0;
        float t0 = q / safe_a;
        float t1 = abs(q) > 1e-20 ? c / q : -b / safe_a;
        bool near_root = parameters.z > 0.5;
        float t = near_root ? min(t0, t1) : max(t0, t1);
        if (discriminant < 0.0) t = -b / safe_a;
        float limit = length(origin) + parameters.w;
        bool valid_equation = true;
        if (a == 0.0) {
            valid_equation = b != 0.0 && near_root;
            t = b != 0.0 ? -c / (2.0 * b) : 0.0;
        }
        valid_distance = valid_equation && t >= 0.0 && t <= limit;
        t = clamp(t, 0.0, limit);
        point = origin + direction * t;

        float denominator = 1.0 + k * point.z;
        if (cylinder) {
            float x_normal = k * point.x;
            float angle = atan(x_normal, denominator);
            valid_chart = abs(angle) < 3.14159265;
            float ratio = point.x / max(denominator, 1e-20);
            float small = k * ratio;
            float x = denominator > 0.0 && abs(small) < 0.0001
                ? ratio * (1.0 - small * small / 3.0) : angle / k;
            chart = vec2(x, point.y);
            right = unit_direction(vec3(denominator, 0.0, -x_normal));
            down = vec3(0.0, -1.0, 0.0);
        } else {
            float radial = length(k * point.xy);
            float angle = atan(radial, denominator);
            valid_chart = angle < 3.14159265;
            float small = radial / max(denominator, 1e-20);
            float scale = denominator > 0.0 && small < 0.0001
                ? (1.0 - small * small / 3.0) / denominator : angle / max(radial, 1e-20);
            chart = point.xy * scale;
            float r = length(chart);
            float signed_angle = k * r;
            float sinc = abs(signed_angle) < 1e-4
                ? 1.0 - signed_angle * signed_angle / 6.0 : sin(signed_angle) / signed_angle;
            vec2 radial_direction = chart / max(r, 1e-20);
            float response = cos(signed_angle) - sinc;
            vec3 axis = vec3(radial_direction * response, -sin(signed_angle));
            right = vec3(sinc, 0.0, 0.0) + axis * radial_direction.x;
            down = -(vec3(0.0, sinc, 0.0) + axis * radial_direction.y);
        }
        vec4 projected = projection * vec4(point, 1.0);
        projected_depth = projected.z / projected.w * 0.5 + 0.5;
        valid_chart = valid_chart && projected.w > 0.0;
    depth = clamp(projected_depth, 0.0, 1.0);
    coverage = clamp(discriminant / max(fwidth(discriminant), 1e-20) + 0.5, 0.0, 1.0);
    if (!valid_chart || !valid_distance || projected_depth < 0.0 || projected_depth > 1.0)
        coverage = 0.0;

    vec2 root_position = (chart + vec2(chart_to_root.x, -chart_to_root.y))
        * chart_to_root.zw;
    if (clip_edges.x < 0.0) {
        vec4 rectangle = paint_at(draw + 2u);
        vec2 distance = min(root_position - rectangle.xy, rectangle.zw - root_position);
        vec2 footprint = max(fwidth(root_position), vec2(1e-10));
        vec2 edge = distance / footprint;
        coverage = min(coverage, clamp(min(edge.x, edge.y) + 0.5, 0.0, 1.0));
    } else {
        for (uint i = 0; i < uint(clip_edges.x); ++i) {
            vec3 edge = paint_at(draw + 4u + i).xyz;
            if (all(equal(edge.xy,vec2(0.0)))) continue;
            float distance = dot(edge, vec3(root_position, 1.0));
            coverage = min(coverage, clamp(distance / max(fwidth(distance), 1e-10) + 0.5, 0.0, 1.0));
        }
    }

    world_point = world * vec4(point, 1.0);
    right_axis = (world * vec4(right, 0.0)).xyz;
    down_axis = (world * vec4(down, 0.0)).xyz;
    normal_axis = cross(down_axis, right_axis);
    vec3 oriented=(world * vec4(cross(down,right),0)).xyz;
    if (dot(normal_axis,oriented)<0.0) normal_axis=-normal_axis;
    return vec2(dot(paint_at(draw).xyz,vec3(root_position,1.0)),dot(paint_at(draw+1u).xyz,vec3(root_position,1.0)));
}
#endif
