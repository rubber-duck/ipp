# Glass skin reference and validation

This suite renders real native controls and charts with the [Glass skin](../../../packages/ipp-react/src/gui-glass/README.md) on GLES and worker WebGL. The [visual language](../../../packages/ipp-react/src/gui-glass/DESIGN.md) defines intent; scenarios own observable behavior and image assertions.

The current [production reference](fixtures/natural-glass-production/components.png) comes from the approved optical prototype. Its [prompt](fixtures/natural-glass-production/prompt.txt) and [manifest](fixtures/natural-glass-production/manifest.json) record provenance. Earlier angular concepts are historical evidence, not the current target. References are never runtime textures.

Use the [optical preview](optical-material/README.md) with `?skin=production` to inspect actual controls under moving light and view. The [production scenario](scenarios/production-optics.ts) exercises that sheet through real input and captured frames, including connected dropdowns. [Chart scenarios](../../data/scenarios/plot-glass.ts) cover the corresponding native data and rendering paths.

Run selected cases through `python tools/ipp.py regression`, or use `--suite gui-glass-skin` for the suite. [The suite registry](../../../tools/pipeline/suites.json) owns cases and prerequisites; [the testing policy](../../../docs/development/integration-testing.md) owns delivery evidence. Shared native labs support visual iteration, while maintained scenarios verify input, state, lifecycle and appearance across both Hosts.
