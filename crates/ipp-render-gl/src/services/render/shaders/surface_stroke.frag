#version 300 es
// Compact round-capped stroke union. Layout: assets/strokes.rs.
precision highp float;
precision highp int;
uniform highp isampler2D u_curves;
uniform int u_curve_width;
uniform highp usampler2D u_bands;
uniform int u_band_width;
uniform vec4 u_clip;
uniform vec4 u_path_paint;
in vec2 v_path_position;
flat in vec4 v_path_placement;
in vec2 v_surface_position;
in vec4 v_color;
flat in vec4 v_bounds;
flat in int v_band_offset;
flat in int v_curve_start;
out vec4 o_color;
vec2 surface_position;
// SURFACE_INTERSECTION_FRAGMENT
// CANVAS_PAINTS
#ifndef IPP_CANVAS_PAINTS
vec4 ipp_canvas_paint(int slot, int block, vec2 position, vec2 size, vec4 color, float edge, vec4 data, vec2 canvasPosition) { return color; }
#endif

int stroke_index(int index) {
    int address=v_band_offset+index;
    return int(texelFetch(u_bands,ivec2(address%u_band_width,address/u_band_width),0).r);
}

vec4 stroke_data(int index) {
    int address=v_curve_start+index;
    return intBitsToFloat(texelFetch(u_curves,ivec2(address%u_curve_width,address/u_curve_width),0));
}

// Intersect a one-pixel ray p+t*d with a capsule. Its strip and endpoint
// circles form one convex interval, so no polygon winding or curve roots are
// needed. The interval is clipped to the pixel's [-.5,.5] footprint.
vec2 circle_interval(vec2 p,vec2 d,vec2 center,float radius) {
    vec2 q=p-center;
    float a=dot(d,d),b=dot(q,d);
    float perpendicular=q.x*d.y-q.y*d.x;
    float determinant=a*radius*radius-perpendicular*perpendicular;
    if(a<1e-30 || determinant<0.0) return vec2(1.0,-1.0);
    float half_width=sqrt(max(determinant,0.0))/a;
    return vec2(max(-0.5,-b/a-half_width),min(0.5,-b/a+half_width));
}

vec2 clip_interval(vec2 interval,float position,float direction,float low,float high) {
    if(abs(direction)<1e-20) return position>=low && position<=high ? interval : vec2(1.0,-1.0);
    vec2 ends=(vec2(low,high)-position)/direction;
    return vec2(max(interval.x,min(ends.x,ends.y)),min(interval.y,max(ends.x,ends.y)));
}

vec2 capsule_interval(vec2 p,vec2 d,vec4 segment,float radius) {
    vec2 ab=segment.zw-segment.xy,q=p-segment.xy;
    float length2=dot(ab,ab),side=radius*sqrt(length2);
    vec2 strip=clip_interval(vec2(-0.5,0.5),dot(q,ab),dot(d,ab),0.0,length2);
    strip=clip_interval(strip,q.x*ab.y-q.y*ab.x,d.x*ab.y-d.y*ab.x,-side,side);
    vec2 result=strip.x<=strip.y ? strip : vec2(1.0,-1.0);
    vec2 start=circle_interval(p,d,segment.xy,radius),end=circle_interval(p,d,segment.zw,radius);
    if(start.x<=start.y) result=vec2(min(result.x,start.x),max(result.y,start.y));
    if(end.x<=end.y) result=vec2(min(result.x,end.x),max(result.y,end.y));
    return result;
}

