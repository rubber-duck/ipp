# Canvas paint interface 1

A [CanvasPaint](../../../../../ipp-core/src/world/systems/canvas/component.rs) fills its entity's own box through an authored function: a `CanvasBox`, or the Background part of its skin or control, whatever that part's shape. The box keeps its coverage, antialiasing, corner cuts and accents, border, glow, clip and opacity; the function supplies only the fill colour. The [architecture](../../../../../../docs/architecture/rendering.md#gui-shapes-and-retained-presentation) owns the design; this guide owns the interface.

`source` and `variant` select an immutable [ShaderDefinition](../../../../../ipp-core/src/services/asset_management/formats/shader.rs) with a paint body. `properties` are the paint's named inputs, independent of the definition exactly as [custom-material properties](../materials/CUSTOM_MATERIALS.md) are: write them with `DynamicProperty.set`, animate them with ordinary clips and save them with the World. A paint has no time input; it moves only when a clip or a client changes its properties, like a spinner's arc angle.

## Definition

A paint definition has a `paint` body for the `glsl-es-300` backend, shared by WebGL 2 and GLES 3, and float scalar or vector parameters (`f32`, `vec2`, `vec3`, `vec4`). It has no vertex or fragment body, recipe flags, required streams or textures. Encode it with `encodeShaderDefinition` and upload it through the ordinary owned asset API; every source edit is a new asset identity.

The body is the statements of one function. The engine wraps it as

```glsl
vec4 paint(vec2 position, vec2 size, vec4 color, float edge)
```

under a name of its own and returns its result as the fill's straight linear RGBA.

| Input | Meaning |
| --- | --- |
| `position` | The fragment in the shape's own units, from its own top-left corner: before visual scale, so a scaled panel keeps its pattern's proportions, and mirrored with a mirrored box |
| `size` | The shape's own size in those units: the box's size or its laid-out size |
| `color` | The straight linear RGBA the box would fill with: the `CanvasStyle` tint times white for a plain box, or the skin row's colour (a gradient's start colour) for a skinned part, after transitions; without the opacity |
| `edge` | Signed distance to the shape's outer contour in the shape's own units, negative inside; exact under uniform scale |
| `p_<name>` | Each declared parameter, read from the instance's properties |

The engine clamps the result to the unit range and multiplies its alpha by the entity's opacity. A gradient, colour field or checker named for the same part is not drawn, since the paint replaces the fill. Derivative functions such as `fwidth` are available for antialiased patterns; loops are allowed and their cost is the author's.

A body may not hold preprocessor directives, braces that close its function or leave it open, `discard`, or names beginning with `ipp`, `IPP_`, `a_`, `v_` or `u_`. It may call built-in functions only: helper functions and globals are not part of the interface.

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

## Limits and failure

The [canvas program](paint.rs), which draws the canvas's shapes while text draws with a program of its own, holds at most `CANVAS_PAINT_SLOTS` (8) paints at once and gives every paint instance of a canvas its own block of parameter vectors, one per parameter, in an array of `CANVAS_PAINT_VECTORS` (128) vectors that the instances of that canvas share. A paint keeps its slot while it is drawn; a new paint takes a free slot or the slot of a paint not drawn in the current frame.

The shader provider compiles each body alone before the asset loads, so a body that does not compile fails its asset with the compiler's message and never reaches the canvas program. An instance draws its box's colour solidly, and `RenderService::canvas_paint_diagnostics()` names its canvas output, entity and reason, when its definition is unset, pending, failed or recovering, is not a paint, finds no slot, finds no room for its parameters, lacks a property of a declared parameter's type, or was rejected by the combined canvas program. Configured diagnostic logging reports each changed reason once. The rest of the canvas keeps drawing.

Context loss releases the canvas program and its slots; recovery revalidates paint assets through the provider and rebuilds the program on next use.

## Validation

Renderer unit and service tests cover the body checks, generated program, slots, parameter blocks, draw counts and every fallback. The maintained [default-skin scenario](../../../../../../tests/gui/default-skin/README.md) captures painted panels on worker/WASM/WebGL and native GLES.
