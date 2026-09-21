# Surface Font and Drawing Payloads

[Assets](../../../../../docs/architecture/assets.md) · [Rendering](../../../../../docs/architecture/rendering.md)

The offline converter in `tools/convert_surface_asset.py` produces renderer-independent, little-endian `IPPF` font and `IPPD` drawing payloads. Runtime loaders validate complete payloads and retain only portable CPU data. Renderers derive bands, vertices and GPU resources without changing the immutable decoded asset.

Install the pinned Python tools with `python tools/ipp.py setup python`, then run the converter in that development environment:

```sh
.venv/bin/python tools/convert_surface_asset.py font input.ttf output.ippf
.venv/bin/python tools/convert_surface_asset.py svg input.svg output.ippd --tolerance 0.01
```

On Windows, use `.venv/Scripts/python.exe`. SVG tolerance is measured in converted source units; choose it for the largest intended display scale. Font outlines retain their original quadratic geometry. The [terminal example](../../../../../examples/surface-terminal/README.md) shows runtime placement and asset use.

Every contour starts with two finite `f32` coordinates and a positive `u32` segment count. A segment is a one-byte kind followed by three zero bytes. Kind 0 contains a two-`f32` line endpoint. Kind 1 contains a two-`f32` control point and a two-`f32` endpoint. Contours are closed; closure from the last endpoint to the start is implicit.

## IPPF version 1

The header contains `IPPF`, `u32` version 1, `u32` units per em, three `f32` line metrics, and `u32` glyph, cmap and kerning counts. Glyph records stay in original glyph ID order and contain advance, left bearing, four-coordinate bounds, a contour count and contours. Glyph zero is `.notdef`. Sorted cmap records contain `u32` Unicode scalar and glyph ID. Sorted kerning records contain two glyph IDs and an `f32` horizontal adjustment. Static TrueType `glyf` fonts are accepted; variable fonts must be instantiated before conversion.

## IPPD version 1

The header contains `IPPD`, `u32` version 1, four-`f32` Y-down view-box bounds, four-`f32` converted bounds, positive `f32` quadratic tolerance and a layer count. Each painter-ordered layer contains sRGB RGB plus linear alpha bytes, a fill-rule byte (0 nonzero, 1 even-odd), three zero bytes, a contour count and contours.

The SVG converter accepts paths, rectangles including rounded corners, circles, ellipses, lines, polylines, polygons, groups, affine transforms, solid fill/stroke paint, fill rules, paint opacity, line caps/joins and dashes. It expands strokes to fills and converts cubic and arc segments to quadratics within the recorded output-coordinate tolerance, accounting for the largest affine scale. Definitions are validated but do not paint because reusable references are outside the subset. It rejects unsupported elements, attributes, style properties, nested viewports and isolated opacity with element context instead of discarding visual semantics.
