// Contours may have a signed base winding per cell. Band and cell indices
// both address the original edge records in path order.
float cross2(vec2 a, vec2 b) {
    return a.x * b.y - a.y * b.x;
}

bool path_contains(uint records, vec2 point, float mode, float radius) {
    vec4 header = record_at(records + uint(0));
    vec4 range;
    uint indices;
    if (header.x < 0.0) {
        vec4 domain = record_at(records + uint(1));
        uint side = uint(-header.x);
        if (point.x < domain.x || point.x > domain.z || point.y < header.y || point.y >= domain.w) return false;
        uvec2 cell = min(uvec2((point - vec2(domain.x, header.y)) * vec2(domain.y, header.z)), side - 1);
        range = record_at(records + uint(2 + cell.y * side + cell.x));
        indices = 2 + side * side;
    } else {
        int row = int(floor((point.y - header.y) * header.z));
        if (row < 0 || row >= int(header.x)) return false;
        range = record_at(records + uint(row + 1));
        indices = 1 + uint(header.x);
    }
    uint begin = uint(range.x);
    uint end = begin + uint(range.y);
    int winding = int(range.z);
    for (uint i = begin; i < end; ++i) {
        uint element = uint(record_at(records + uint(indices + i / 4))[i % 4]);
        uint record = uint(header.w) + element * 2;
        vec4 pair = record_at(records + uint(record));
        vec2 a = pair.xy;
        vec2 b = pair.zw;
        vec4 endpoint = record_at(records + uint(record + 1));
        vec2 c = endpoint.xy;
        if (mode > 5.5) {
            vec2 axis = b - a;
            float length_squared = dot(axis, axis);
            float t = length_squared > 0.0
                ? clamp(dot(point - a, axis) / length_squared, 0.0, 1.0)
                : 0.0;
            vec2 delta = point - (a + t * axis);
            if (dot(delta, delta) < radius * radius) return true;
        } else if (mode > 4.5) {
            float u = cross2(b - a, point - a);
            float v = cross2(c - b, point - b);
            float w = cross2(a - c, point - c);
            if ((u > 0.0 || v > 0.0 || w > 0.0) && min(u, min(v, w)) >= 0.0) return true;
            if ((u < 0.0 || v < 0.0 || w < 0.0) && max(u, max(v, w)) <= 0.0) return true;
        } else {
            if (point.y < min(a.y, c.y) || point.y >= max(a.y, c.y)) continue;
            float direction = sign(c.y - a.y);
            float quadratic = endpoint.z;
            float linear = endpoint.w;
            float constant_term = a.y - point.y;
            float t;
            if (quadratic == 0.0) {
                t = clamp(-constant_term / linear, 0.0, 1.0);
            } else {
                float root = sqrt(max(linear * linear - 4.0 * quadratic * constant_term, 0.0));
                float divisor = linear + direction * root;
                t = divisor == 0.0 ? 0.0 : clamp(-2.0 * constant_term / divisor, 0.0, 1.0);
            }
            float x = mix(mix(a.x, b.x, t), mix(b.x, c.x, t), t);
            if (x > point.x) winding += int(direction);
        }
    }
    return mode > 3.5 ? (winding & 1) != 0 : winding != 0;
}

float path_coverage(uint records, vec2 local, float mode, float radius,
                    vec2 dx, vec2 dy) {
    float coverage = 0.0;
    for (int x = 0; x < 2; ++x) {
        for (int y = 0; y < 2; ++y) {
            vec2 point = local + dx * (float(x) * 0.5 - 0.25) + dy * (float(y) * 0.5 - 0.25);
            coverage += path_contains(records, point, mode, radius) ? 0.25 : 0.0;
        }
    }
    return coverage;
}
