# Native Metal rendering

The Metal RenderService consumes completed World publications through the [rendering contract](../../docs/architecture/rendering.md). It submits Camera domains and Canvas painter order using native Metal resources. Nested Camera passes complete before their consumers sample those targets. Canvas uses direct paint where possible, analytic intersections for coincident built-in curved Surfaces, and owned images for cached presentation and separated curved layers. Both renderers use the same Plot view placement and authored shader definitions.

Metal batches consecutive Canvas primitives in painter order. Compatible curved draws share the Surface mapping and can read distinct paint parameters within one shader batch. For eligible upright cylinders, a compute pass solves the intersection per screen column. The renderer reuses the resulting GPU buffer for matching views and placements, with clip coverage, original curves, and authored paint evaluated per primitive. For eligible closed contours, Metal prepares a small grid of signed winding values and candidate edges when uploading the path. Each antialiasing sample tests only the remaining candidates in its cell.

The offscreen macOS Host supplies the clock, output selection, WebSocket ingress and resource I/O. It can import CEF accelerated paint for configured local HTML pages. Metal converts the callback-borrowed BGRA IOSurface into a private linear texture on the GPU, and the Host publishes only the exact completed request.

```sh
python tools/ipp.py build metal-host
target/metal-host/ipp-metal-host
IPP_EGL_LIBRARY_DIR=/path/to/angle/lib python tools/ipp.py regression --suite metal-camera
python tools/ipp.py regression --suite metal-cef
```

The paired native scenario captures Camera and nested Canvas content through Metal and hardware GLES. The CEF scenario checks accelerated pixels and resource recovery across two device contexts. The instrumentation Host records CPU draw spans and Metal command-buffer elapsed samples. The [performance guide](../../tests/performance/README.md#native-metal-and-gles) gives the paired release and diagnostic commands.
