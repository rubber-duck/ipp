# Glass visual language

The liquid glass study guides the current skin; the earlier natural-glass reference remains historical. [GUI architecture](../../../../docs/architecture/gui.md) owns behavior; [rendering architecture](../../../../docs/architecture/rendering.md#presentation-aware-paints) owns view and light inputs.

## Surfaces

Treat controls as thin sheets of smoked liquid glass: capsule ends for standalone controls, rounded rectangular expanded surfaces and polished tubes for rings. Large panel backgrounds use slightly transparent solid color. Use quiet translucent faces and rounded polished edges that reflect the chosen light and view. Avoid drawn neon borders, corner shards and broad glow. Simulate the material locally; do not add scene reflection or blur passes.

Connected controls share a continuous contour and reflection field. An open dropdown extends its trigger’s sheet. Tabs, segments, rows and inset cells divide a parent surface with restrained tints and separators instead of restarting its bevel. Keep popup content readable over the application.

Keep the polished rim narrow and the face soft. The [liquid sheet shader](liquid-material.ts) integrates both silhouette coverage and premultiplied optical color within each physical edge pixel, accounting for the native contour's existing coverage. Sampling the varying normals and highlights together with coverage lets a partially visible rim contribute even when the pixel center misses it. The flat interior needs only one optical evaluation; the rim uses a bounded sample grid within the same paint pass. Higher display density should reveal a sharper contour without thickening the rim or introducing jagged highlights.

## State and color

Keep idle reflections dim, including selected and checked controls, with no external glow. Hover, focus and press may raise the bounded material highlights. Selection retains a useful tint, focus stays visible, and disabled values remain legible. Native state and geometry own interaction; decoration must not change values or hit targets.

Palette roles are independent. Primary and secondary tint the material; semantic colors communicate status. Exact color fields, swatches and chart data colors retain their meaning. Use consistent compact proportions and content insets, as defined by the [tokens](tokens.ts) and [metrics](metrics.ts).

## Motion and charts

Applications choose lighting and animation; the skin never binds cursor input implicitly. Light and view changes should feel continuous across a connected surface. Native feedback and decorative animation follow the Host clock and reduced-motion preference.

Charts use the same glass language while preserving silhouettes, topology, data enclosure and picking. Keep grids and labels quieter than marks. Selection strengthens the material response without adding wire clutter; curved surfaces must not reveal tessellation seams.

The [liquid sheet shader](liquid-material.ts), [liquid arc shader](liquid-arc-material.ts) and [chart materials](chart.tsx) own implementation details. [Rendering scenarios](../../../../tests/gui/glass-skin/README.md) exercise those decisions on native and browser Hosts.
