# Canvas paint interface 1

A [CanvasPaint](../../../../../ipp-core/src/world/systems/canvas/component.rs) fills its entity's own box through an authored function: a `CanvasBox`, or the Background part of its skin or control, whatever that part's shape. On a Plot chart entity it also fills that chart's data-mark boxes and analytic paths; axes, labels, connectors and selection decoration keep their ordinary colors. The renderer keeps coverage, antialiasing, corner cuts and accents, border, glow, clip and opacity; the function supplies only the fill colour. The [architecture](../../../../../../docs/architecture/rendering.md#gui-shapes-and-retained-presentation) owns the design; this guide owns the interface.

`source` and `variant` select an immutable [ShaderDefinition](../../../../../ipp-core/src/services/asset_management/formats/shader.rs) with a paint body. `properties` are the paint's named inputs, independent of the definition exactly as [custom-material properties](../materials/CUSTOM_MATERIALS.md) are: write them with `DynamicProperty.set`, animate them with ordinary clips and save them with the World. A paint has no time input; it moves only when a clip or a client changes its properties, like a spinner's arc angle.

## Definition

A paint definition has a `paint` body for the `glsl-es-300` backend, shared by WebGL 2 and GLES 3, or a portable [`ipp` body](../materials/CUSTOM_MATERIALS.md#portable-ipp-target) the builder lowers to it, and float scalar or vector parameters (`f32`, `vec2`, `vec3`, `vec4`). Definitions may also declare `Texture2D` parameters for the unified sampling contract; the canvas program does not bind paint textures yet, so a texture-carrying paint fails to load with that reason until that binding lands. It has no vertex or fragment body or required streams. The optional `presentation` recipe flag supplies the presenting view; `lighting` additionally supplies selected scene lights and requires `presentation`. Other recipe flags belong to mesh materials. Encode it with `encodeShaderDefinition` and upload it through the ordinary owned asset API; every source edit is a new asset identity.

The body is the statements of one function. The engine wraps it as

```glsl
vec4 paint(vec2 position, vec2 size, vec4 color, float edge, vec4 data)
```

under a name of its own and returns its result as the fill's straight linear RGBA.

| Input | Meaning |
| --- | --- |
| `position` | The fragment in the shape's own units, from its own top-left corner: before visual scale, so a scaled panel keeps its pattern's proportions, and mirrored with a mirrored box |
| `size` | The shape's own size in those units: the box's size, its laid-out size, or a path's control-point enclosure |
| `color` | The straight linear RGBA the shape would fill with: the `CanvasStyle` tint times white for a plain box, or the skin row's colour (a gradient's start colour) for a skinned part, after transitions; a Plot mark supplies its evaluated data color and inherited tint, without opacity |
| `data` | The resolved per-part `paint_data` vector, sampled through native skin transitions; zero when absent |
| `edge` | Signed contour distance in the shape's own units, negative inside; boxes are exact under uniform scale, analytic paths use the bounded approximation described below |
| `p_<name>` | Each declared parameter, read from the instance's properties |

The engine clamps the result to the unit range and multiplies its alpha by the entity's opacity. A gradient, colour field or checker named for the same part is not drawn, since the paint replaces the fill. Derivative functions such as `fwidth` are available for antialiased patterns; loops are allowed and their cost is the author's.

A body may not hold preprocessor directives, braces that close its function or leave it open, `discard`, or names beginning with `ipp`, `IPP_`, `a_`, `v_` or `u_`. It may call GLSL built-ins and the opt-in presentation functions below; authored helper functions and globals are not part of the interface.

## Presentation inputs

A paint with `recipe: { presentation: true }` receives `viewDirection`, a normalized direction from the fragment toward the viewer in the presenting Surface frame (+X right, +Y down, +Z front). This is the Surface tangent frame, independent of a box's own rotation or reflection; authors can transform it into their local material frame. Perspective views use the actual fragment position; orthographic views share their direction. Root canvases use `[0, 0, 1]`. Rear views retain negative front-normal components so the material can choose its two-sided finish.

Adding `lighting: true` exposes `sceneLightCount`, `sceneLightDirection(i)` and `sceneLightRadiance(i)`. Indices run from zero to the bounded selected count. Directions use the same Surface frame; radiance is linear RGB with point/spot distance and cone attenuation. Lights come from the containing camera domain using the renderer's stable influence selection. No shadows or ambient/environment texture are implied. Out-of-range indices and recipes without lighting return frontal direction and zero radiance. Root canvases have no scene lights.

