# Glass optical study

This study isolates the same thin-sheet model used by the production Glass skin. The face is flat and the rounded perimeter has a polished, rolled cross-section. There are no corner shards, painted outlines or emissive borders. Clear, frosted and smoked presets share one shader; camera/light inputs and material properties are separate from control state.

The optical terms follow [Filament's dielectric microfacet model](https://google.github.io/filament/Filament.md.html) and [PBRT's dielectric reflection/transmission](https://www.pbr-book.org/4ed/Reflection_Models/Dielectric_BSDF): Fresnel reflectance, GGX/Smith direct response and exponential absorption. A finite analytic studio and an attenuated rear-surface reflection approximate reflected surroundings. Ordinary alpha composition supplies straight-through transmission; roughness broadens reflected features and adds a scattering approximation, without blurring coverage or text. This is not a full glass transport simulation: transmitted background features are not distorted or blurred. No scene-color sampling, reflection target, ray tracing, blur pass or renderer clock is used. GPU performance has not been measured.

The browser preview is served at `/target/gui-glass-skin/optical-preview.html` after building `gui-glass-skin-fixtures` and `browser --render`. Its moving light uses a native animation clip, and its controls move the actual camera and light. Reduced-motion preference starts it paused. The component sheet fits the page while its framebuffer follows the displayed canvas size and browser pixel density, including resize, zoom and monitor changes, bounded by the presentation surface limits. Fixed-size reference captures retain their explicit dimensions. The maintained `preview-dpi` browser case checks actual rendered frames and pointer alignment after resizing.

The preview now presents the [generated component reference](../fixtures/natural-glass-concept/components.png) as actual rendered controls. Buttons, switches, checkboxes, radio and segmented groups, fields, sliders, a dropdown and tabs use native interaction and shared client-authored part paints. The comparison button row pins example optical states; the progress, chips and tooltip are visual specimens. Clear, frosted and smoked surfaces share the dielectric shader, with size-independent rounded edges and a broader reflection model for small glass lenses. The original study remains a comparison fixture; the production view uses public Glass GUI components and the promoted shared optical model.

The component study has its own persistent session and maintained native/worker scenarios. The original three-finish context probes remain available for checking the renderer boundary independently of this layout.

The open dropdown's trigger and menu sample a shared glass contour, keeping the polished sides continuous across their join. The native controls and overlay retain separate interaction regions; closing the menu restores the trigger's standalone rounded contour.

Connected choices belong to their parent sheet: tabs, segmented controls and menu rows add translucent state tints rather than separate glass tiles. The parent owns the reflections and polished perimeter; internal divisions are subtle flat rules, and keyboard focus is an inset underline. Region paints clip to the parent contour and preserve its edge highlights while native state rows and motion continue to drive interaction feedback.

```sh
node tools/shared-host/shared-host.mjs session start tests/gui/glass-skin/optical-material/component-lab.tsx --name glass-components
node tools/shared-host/shared-host.mjs session capture glass-components
node tools/shared-host/shared-host.mjs session capture glass-components right
node tools/shared-host/shared-host.mjs session capture glass-components angle
```

`context-scene.tsx` uses the renderer's actual presenting camera, Surface and scene light. Its explicit opt-in chooses the first bounded scene light as the study's key light. The underlying API exposes the full selected set; applications decide how to use it. The maintained native GLES and worker WebGL scenario checks rendered directions, light falloff, rear views, layer placement, root defaults and image refresh under changed camera/light context.

```sh
node tools/shared-host/shared-host.mjs host list
node tools/shared-host/shared-host.mjs session start tests/gui/glass-skin/optical-material/context-lab.ts --name glass-context
node tools/shared-host/shared-host.mjs session capture glass-context
node tools/shared-host/shared-host.mjs session capture glass-context angle right
node tools/shared-host/shared-host.mjs session capture glass-context angle cylinder
```

`scene.tsx` instead lays out explicit frontal/oblique/grazing view vectors for side-by-side material comparisons. Those cells do not claim a real camera moved. `lab.tsx` can play a six-second native property clip on the authored light parameter. Source edits reload in the persistent session:

```sh
node tools/shared-host/shared-host.mjs session start tests/gui/glass-skin/optical-material/lab.tsx --name glass-optics
node tools/shared-host/shared-host.mjs session capture glass-optics
node tools/shared-host/shared-host.mjs session capture glass-optics right
node tools/shared-host/shared-host.mjs session capture glass-optics motion
```

The accepted [rendering architecture](../../../../docs/architecture/rendering.md#presentation-aware-paints) owns presentation context; the [paint interface](../../../../crates/ipp-render-gl/src/services/render/canvas/CANVAS_PAINTS.md#presentation-inputs) documents shader inputs. Use `optical-preview.html?skin=production` to inspect the actual production skin with the same view, light and Surface controls.

`optical-preview.html?skin=liquid` is a separate soft liquid-glass prototype, using native controls and a smoky body with a rounded meniscus. The narrow optical edge responds to the presenting view and light; hover lifts the body without an outside halo. Its fixed procedural studio background demonstrates straight-through transmission, not scene blur or refraction. Production defaults are unchanged. `component-lab.tsx` accepts `liquid` at session startup; the maintained `liquid-optics-native` and `liquid-optics-worker` cases capture idle, moving-light, oblique and hover states and exercise native input.
