# KayKit platformer authoring provenance

The editable scene is built from the free downloads available from [KayKit - Platformer Pack](https://kaylousberg.itch.io/kaykit-platformer) and [KayKit - Character Animations](https://kaylousberg.itch.io/kaykit-character-animations). Only the pieces used by the scene are packed into `platformer.blend`; the full archives remain outside the repository under `target/asset-downloads/kaykit/`.

| Source archive | Bytes | SHA-256 |
| --- | --: | --- |
| `KayKit_Platformer_Pack_1.0_FREE.zip` | 23,789,539 | `7e140ee01abf99a5896cf93a02ff3dc6a23a7222109fd04854821f44a3a3adeb` |
| `KayKit_Character_Animations_1.1.zip` | 14,858,957 | `65882f31f905ad2e953819648a59287cdeab8f623908d5ef701971d3758be20f` |

The scene uses the medium mannequin and its `Walking_A`, `Running_A`, and `Crawling` actions, renamed to the stable application clip names. The rig has 23 deform/control bones, within IPP's 32-joint export limit. Root-bone translation is held at the source action's first sample because `platformer-root` owns travel around the route.

Run `build_platformer.py` through Blender 5.2 or newer with `--factory-startup`. Pass the output blend, extracted Platformer Pack directory, extracted Character Animations directory, preview image, and output route JSON as the five arguments after `--`. The builder generates both the authoring curve and runtime route from one waypoint list, then ray-casts 136 samples against evaluated course meshes. It rejects a route whose support height differs by more than eight centimetres.

Ordinary gallery builds export the packed scene through the standard Blender exporter and import it through the matching worker/WASM runtime. They write the saved World, clips, meshes, textures and companion metadata to ignored `target/gallery-platformer-assets/`. Neither upstream pack is needed. To generate only that product:

```sh
python tools/ipp.py build gallery-platformer-assets
```

The build uses `--clips-only` because the gallery application owns gait playback; saving Blender's active walk controller would compete with the application's controller. [route.py](route.py) is the shared waypoint and gait source for both scene assembly and generated runtime route metadata.
