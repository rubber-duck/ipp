# Native Metal rendering

The Metal RenderService consumes completed World publications through the [rendering contract](../../docs/architecture/rendering.md). It submits Camera domains and Canvas painter order using native Metal resources. Nested Camera passes complete before their consumers sample those targets. Canvas uses direct paint where possible, analytic intersections for coincident built-in curved Surfaces, and owned images for cached presentation and separated curved layers. Both renderers use the same Plot view placement and authored shader definitions.

Rust calls the Metal framework through `objc2-metal`. A window host can give `MetalRenderService::set_presentation_layer` its `objc2-quartz-core` `CAMetalLayer`. The renderer presents the root color texture to that layer and keeps the root available for capture. The command-line Host renders offscreen.

Canvas uses four-vertex instanced triangle strips for compatible affine quads and analytic covers. Clipped polygons retain their triangle lists. Transient batches share write-combined Metal buffers, which return to the upload pool only after their command buffer completes. Large retained batches own immutable GPU ranges that they can replay without uploading them again. GPUI walks ordered batches of shadows, quads, path tiles and sprites, then submits ranges of shared instance data. IPP applies that layout to consecutive compatible draws. Its curved Surface path still evaluates clip planes, depth and paint at the intersection. See [GPUI’s batch dispatch](https://github.com/zed-industries/zed/blob/a0e666ae930a85ab38947e2d49222b0ea99f1c2f/crates/gpui_apple/src/metal_renderer.rs#L757-L800) and [instanced quad draw](https://github.com/zed-industries/zed/blob/a0e666ae930a85ab38947e2d49222b0ea99f1c2f/crates/gpui_apple/src/metal_renderer.rs#L1007-L1037).

Metal batches consecutive Canvas primitives in painter order. Compatible curved draws share the Surface mapping and can read distinct paint parameters within one shader batch. For eligible upright cylinders, a compute pass solves the intersection per screen column. The renderer reuses the resulting GPU buffer for matching views and placements, with clip coverage, original curves, and authored paint evaluated per primitive. For eligible closed contours, Metal prepares a small grid of signed winding values and candidate edges when uploading the path. Each antialiasing sample tests only the remaining candidates in its cell.

The offscreen macOS Host supplies the clock, output selection, WebSocket ingress and resource I/O. It can import CEF accelerated paint for configured local HTML pages. Metal converts the callback-borrowed BGRA IOSurface into a private linear texture on the GPU, and the Host publishes only the exact completed request.

```sh
python tools/ipp.py build metal-host
target/metal-host/ipp-metal-host
IPP_EGL_LIBRARY_DIR=/path/to/angle/lib python tools/ipp.py regression --suite metal-camera
python tools/ipp.py regression --suite metal-cef
```

The paired native scenario captures Camera and nested Canvas content through Metal and hardware GLES. The CEF scenario checks accelerated pixels and resource recovery across two device contexts. The instrumentation Host records CPU draw spans and Metal command-buffer elapsed samples. The [performance guide](../../tests/performance/README.md#native-metal-and-gles) gives the paired release and diagnostic commands.
