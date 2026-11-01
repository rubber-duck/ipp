#version 300 es
// Retained triangle union: assets/fills.rs. Every covered interval blends once.
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
flat in int v_curve_start;
flat in int v_band_offset;
out vec4 o_color;
vec2 surface_position;
// SURFACE_INTERSECTION_FRAGMENT
// CANVAS_PAINTS
#ifndef IPP_CANVAS_PAINTS
vec4 ipp_canvas_paint(int slot, int block, vec2 position, vec2 size, vec4 color, float edge, vec4 data, vec2 canvasPosition) { return color; }
#endif

int fill_index(int index) {
    int address=v_band_offset+index;
    return int(texelFetch(u_bands,ivec2(address%u_band_width,address/u_band_width),0).r);
}

ivec4 fill_texel(int index) {
    int address=v_curve_start+index;
    return texelFetch(u_curves,ivec2(address%u_curve_width,address/u_curve_width),0);
}

vec4 fill_data(int index) { return intBitsToFloat(fill_texel(index)); }
float cross2(vec2 a,vec2 b) { return a.x*b.y-a.y*b.x; }
bool overlaps(vec4 a,vec4 b) { return all(lessThanEqual(a.xy,b.zw)) && all(lessThanEqual(b.xy,a.zw)); }

// A retained cell certifies its whole one-cell guard as interior or exterior.
// The index never approximates coverage: boundary cells and wider projected
// footprints keep the detailed query, including on curved and rear surfaces.
int uniform_coverage(vec2 p,vec2 margin,ivec4 header) {
    if(header.z==0) return 0;
    vec4 bounds=fill_data(1);
    vec2 cell_size=(bounds.zw-bounds.xy)/float(header.z);
    if(any(greaterThanEqual(margin,cell_size)) || any(lessThan(p,bounds.xy)) || any(greaterThanEqual(p,bounds.zw))) return 0;
    ivec2 cell=clamp(ivec2(floor((p-bounds.xy)/cell_size)),ivec2(0),ivec2(header.z-1));
    int index=cell.y*header.z+cell.x;
    return (fill_index(header.w+index/8) >> ((index%8)*2)) & 3;
}

// Direct local leaf lists remove a tree walk from ordinary fragments. The same
// conservative hierarchy and spatial fallback work for every source ordering.
ivec2 fill_candidates(vec2 p,vec2 footprint,int nodes) {
    int axis=fill_index(0),levels=fill_index(1),level=0;
    vec4 bounds=fill_data(1);
    float extent=max(bounds[axis+2]-bounds[axis],1e-12);
    int bins=fill_index(2);
    while(level+1<levels && footprint[axis]>extent/float(bins)) {
        level++; bins=fill_index(2+level*2);
    }
    int bin=clamp(int(floor((p[axis]-bounds[axis])/extent*float(bins))),0,bins-1);
    int header=fill_index(3+level*2)+bin*2;
    int first=fill_index(header),count=fill_index(header+1);
    return count>32 ? ivec2(-1,nodes) : ivec2(first,count);
}

bool triangle_inside(vec2 p,vec2 a,vec2 b,vec2 c) {
    return cross2(b-a,p-a)>=0.0 && cross2(c-b,p-b)>=0.0 && cross2(a-c,p-c)>=0.0;
}

vec2 half_plane(vec2 interval,vec2 p,vec2 d,vec2 a,vec2 b) {
    float position=cross2(b-a,p-a),direction=cross2(b-a,d);
    if(abs(direction)<1e-25) return position>=0.0 ? interval : vec2(1.0,-1.0);
    float boundary=-position/direction;
    return direction>0.0 ? vec2(max(interval.x,boundary),interval.y) : vec2(interval.x,min(interval.y,boundary));
}

vec2 triangle_interval(vec2 p,vec2 d,vec2 a,vec2 b,vec2 c) {
    return half_plane(half_plane(half_plane(vec2(-0.5,0.5),p,d,a,b),p,d,b,c),p,d,c,a);
}

vec2 nearest_edge(vec2 p,vec2 a,vec2 b) {
    vec2 ab=b-a;
    return a+ab*clamp(dot(p-a,ab)/max(dot(ab,ab),1e-30),0.0,1.0);
}

