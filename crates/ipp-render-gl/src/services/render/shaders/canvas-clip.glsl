uniform highp vec4 u_canvas_planes[16];
float canvas_clip(vec2 p) {
    for (int i=0; i<16; ++i) {
        vec4 plane=u_canvas_planes[i];
        if (plane.w==0.0) break;
        float side=dot(plane.xy,p)+plane.z;
        if (side<0.0 || (side==0.0 && plane.w<0.0)) return 0.0;
    }
    return 1.0;
}