The renderer maps flat, cylindrical and spherical Surfaces, separated layers and nested Canvas placement while rasterizing as well as drawing directly. Additional non-affine Surface providers need a corresponding presentation mapping implementation. Relevant context changes invalidate containing images, including optionally throttled caches; they do not rewrite component values or local geometry. See the [owning architecture](../../../../../../docs/architecture/rendering.md#presentation-aware-paints) for cache policy and app ownership. This opt-in adds no reflection target or scene-color sampling.

## Example

```ts
const scanlines = encodeShaderDefinition({
  parameters: { spacing: "f32", strength: "f32" },
  backends: {
    "glsl-es-300": {
      paint: `
        float line = fract(position.y / p_spacing);
        float band = smoothstep(0.0, 0.5, line) * smoothstep(1.0, 0.5, line);
        return vec4(color.rgb * mix(1.0, band, p_strength), color.a);`,
    },
  },
});
```

```tsx
<Entity id="panel">
  <Box width={240} height={120} />
  <Paint source={assetRef("scanlines")} spacing={4} strength={0.6} />
</Entity>
```

Generated clients insert a `CanvasPaint` component and set `spacing` and `strength` as dynamic properties of its component identity.

## Plot data marks

A chart entity's sibling `CanvasPaint` applies only to data marks. Plot keeps its existing data preparation and source-row hit geometry; Canvas resolves the paint's current component lifetime while walking the chart. Adding, removing or replacing a paint changes material selection, and property edits update shared parameter blocks without rebuilding bindings or generated mark geometry. Unavailable paints retain each mark's ordinary color.

Analytic paths use the same admitted paint slots and parameter blocks as boxes, with a separate variant of the existing analytic path program. Position is relative to the path enclosure's top-left corner, size is that enclosure's extent, and data is zero. The original winding coverage, antialiasing and clipping remain unchanged. Paint alpha is multiplied by inherited opacity exactly once. Text continues through its ordinary program.

Analytic paths retain native coverage, antialiasing and clipping. Their contour-distance implementation lives with the [path shaders](../shaders) rather than becoming a separate geometry or texture pipeline.

## Shared part paints

A theme row with `paint: true` uses the `CanvasPaint` on the theme entity; a `GuiSkin` override row with `paint: true` uses the control entity's paint. A false row disables inherited paint and the entity's Background fallback. An absent property inherits, preserving the ordinary entity Background behavior. Parts share the exact component lifetime and its named properties, while each shape carries its resolved `paint_data` vector separately. The uniform budget counts distinct packed parameter blocks, not controls or parts. Identical parameters for the same admitted shader share GPU storage, including across separate theme paint components; authored values remain independent. Removing or replacing a component invalidates its selection before reuse.

`paint_data` is an ordinary finite Vec4 row property. State and checked rows resolve it, and native skin transitions sample it with the same timing, interruption and reduced-motion rules as colour. Named properties continue to animate through ordinary clips. Paint functions receive `data` independently of the shared `p_*` values; existing function bodies that do not read `data` need no change.

Decorative outsets affect paint coverage, not layout or hit regions. By default, paint coordinates describe the original part. A touching overlay with `join_anchor` shares its anchor’s background contour and paint rectangle after native placement; both parts cover disjoint sections of the same sheet without changing their clipping or input scope. The [layout implementation](../../../../../ipp-core/src/world/systems/gui/layout/joined_surface.rs) owns that mapping.

## Resources and failure

Paints use bounded renderer-owned shader and parameter storage. Equivalent shader definitions share compiled programs while asset lifetimes and authored properties remain independent. Missing, invalid or unavailable paints fall back to the shape’s authored color and report through normal diagnostics. Context recovery rebuilds derived GPU resources from retained assets.

The [paint implementation](paint.rs) owns admission, budgets, packing and diagnostics; [shader validation](../materials/shader_asset.rs) owns the accepted body syntax. These are implementation details, not application configuration.

## Validation

Renderer unit and service tests cover the body checks, generated program, slots, parameter blocks, draw counts and every fallback. The maintained [default-skin scenario](../../../../../../tests/gui/default-skin/README.md) captures painted panels on worker/WASM/WebGL and native GLES.