#ifdef IPP_CANVAS_PAINTS
bool fill_inside(vec2 p,int nodes) {
    ivec2 candidates=fill_candidates(p,vec2(0.0),nodes);
    int cursor=0;
    while(cursor<candidates.y) {
        int node=candidates.x<0 ? cursor : fill_index(candidates.x+cursor);
        vec4 bounds=fill_data(1+node*2);
        ivec4 meta=fill_texel(2+node*2);
        if(!overlaps(bounds,vec4(p,p))) { cursor=candidates.x<0 ? meta.x : cursor+1; continue; }
        for(int i=0;i<meta.z;i++) {
            vec4 ab=fill_data(meta.y+i*2); vec2 c=fill_data(meta.y+i*2+1).xy;
            if(triangle_inside(p,ab.xy,ab.zw,c)) return true;
        }
        cursor++;
    }
    return false;
}

float boundary_distance(vec2 p,vec2 a,vec2 b,int nodes) {
    vec2 q=nearest_edge(p,a,b),ab=b-a;
    float distance=length(q-p);
    if(distance>=2.0) return 2.0;
    vec2 normal=vec2(-ab.y,ab.x)*inversesqrt(max(dot(ab,ab),1e-30))*0.025;
    // Triangulation and overlapping source intervals introduce internal edges.
    // They are not paint boundaries, just as they never blend independently.
    return fill_inside(q+normal,nodes)!=fill_inside(q-normal,nodes) ? distance : 2.0;
}
#endif

float pixel_union_sweep(vec2 p,vec2 ray,vec4 query,ivec2 candidates,int triangles) {
    float cursor=-0.5,coverage=0.0;
    // Source order is arbitrary. Each pass consumes at least one interval end;
    // complete coverage exits immediately. Traversal visits local leaves only.
    for(int pass=0;pass<triangles*2+1 && cursor<0.5;pass++) {
        float end=cursor,next=0.5;
        int candidate=0;
        while(candidate<candidates.y) {
            int node=candidates.x<0 ? candidate : fill_index(candidates.x+candidate);
            vec4 bounds=fill_data(1+node*2);
            ivec4 meta=fill_texel(2+node*2);
            if(!overlaps(bounds,query)) { candidate=candidates.x<0 ? meta.x : candidate+1; continue; }
            for(int i=0;i<meta.z;i++) {
                vec4 ab=fill_data(meta.y+i*2); vec2 c=fill_data(meta.y+i*2+1).xy;
                vec2 span=triangle_interval(p,ray,ab.xy,ab.zw,c);
                if(span.x>span.y || span.y<=cursor) continue;
                if(span.x<=end) end=max(end,span.y);
                else next=min(next,span.x);
            }
            candidate++;
        }
        if(end>cursor) { coverage+=end-cursor; cursor=end; }
        else if(next>cursor) cursor=next;
        else break;
    }
    return clamp(coverage,0.0,1.0);
}

float pixel_union(vec2 p,vec2 ray,vec4 query,int nodes,int triangles) {
    ivec2 candidates=fill_candidates(p,(query.zw-query.xy)*0.5,nodes);
    // Merge local coverage as it arrives. Repeatedly scanning and refetching
    // triangles to discover the next interval is expensive for dense plots.
    // This bounded scratch space is independent of source count/order; overflow
    // restarts the complete sweep, never truncates or approximates coverage.
    vec2 intervals[8];
    int count=0,candidate=0;
    while(candidate<candidates.y) {
        int node=candidates.x<0 ? candidate : fill_index(candidates.x+candidate);
        vec4 bounds=fill_data(1+node*2);
        ivec4 meta=fill_texel(2+node*2);
        if(!overlaps(bounds,query)) { candidate=candidates.x<0 ? meta.x : candidate+1; continue; }
        for(int i=0;i<meta.z;i++) {
            vec4 ab=fill_data(meta.y+i*2); vec2 c=fill_data(meta.y+i*2+1).xy;
            vec2 span=triangle_interval(p,ray,ab.xy,ab.zw,c);
            if(span.y<=span.x) continue;
            int j=0;
            while(j<count) {
                if(span.x<=intervals[j].y && span.y>=intervals[j].x) {
                    span=vec2(min(span.x,intervals[j].x),max(span.y,intervals[j].y));
                    count--; intervals[j]=intervals[count]; j=0;
                } else j++;
            }
            if(span.y-span.x>=1.0) return 1.0;
            if(count==8) return pixel_union_sweep(p,ray,query,candidates,triangles);
            intervals[count++]=span;
        }
        candidate++;
    }
    float coverage=0.0;
    for(int i=0;i<count;i++) coverage+=intervals[i].y-intervals[i].x;
    return clamp(coverage,0.0,1.0);
}

