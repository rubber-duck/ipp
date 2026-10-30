#version 300 es
precision highp float;
uniform mat4 u_mvp;
uniform vec4 u_viewport;
uniform vec4 u_placement; // position.xy, size.xy
out vec2 v_uv;
flat out vec4 v_bitmap_placement;
out vec2 v_surface_position;
// SURFACE_INTERSECTION_VERTEX
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(1 - ((gl_VertexID >> 1) & 1)));
    vec2 local = corner * u_placement.zw;
    v_surface_position = u_placement.xy + local;
    v_uv = corner;
    v_bitmap_placement = u_placement;
    gl_Position = u_mvp * vec4(v_surface_position, 0.0, 1.0);
    if (u_intersection.x != 0.0) {
        vec2 a=u_placement.xy, b=a+u_placement.zw;
        gl_Position=intersection_vertex(corner,vec4(min(a,b),max(a,b)));
    }
}
