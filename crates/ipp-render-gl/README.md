# Shared GL renderer

`ipp-render-gl` provides shared WebGL 2 and GLES 3 rendering through a compile-time device boundary. Rust consumes final evaluated World state; platform Hosts own contexts, surfaces, clocks and presentation. Rendering never advances simulation. The crate depends only on `ipp-core`; [Cargo features](Cargo.toml) select optional deformation, shadows and particles. The [rendering architecture](../../docs/architecture/rendering.md) owns system boundaries and color conventions.

## Resources and recovery

Install RenderService loaders before creating resources. Host service progression decodes and uploads meshes and compiles requested programs before reporting graphics readiness. Presentation consumes ready resources and declares further demand. Missing required resources skip affected draws; custom materials follow their defined fallback chain.

GPU readiness and CPU availability have separate lifetimes. Graphics failure/recovery preserves usable CPU data and resource identities. Shared release completes the Host's all-World invalidation barrier before reuse. [Graphics loaders](src/services/render/assets.rs) and [RenderService](src/services/render/service/mod.rs) implement these boundaries.

A usable, explicitly selected camera is required for World draws. Missing or invalid camera data clears the target and permits later correction; removal does not preserve camera components. Camera and shadow culling use trustworthy evaluated bounds through the GeometrySystem spatial index; missing or unproven enclosures preserve visibility.

Render preparation reuses evaluated geometry, packs selected lights once per submission and retains scratch buffers for sequential World draws. Material ordering groups opaque draws by shader and values while preserving the existing transparent depth order. Devices cache current GL bindings and exact per-program uniform values within a submission, invalidating them at frame entry, resource deletion and context recovery. These choices affect draw preparation and redundant uploads without merging ordinary draws. The [renderer source](src/services/render/service/) owns the implementation.

## Native host binding and smoke fixture

A RenderService currently supports one graphics context and synchronous submissions across multiple Worlds. Simultaneous independent contexts over one catalog are unsupported. Native embedders supply context lifetime/currentness and loaded entry points; the renderer creates no window or event loop.

The [native smoke guide](examples/smoke/README.md) owns EGL setup, fixture commands and capture instructions. Browser distributions use the maintained [assembler](../../packages/ipp-client/tools/assemble.mjs) with the same capabilities as the WASM artifact; the [browser render host](../../packages/ipp-client/src/render-worker.ts) owns setup and recovery.

## Runtime shader templates

[Shader composition](src/services/render/shader.rs) uses capability-gated templates and immutable recipes. Material values remain uniforms. [Program providers](src/services/render/program_assets.rs) compile through ordinary resource progression/recovery. The [custom material guide](src/services/render/CUSTOM_MATERIALS.md) owns the GLSL authoring interface, fallback, transparency and instancing contracts.

## Direct lighting and spotlight shadows

Standard rendering includes unlit/PBR materials, textures, custom materials and geometry visualization. Direct lights and uniform ambient fill are supported; optional spotlight shadows fall back to unshadowed illumination when capacity is unavailable. Image-based lighting, MSAA, automatic LOD selection and general residency scheduling remain unimplemented.

The maintained `lighting`, `custom-materials`, `geometry` and particle suites retain completed-frame evidence and recovery artifacts. Native smoke proves actual GLES rendering; browser/native integration suites add generated-client and transport coverage. Build success alone is not rendering evidence.
