// Spotlight atlas filtering in receiver-plane depth, including a finite emitter.
@group(3) @binding(0) var shadow_map: texture_2d<f32>;

fn shadow_contains(uv:vec2f)->bool {
    return all(uv>=vec2f(0)) && all(uv<=vec2f(1));
}

fn shadow_depth(uv:vec2f,s:vec4f)->f32 {
    if !shadow_contains(uv) { return 1.0; }
    let tile=vec2f(s.x % s.w,floor(s.x/s.w));
    let bounded=clamp(uv,vec2f(s.z*0.5),vec2f(1.0-s.z*0.5));
    let pixel=(tile+vec2f(bounded.x,1.0-bounded.y))/s.w*vec2f(textureDimensions(shadow_map));
    return textureLoad(shadow_map,vec2i(pixel),0).r;
}

fn shadow_texel(uv:vec2f,s:vec4f)->vec2f { return (floor(uv/s.z)+0.5)*s.z; }

fn receiver_gradient(n:vec3f,p:vec4f,m:mat4x4f)->vec2f {
    let tangent=cross(n,select(vec3f(1,0,0),vec3f(0,1,0),abs(n.y)<0.99));
    let a=m*vec4f(tangent,0); let b=m*vec4f(cross(n,tangent),0);
    let da=(a.xyz-p.xyz*(a.w/p.w))/p.w; let db=(b.xyz-p.xyz*(b.w/p.w))/p.w;
    let det=da.x*db.y-db.x*da.y;
    if abs(det)<1e-10 {return vec2f(0);}
    return vec2f(da.z*db.y-db.z*da.y,da.x*db.z-db.x*da.z)/det;
}

fn shadow_distance(depth:f32,near:f32,far:f32)->f32 {return near*far/(far-depth*(far-near));}

fn visibility(index:u32,nl:f32,n:vec3f,position:vec3f)->f32 {
    let s=camera.shadow_settings[index];
    if s.x<0.0 {return 1.0;}
    let m=camera.shadow_matrices[index]; let p=m*vec4f(position,1.0);
    if p.w<=0.0 {return 1.0;}
    let uvz=p.xyz/p.w*0.5+0.5;
    if any(uvz<=vec3f(0)) || any(uvz>=vec3f(1)) {return 1.0;}
    let bias=s.y*(1.0+2.0*(1.0-nl));
    let gradient=receiver_gradient(n,p,m);
    let light=camera.lights[index].cone;
    var visible=0.0;
    if light.y<=0.0 {
        for(var y=-1;y<=1;y++) { for(var x=-1;x<=1;x++) {
            let uv=shadow_texel(uvz.xy+vec2f(f32(x),f32(y))*s.z,s);
            let depth=shadow_depth(uv,s);
            visible+=select(0.0,1.0,depth>=1.0 || uvz.z+dot(gradient,uv-uvz.xy)-bias<=depth);
        }}
        return visible/9.0;
    }
    let search=min(1.414214,light.y*(p.w-light.z)/(p.w*light.z));
    let center=shadow_texel(uvz.xy,s);
    let d=shadow_depth(center,s);
    let blocked=d<1.0 && d<uvz.z+dot(gradient,center-uvz.xy)-bias;
    var sum=select(0.0,shadow_distance(d,light.z,light.w),blocked);
    var count=select(0.0,1.0,blocked);
    for (var i=0;i<32;i++) {
        let distance=s.z*pow(max(1.0,search/s.z),(f32(i)+0.5)/32.0);
        let angle=f32(i)*2.39996323;
        let uv=shadow_texel(uvz.xy+vec2f(cos(angle),sin(angle))*distance,s);
        if !shadow_contains(uv) {continue;}
        let depth=shadow_depth(uv,s);
        if depth<1.0 && depth<uvz.z+dot(gradient,uv-uvz.xy)-bias {
            sum+=shadow_distance(depth,light.z,light.w); count+=1.0;
        }
    }
    if count==0.0 {return 1.0;}
    let blocker=sum/count;
    let radius=min(1.414214,max(s.z,light.y/p.w*((p.w-blocker)/blocker)));
    for (var i=0;i<48;i++) {
        let angle=f32(i)*2.39996323;
        let disk=vec2f(cos(angle),sin(angle))*sqrt((f32(i)+0.5)/48.0);
        let uv=shadow_texel(uvz.xy+disk*radius,s);
        let depth=shadow_depth(uv,s);
        visible+=select(0.0,1.0,depth>=1.0 || uvz.z+dot(gradient,uv-uvz.xy)-bias<=depth);
    }
    return visible/48.0;
}

@fragment fn shadow_fragment(v:Raster)->@location(0) f32 { return v.clip.z; }
