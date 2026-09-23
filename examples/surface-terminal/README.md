# Surface terminal

This example places a terminal-like panel in a 3D World using the optional `surfaces` capability. It includes font outlines, SVG drawings and an RGBA bitmap. It does not connect to a terminal process.

Build the example and run its maintained scenarios with `python tools/ipp.py test surfaces`. The browser fixture supplies a Host and converted immutable assets to [the scene](scene.tsx). The same client contracts work with native Hosts.

Items use metres on the Surface's local XY plane. Text positions are baseline origins and font size is metres per em. Drawing scale converts source SVG units to metres. An owned React Surface assigns stable identities to keyed items; item order controls composition. Use `surfaceProperty(id, "opacity")` or the other exported property names to target individual items through ordinary animation and StateOverlay commands.

A Surface presents directly unless its entity also declares `<SurfaceCache>`, which opts it into [distance-based texture caching](../../docs/architecture/rendering.md#optional-surface-texture-caching). The [cache policy](../../crates/ipp-core/src/world/systems/surface/cache_policy.rs) documents the distance bands, limits and defaults.

Editable SVG sources live in [authoring/svg](authoring/svg) under [CC0](authoring/svg/CC0.txt). Fonts and their notices are maintained in [shared fonts](../../assets/fonts/README.md). The build downloads the pinned fonts automatically, converts the shared runtime font under `target/font-assets`, and writes this example's drawings and bitmap under `target/surface-assets`. [The format reference](../../crates/ipp-core/src/services/asset_management/SURFACE_FORMATS.md) defines the converter subset and approximation limits.

## Workload measurements

The positioned-text workload supports visible row/column counts, typing, blink, scrolling, full replacement and a sliding window of glyphs not shown before. The asset build exports the printable ASCII and unseen glyph sets it draws from. See the [retained rendering measurements](../../tests/performance/retained-gui.md) for the maintained browser command, diagnostics and physical-device procedure.
