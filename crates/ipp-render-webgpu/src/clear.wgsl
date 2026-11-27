@vertex fn vertex(@builtin(vertex_index) i:u32) -> @builtin(position) vec4f {
    let p = array(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3));
    return vec4f(p[i],1,1);
}

@fragment fn fragment()->@location(0) vec4f {
    return vec4f(0.003095975,0.004400849,0.007194409,1);
}
