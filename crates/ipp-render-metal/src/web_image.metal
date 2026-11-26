// BGRA bytes arrive premultiplied in encoded sRGB. Decode after separating
// straight color and store premultiplied linear pixels in renderer-owned storage.
kernel void import_web_image(texture2d<float, access::read> source [[texture(0)]],
                             texture2d<half, access::write> destination [[texture(1)]],
                             uint2 point [[thread_position_in_grid]]) {
    if (point.x >= source.get_width() || point.y >= source.get_height()) return;
    float4 pixel = source.read(point);
    float3 straight = pixel.a > 0.0 ? clamp(pixel.rgb / pixel.a, 0.0, 1.0) : float3(0.0);
    float3 linear = select(
        pow((straight + 0.055) / 1.055, float3(2.4)),
        straight / 12.92,
        straight <= float3(0.04045)
    );
    destination.write(half4(float4(linear * pixel.a, pixel.a)), point);
}
