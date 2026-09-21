# Custom material interface 1

[CustomMaterial](../../../../ipp-core/src/world/systems/render/custom_material.rs) selects an immutable [ShaderDefinition](../../../../ipp-core/src/services/asset_management/shader.rs). Its fixed fields use the same generated target-specific access as other components. `properties` is an independent named store: shader changes never create, delete or reset properties. Use `DynamicProperty.set`, `DynamicProperty.remove`, inspection `properties`, and `DynamicProperty.override` in generated clients. React exposes `<CustomMaterial tint={[1, 0, 0, 1]} />` with ordinary Bound/Owned/Auto semantics. Unknown props infer dynamic types; explicit helpers such as `mat2(1, 0, 0, 1)` and `i32(3)` disambiguate types. See the [React parameter API](../../../../../packages/ipp-react/README.md#custom-material-parameters). Owned and Auto fallback declarations may establish their own property types; Bound declarations require existing matching properties.

Property identities survive value edits and unrelated additions; removal, retyping or component replacement invalidates the affected bindings. Asset-valued properties retain general typed source/variant references, including hidden producer values and step animation. Shader `Texture2D` requirements accept only the texture payload type; unrelated extra properties remain valid component state. Both stages access generated `p_<name>` expressions or texture samplers. [Parameter packing](custom_shader.rs) owns GPU layout independently of CPU storage; parameter values do not create program variants.

## GLSL ES 3.00

The `glsl-es-300` backend serves WebGL 2 and GLES 3. Other backend entries round-trip as opaque source without claiming renderer support. Encode definitions with `encodeShaderDefinition` and upload through the ordinary owned asset API. Every source edit requires a new asset identity.

```ts
const definition = {
  recipe: { normals: true, lighting: false, shadowPass: false },
  parameters: { tint: "vec4" },
  backends: {
    "glsl-es-300": {
      fragment: "vec4 materialFragment() { return p_tint; }",
    },
  },
};
```

Backend entries contain complete top-level source bodies, including helper functions and custom varyings. Omit `#version` and `main`; the framework supplies them. A vertex body implements `void materialVertex()` and writes `gl_Position` and all varyings its fragment body uses. An absent vertex body calls `ippDefaultVertex()`. A fragment body implements `vec4 materialFragment()` returning linear RGBA, and may `discard`. An absent or blank fragment body makes the candidate unavailable.

| Interface | Meaning / default |
| --- | --- |
| `a_position` | Required mesh position, location 0 |
| `a_color`, `v_color` | Linear RGB at location 1; absent stream uses white |
| `a_uv`, `v_uv` | UV at location 2; absent stream uses zero |
| `a_weight`, `v_weight` | Texture weight at location 3; absent stream uses one |
| `a_normal`, `v_normal` | Normal at location 4; absent stream uses zero; the lighting helper derives the face normal |
| `v_position` | World position produced by the default vertex helper |
| `u_mvp` | Current pass model/view/projection transform |
| `u_model`, `u_normal` | Model and inverse-transpose normal transforms |
| `IPP_SHADER_INTERFACE_VERSION` | `1` |
| `IPP_PASS_SHADOW` | `0` for surface, `1` for shadow |
| `IPP_RECEIVES_LIGHT` | Whether standard lighting inputs are enabled |

`requiredAttributes` bits 0–3 require authored color, UV, normal and texture-weight streams respectively. Required missing streams select fallback. Attribute locations 5–8 and the pose/skinning helpers follow the existing optional framework. `ippDefaultVertex()` applies pose interpolation and skinning when present. Custom authors can call it before adjusting output, or use the composed helpers directly. Keep custom deformation consistent across passes; `u_mvp` changes with the pass. Do not declare engine names (`a_`, `v_`, `u_`, `p_`, `ipp`, or `IPP_` prefixes). Custom varyings use other names and must match between stages.

With `receives_light`, fragment code gains `u_camera` (xyz position for perspective; view direction when w is one for orthographic), `u_ambient` (linear RGB), `u_light_count`, `u_lights`, `ippSurfaceNormal()`, `ippLightDirection(index, worldPosition)` and `ippLightRadiance(index, worldPosition)`. Code chooses its own lighting calculation. `ippShadowVisibility(lightIndex, nDotLight)` returns visibility for that light's own shadow tile when reception is enabled, otherwise one. Use it inside the same indexed light loop as `ippLightRadiance`. The one-argument overload selects the first spotlight with an assigned shadow tile. [Lighting inputs](shaders/custom-lighting.glsl) and [shadow sampling](shaders/shadow-sampling.glsl) own the raw uniform layout. No lighting is automatically applied to the returned color.

Alpha modes are 0 opaque, 1 cutout and 2 straight-alpha blend. The wrapper discards cutout fragments below `alpha_cutoff` (default 0.5), forces opaque/cutout coverage to one, and clamps output to RGBA8 range. Blended draws follow opaque/cutout draws, ordered far-to-near by camera-space depth with stable identity ties; depth testing stays enabled and depth writes are disabled. Main shading and blending operate in linear space with an `SRGB8_ALPHA8` color attachment. Hardware encodes RGB for storage and decodes destination colors for blending and texture sampling, preserving dark gradients at 8-bit precision. The fullscreen presentation pass receives decoded linear colors and performs the display sRGB conversion. Alpha stays linear.

Lighting, shadow reception and shadow casting default off. Blended materials do not cast shadows. Shadow programs reuse both authored bodies and alpha/discard behavior. Any unusable requested custom program makes the entire candidate unavailable. Custom vertex stages disable authored-bound culling unless `conservative_bounds` explicitly asserts coverage of the deformation; headless picking still uses authored picking geometry.

## Availability and recovery

Select custom, PBR, then unlit from the same entity, with solid unlit red as the final custom-material fallback. Missing resources, incompatible property types/attributes, device limits and compilation/link errors preserve authored state and use fallback. `RenderService::custom_material_diagnostics()` identifies affected entities; configured diagnostic logging reports reason changes. Compilation failure is retained by the immutable shader resource. Parameter writes reuse loaded programs; new definitions and ordinary unload/context recovery trigger a new loading attempt. Surface and shadow participation always use the same selected candidate.

## Assets and validation

The [shader asset codec](../../../../ipp-core/src/services/asset_management/shader.rs) owns recipes, backend bodies and parameter requirements. Use the matching generated encoder instead of constructing binary tags or component offsets. [Dynamic values](../../../../ipp-core/src/components/dynamic_properties/mod.rs) participate in ordinary animation, overlays and persistence; World snapshots retain underlying properties without fetching shader definitions.

Run `python tools/ipp.py test custom-materials` for generated worker/WASM/WebGL evidence. The native `egl_custom_materials` example accepts an EGL library directory, cube mesh fixture and artifact directory; enable `shadows,mesh-poses,skeletal-animation` for the expanded device. Scenarios retain captures and environment records under `target/integration-artifacts/custom-materials`. Core property/animation/overlay/persistence tests and renderer packing tests supplement the real frame scenarios.

## Provider readiness and explicit recipes

Shader source and recipe parameters are retained recovery inputs. The renderer registers a loader with a narrow device handle; that loader compiles and links the requested programs before publishing `Loaded`. Failure releases partial programs and reports `Failed`. Loaded assets retain linked handles, declared parameter types and compact usage metadata; generated GLSL and decoded source copies are discarded. Context replacement invalidates graphics handles while preserving usable decoded metadata and rebuilds them through the same loader. Failed recovery retains that CPU data.

The [shader provider](shader_asset.rs) owns readiness/recovery; [material preparation](custom_material.rs) validates use against the selected recipe. `ShaderRecipe` owns compilation flags. Material lighting/deformation must match the selected recipe; values and alpha settings remain instance state. Built-in programs also use ordinary resource providers, with frame preparation declaring demand and draw submission consuming ready programs.

## Instanced particle meshes

A particle mesh requires `recipe.instancing: true` in the JavaScript encoder. Ordinary draws require it to be false. The compiled default vertex helper automatically consumes the instance transform for a fragment-only custom material. A supplied `materialVertex` implements the instanced interface itself; ordinary custom vertices are not adapted.

The [composed GL interface](shader.rs) defines `IPP_INSTANCED=1`, `a_instance_model` (mat4, attribute locations 9–12), `a_instance_data` (vec4, location 13), `ippInstanceModel()` and `ippInstanceNormal()`. `u_mvp` is view-projection, and `u_model`/`u_normal` are identity for particle batches. The default helper composes the instance model and its inverse-transpose normal matrix. Shared `p_*` properties remain per effect. Extra instance-data lanes are reserved; custom code should rely on the documented transform helpers. The same interface applies in surface and shadow passes. The `particles` build requires at least 14 vertex attributes, within the WebGL 2/GLES 3 baseline.