void main() {
    surface_position=intersection_position(v_surface_position);
    vec2 p=u_intersection.x==0.0 ? v_path_position : (surface_position-v_path_placement.xy)/v_path_placement.zw;
    vec2 dx=dFdx(p),dy=dFdy(p),footprint=(abs(dx)+abs(dy))*0.5;
    vec2 clip_width=max(fwidth(surface_position),vec2(1.0/65536.0));
    vec2 clip_inside=min(surface_position-u_clip.xy,u_clip.zw-surface_position);
    float clip_coverage=clamp(min(clip_inside.x/clip_width.x+0.5,clip_inside.y/clip_width.y+0.5),0.0,1.0)*intersection_coverage;
    if(clip_coverage<=0.0) discard;
    ivec4 header=fill_texel(0);
    int nodes=header.x,cursor=0;
    vec2 margin=footprint+vec2(1e-5);
#ifdef IPP_CANVAS_PAINTS
    if(u_path_paint.x>0.0) margin=max(margin,vec2(2.0));
#endif
    int classification=uniform_coverage(p,margin,header);
    if(classification==2) discard;
    float coverage=1.0,edge=2.0;
    if(classification!=1) {
        vec4 query=vec4(p-margin,p+margin);
        float nearest=1e30;
        vec2 normal=vec2(0.0,1.0);
        bool inside=false;
        ivec2 candidates=fill_candidates(p,margin,nodes);
        while(cursor<candidates.y) {
            int node=candidates.x<0 ? cursor : fill_index(candidates.x+cursor);
            vec4 bounds=fill_data(1+node*2);
            ivec4 meta=fill_texel(2+node*2);
            if(!overlaps(bounds,query)) { cursor=candidates.x<0 ? meta.x : cursor+1; continue; }
            for(int i=0;i<meta.z;i++) {
                vec4 ab=fill_data(meta.y+i*2); vec2 c=fill_data(meta.y+i*2+1).xy;
                inside=inside || triangle_inside(p,ab.xy,ab.zw,c);
                int boundary=fill_texel(meta.y+i*2+1).z;
                vec2 vertices[3]; vertices[0]=ab.xy; vertices[1]=ab.zw; vertices[2]=c;

                for(int j=0;j<3;j++) {
                    if((boundary & (1 << j))==0) continue;
                    vec2 a=vertices[j],b=vertices[(j+1)%3];
                    // Most filled pixels are far from every true boundary. Avoid
                    // projections, roots and membership probes for distant edges.
                    if(!overlaps(vec4(min(a,b),max(a,b)),query)) continue;
                    vec2 q=nearest_edge(p,a,b),delta=p-q;
                    float distance=length(delta);

                    if(distance<nearest) {
                        nearest=distance;
                        vec2 tangent=b-a;
                        normal=distance>1e-12 ? delta/distance : vec2(-tangent.y,tangent.x)*inversesqrt(max(dot(tangent,tangent),1e-30));
                    }
#ifdef IPP_CANVAS_PAINTS
                    if(u_path_paint.x>0.0 && distance<edge) edge=min(edge,boundary_distance(p,a,b,nodes));
#endif
                }

            }
            cursor++;
        }
        // Exact shared edges are internal. A pixel inside the union and farther
        // than its footprint from every exterior candidate has full coverage.
        bool away_from_boundary=nearest>length(footprint);
        if(!inside && away_from_boundary) discard;
        bool full=inside && away_from_boundary;
        vec2 ray=abs(dot(normal,dx))>=abs(dot(normal,dy)) ? dx : dy;
        coverage=full ? 1.0 : pixel_union(p,ray,vec4(p-footprint-vec2(1e-5),p+footprint+vec2(1e-5)),nodes,header.y);
    }
    if(coverage<=0.0) discard;
    vec4 color=v_color;
#ifdef IPP_CANVAS_PAINTS
    if(u_path_paint.x>0.0) {
        color=clamp(ipp_canvas_paint(int(u_path_paint.x),int(u_path_paint.y),p-v_bounds.xy,v_bounds.zw-v_bounds.xy,v_color,-edge,vec4(0),surface_position),0.0,1.0);
        color.a*=u_path_paint.z;
    }
#endif
    o_color=vec4(color.rgb,color.a*coverage*clip_coverage);
}
