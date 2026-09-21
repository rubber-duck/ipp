# Texture Payloads

[Decoder](texture.rs) · [Rendering guide](../../../../../docs/development/rendering.md#textures-and-built-in-sources)

The IPPT format supplies immutable texture pixels. The decoder validates the exact payload length; graphics loading applies the following sampling and upload settings.

| IPPT v3 | Contract |
| --- | --- |
| Header | `IPPT`, little-endian u32 version/width/height |
| Pixels | Tightly packed RGBA8, exact payload length, top row first |
| Color | sRGB colour bytes and linear straight coverage alpha; older versions are rejected |
| Sampling | UV (0,0) top-left; nearest/repeat, no mipmaps |
| GL upload | `SRGB8_ALPHA8`, `RGBA`, `UNSIGNED_BYTE`, unpack alignment 1 |

Opaque mesh materials continue to ignore texture alpha. Surface bitmaps multiply coverage alpha by their item tint alpha and opacity, then blend in linear colour space.
