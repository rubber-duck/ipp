uniform vec4 u_intersection;
uniform mat4 u_intersection_projection;
uniform mat4 u_intersection_inverse;
uniform vec4 u_viewport;
flat in vec4 v_intersection_chart;
float intersection_coverage = 1.0;

// All invocations, including misses, produce finite coordinates before any
// derivative or discard. The stable quadratic avoids subtracting a huge centre
// as curvature approaches zero, matching the headless provider's equation.
vec2 intersection_position(vec2 planar) {
    gl_FragDepth = gl_FragCoord.z;
    if (u_intersection.x == 0.0) return planar;
    vec2 ndc = gl_FragCoord.xy / u_viewport.xy * 2.0 - 1.0;
    vec4 n = u_intersection_inverse * vec4(ndc,-1.0,1.0);
    vec4 m = u_intersection_inverse * vec4(ndc,0.0,1.0);
    vec3 origin = n.xyz/n.w;
    vec3 direction = normalize(m.xyz/m.w-origin);
    vec3 axes = u_intersection.x < 1.5 ? vec3(1,0,1) : vec3(1);
    float k = u_intersection.y;
    float a = k*dot(direction*axes,direction);
    float b = k*dot(origin*axes,direction)+direction.z;
    float c = k*dot(origin*axes,origin)+2.0*origin.z;
    float discriminant = b*b-a*c;
    float root = sqrt(max(discriminant,0.0));
    float q = -b-(b < 0.0 ? -root : root);
    float safe_a = a != 0.0 ? a : 1.0;
    float t0 = q/safe_a;
    float t1 = abs(q)>1e-20 ? c/q : -b/safe_a;
    float t = u_intersection.z > 0.5 ? min(t0,t1) : max(t0,t1);
    if (discriminant < 0.0) t = -b/safe_a;
    // The chart map is nonexpanding: the complete domain lies in the ball
    // supplied in .w. Reject remote roots before multiplying large distances;
    // this keeps the flat limit finite even for subnormal curvature.
    float limit = length(origin)+u_intersection.w;
    bool valid_equation = true;
    if (a == 0.0) {
        valid_equation = b != 0.0 && u_intersection.z > 0.5;
        t = b != 0.0 ? -c/(2.0*b) : 0.0;
    }
    bool valid_distance = valid_equation && t >= 0.0 && t <= limit;
    t = clamp(t,0.0,limit);
    vec3 point = origin+direction*t;
    vec2 chart;
    bool valid_chart = true;
    if (u_intersection.x < 1.5) {
        float angle=atan(k*point.x,1.0+k*point.z);
        valid_chart=abs(angle)<3.14159265;
        float denominator = 1.0+k*point.z;
        float ratio = point.x/max(denominator,1e-20);
        float small = k*ratio;
        float x = denominator>0.0 && abs(small)<0.0001 ? ratio*(1.0-small*small/3.0) : angle/k;
        chart = vec2(x, point.y);
    } else {
        float radial = length(k*point.xy);
        float angle = atan(radial,1.0+k*point.z);
        valid_chart=angle<3.14159265;
        float denominator = 1.0+k*point.z;
        float small = radial/max(denominator,1e-20);
        float scale = denominator>0.0 && small<0.0001 ? (1.0-small*small/3.0)/denominator : angle/max(radial,1e-20);
        chart = point.xy*scale;
    }
    vec4 projected = u_intersection_projection * vec4(point,1.0);
    float depth = projected.z/projected.w*0.5+0.5;
    gl_FragDepth = clamp(depth,0.0,1.0);
    float width = max(fwidth(discriminant),1e-20);
    intersection_coverage = clamp(discriminant/width+0.5,0.0,1.0);
    if (!valid_chart || !valid_distance || depth < 0.0 || depth > 1.0 || projected.w <= 0.0) intersection_coverage=0.0;
    return (chart-v_intersection_chart.xy)/v_intersection_chart.zw;
}
