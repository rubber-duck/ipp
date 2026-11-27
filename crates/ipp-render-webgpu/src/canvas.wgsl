struct Vertex {
    @location(0) clip: vec4f,
    @location(1) local: vec2f,
    @location(2) world: vec4f,
    @location(3) right: vec3f,
    @location(4) down: vec3f,
    @location(5) normal: vec3f,
    @location(6) slot: u32,
}

struct Raster {
    @builtin(position) position: vec4f,
    @location(0) local: vec2f,
    @location(1) @interpolate(flat) slot: u32,
    @location(2) world: vec4f,
    @location(3) right: vec3f,
    @location(4) down: vec3f,
    @location(5) normal: vec3f,
}

@vertex fn canvas_vertex(input: Vertex) -> Raster {
    var clip = input.clip;
    clip.z = (clip.z + clip.w) * 0.5;
    return Raster(clip, input.local, input.slot, input.world, input.right, input.down, input.normal);
}
