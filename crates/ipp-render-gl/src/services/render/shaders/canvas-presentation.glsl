// Presentation inputs share the device's ordinary lighting upload. In this
// program u_normal maps Canvas coordinates into centred Surface coordinates;
// u_surface is [mapping kind, signed curvature, normal offset]. No scene samples.
uniform mat4 u_model;
uniform mat4 u_normal;
uniform vec3 u_surface;
uniform vec4 u_camera;
uniform vec4 u_lights[IPP_MAX_LIGHTS * 4];
uniform int u_light_count;

vec3 ippPaintPoint(vec2 canvas, out mat3 basis) {
    vec3 q=(u_normal*vec4(canvas,0.0,1.0)).xyz;
    vec3 p=vec3(q.xy,u_surface.z);
    vec3 tx=vec3(1,0,0),ty=vec3(0,-1,0),normal=vec3(0,0,1);
    float k=u_surface.y;
    if(u_surface.x>.5 && abs(k)>1e-7) {
        bool sphere=u_surface.x>1.5;
        float r=sphere?length(q.xy):abs(q.x);
        float a=sphere?k*r:k*q.x;
        float a2=a*a;
        float sinc=abs(a)<.001?1.0-a2/6.0+a2*a2/120.0:sin(a)/a;
        float cosm1=abs(a)<.001?-a2*.5+a2*a2/24.0:cos(a)-1.0;
        normal=vec3(k*q.x*sinc,sphere?k*q.y*sinc:0.0,cos(a));
        p=vec3(q.x*sinc,sphere?q.y*sinc:q.y,cosm1/k)+normal*u_surface.z;
        if(sphere) {
            vec2 radial=r>1e-8?q.xy/r:vec2(0);
            float d=cos(a)-sinc;
            tx=vec3(sinc+radial.x*radial.x*d,radial.x*radial.y*d,-sin(a)*radial.x);
            ty=-vec3(radial.x*radial.y*d,sinc+radial.y*radial.y*d,-sin(a)*radial.y);
        } else {
            tx=vec3(cos(a),0,-sin(a));
        }
    }
    vec3 X=normalize(mat3(u_model)*tx);
    vec3 Y=mat3(u_model)*ty;
    vec3 N=normalize(cross(Y,X))*sign(determinant(mat3(u_model)));
    Y=normalize(cross(X,N));
    basis=mat3(X,Y,N);
    return (u_model*vec4(p,1.0)).xyz;
}

vec3 ippPaintView(vec2 canvas) {
    mat3 basis;
    vec3 p=ippPaintPoint(canvas,basis);
    vec3 v=u_camera.w>.5?u_camera.xyz:u_camera.xyz-p;
    return transpose(basis)*(v*inversesqrt(max(dot(v,v),1e-20)));
}

vec3 ippPaintLightDirection(int i,vec2 canvas) {
    if(i<0 || i>=u_light_count)return vec3(0,0,1);
    mat3 basis;
    vec3 p=ippPaintPoint(canvas,basis);
    vec4 light=u_lights[i*4];
    vec3 delta=light.w<.5?-u_lights[i*4+1].xyz:light.xyz-p;
    return transpose(basis)*(delta*inversesqrt(max(dot(delta,delta),1e-20)));
}

vec3 ippPaintLightRadiance(int i,vec2 canvas) {
    if(i<0 || i>=u_light_count)return vec3(0);
    mat3 basis;
    vec3 p=ippPaintPoint(canvas,basis);
    vec4 light=u_lights[i*4],colorRange=u_lights[i*4+2];
    vec3 delta=light.xyz-p;
    float attenuation=1.0;
    if(light.w>.5) {
        float d2=max(dot(delta,delta),.0001);
        float ratio=sqrt(d2)/max(colorRange.w,.0001);
        attenuation=clamp(1.0-pow(ratio,4.0),0.0,1.0)/d2;
    }
    if(light.w>1.5) {
        vec4 direction=u_lights[i*4+1];
        float outer=u_lights[i*4+3].x;
        float cone=clamp((dot(-delta*inversesqrt(max(dot(delta,delta),1e-20)),direction.xyz)-outer)
            /max(direction.w-outer,1e-6),0.0,1.0);
        attenuation*=cone*cone;
    }
    return colorRange.rgb*attenuation;
}
