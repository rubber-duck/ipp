#ifdef IPP_PLOT
layout(location=0) in vec3 plot_position;
layout(location=1) in vec3 plot_color;
layout(location=2) in vec2 plot_uv;
layout(location=5) in vec3 plot_normal;
#define a_position plot_position
#define a_color plot_color
#define a_normal plot_normal
#define a_uv plot_uv
#define a_weight 1.0
#else
layout(location=0) in vec4 position;
layout(location=1) in vec4 color_weight;
layout(location=2) in vec4 uv;
layout(location=3) in uvec4 joints;
layout(location=4) in vec4 joint_weights;
layout(location=5) in vec4 normal;
layout(location=6) in vec4 pose_position;
layout(location=7) in vec4 pose_normal;
#define a_position position.xyz
#define a_color color_weight.xyz
#define a_normal normal.xyz
#define a_uv uv.xy
#define a_weight color_weight.w
#endif
layout(location=8) in vec4 model_x;
layout(location=9) in vec4 model_y;
layout(location=10) in vec4 model_z;
layout(location=11) in vec4 model_w;
layout(location=12) in vec4 tint;
layout(location=13) in vec4 options;
layout(location=14) in vec4 surface;
layout(set=2,binding=0,std140) uniform Skin {mat4 palette[IPP_MAX_JOINTS];};
#define u_model mat4(model_x,model_y,model_z,model_w)
#define u_mvp (projection*u_model)
mat3 ippNormalBasis(mat3 basis) {
    vec3 x = cross(basis[1], basis[2]);
    vec3 y = cross(basis[2], basis[0]);
    vec3 z = cross(basis[0], basis[1]);
    return mat3(x, y, z) * (1.0 / dot(basis[0], x));
}
mat4 ippAffineNormal(mat4 model) {
    mat3 n = ippNormalBasis(mat3(model));
    vec3 t = model[3].xyz;
    return mat4(vec4(n[0], -dot(t, n[0])), vec4(n[1], -dot(t, n[1])),
                vec4(n[2], -dot(t, n[2])), vec4(0.0, 0.0, 0.0, 1.0));
}
#define u_normal ippAffineNormal(u_model)
#define ipp_position gl_Position
void ippDefaultVertex() {
#ifdef IPP_PLOT
    vec4 p=vec4(a_position, 1.0);
    vec3 n=a_normal;
#else
    vec4 p=mix(position,pose_position,options.x);
    vec3 n=mix(normal.xyz,pose_normal.xyz,options.x);
    if (options.y>0.5) {
        mat4 skin=palette[joints.x]*joint_weights.x+palette[joints.y]*joint_weights.y+palette[joints.z]*joint_weights.z+palette[joints.w]*joint_weights.w;
        p=skin*p;
        n=ippNormalBasis(mat3(skin))*n;
    }
#endif
    gl_Position=u_mvp*p;
    v_position=(u_model*p).xyz;
    v_normal=mat3(u_normal)*n;
    v_normal*=inversesqrt(max(dot(v_normal,v_normal),1e-20));
    v_color=a_color*tint.rgb;
    v_weight=a_weight;
    v_uv=a_uv;
}