float stroke_pixel_union(vec2 p,vec2 d,float radius,int first,int count) {
    float cursor=-0.5,coverage=0.0;
    // Extend each covered interval to its fixed point, then advance across the
    // next empty gap. Source order need not be spatial order. Every iteration
    // consumes an endpoint; no candidate truncation or fixed array is used.
    for(int pass=0;pass<count*2+1 && cursor<0.5;pass++) {
        float end=cursor,next=0.5;
        for(int i=0;i<count;i++) {
            vec2 span=capsule_interval(p,d,stroke_data(stroke_index(first+i)),radius);
            if(span.x>span.y || span.y<=cursor) continue;
            if(span.x<=end) end=max(end,span.y);
            else next=min(next,span.x);
        }
        if(end>cursor) { coverage+=end-cursor; cursor=end; }
        else if(next>cursor) cursor=next;
        else break;
    }
    return clamp(coverage,0.0,1.0);
}

void main() {
    surface_position=intersection_position(v_surface_position);
    vec2 p=u_intersection.x==0.0 ? v_path_position : (surface_position-v_path_placement.xy)/v_path_placement.zw;
    // Compute derivatives before any divergent candidate lookup or discard.
    vec2 dx=dFdx(p),dy=dFdy(p);
    vec2 footprint=abs(dx)+abs(dy);
    vec2 clip_width=max(fwidth(surface_position),vec2(1.0/65536.0));
    vec2 clip_inside=min(surface_position-u_clip.xy,u_clip.zw-surface_position);
    float clip_coverage=clamp(min(clip_inside.x/clip_width.x+0.5,clip_inside.y/clip_width.y+0.5),0.0,1.0)*intersection_coverage;
    int axis=stroke_index(0),levels=stroke_index(1),level=0;
    float extent=max(v_bounds[axis+2]-v_bounds[axis],1e-12);
    int bins=stroke_index(2);
    while(level+1<levels && footprint[axis]>extent/float(bins)) {
        level++; bins=stroke_index(2+level*2);
    }
    int bin=clamp(int(floor((p[axis]-v_bounds[axis])/extent*float(bins))),0,bins-1);
    int header=stroke_index(3+level*2)+bin*2;
    int first=stroke_index(header),count=stroke_index(header+1);
    float radius=stroke_data(0).x;
    float distance_to_line=1e30;
    vec2 nearest_normal=vec2(1.0,0.0);
    for(int i=0;i<count;i++) {
        vec4 segment=stroke_data(stroke_index(first+i));
        vec2 ab=segment.zw-segment.xy;
        float length2=max(dot(ab,ab),1e-30);
        float t=clamp(dot(p-segment.xy,ab)/length2,0.0,1.0);
        vec2 delta=p-(segment.xy+ab*t);
        float d=length(delta);
        if(d<distance_to_line) {
            distance_to_line=d;
            nearest_normal=d>1e-12 ? delta/d : vec2(-ab.y,ab.x)*inversesqrt(length2);
        }
    }
    float aa=max(length(vec2(dot(nearest_normal,dx),dot(nearest_normal,dy))),1.0/65536.0);
    // Both edges contribute when a stroke is thinner than a pixel. A lone
    // distance ramp would incorrectly paint a zero-width line at half opacity.
    float coverage=clamp((radius-distance_to_line)/aa+0.5,0.0,1.0)-clamp((-radius-distance_to_line)/aa+0.5,0.0,1.0);
    if(radius < aa * 0.75) {
        vec2 ray=abs(dot(nearest_normal,dx))>=abs(dot(nearest_normal,dy)) ? dx : dy;
        coverage=stroke_pixel_union(p,ray,radius,first,count);
    }
    vec4 color=v_color;
#ifdef IPP_CANVAS_PAINTS
    if(u_path_paint.x>0.0) {
        color=clamp(ipp_canvas_paint(int(u_path_paint.x),int(u_path_paint.y),p-v_bounds.xy,v_bounds.zw-v_bounds.xy,v_color,min(distance_to_line-radius,0.0),vec4(0),surface_position),0.0,1.0);
        color.a*=u_path_paint.z;
    }
#endif
    if(coverage<=0.0 || clip_coverage<=0.0) discard;
    o_color=vec4(color.rgb,color.a*coverage*clip_coverage);
}
