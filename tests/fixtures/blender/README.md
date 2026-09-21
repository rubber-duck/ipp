# Blender fixtures

[fox.blend](fox.blend) is a compressed, self-contained fixture with its original packed 1024×1024 texture, a 24-bone rig and the Survey, Walk and Run skeletal actions. No source GLB or external image is required. Survey is active; the exporter also exposes the associated skeletal actions as reusable clips. A separate `FoxBreathing` shape-key action exercises mesh-pose interpolation before skinning, with independent runtime controllers for the two deformations.

The exporter preserves texture dimensions and uses textured PBR for the fixture's Principled material. The test-only unlit comparison isolates geometry and UV fidelity from lighting and normal interpolation differences. [exporter_check.py](../../blender/exporter_check.py) verifies retained actions, packed texture availability and deformation against Blender; [fox_fixture.py](../../blender/fox_fixture.py) owns the added shape animation and comparison commands.

## Provenance and attribution

The [Khronos Fox source](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/9429648735279342b4c32b8745f7904196607379/Models/Fox) is pinned to commit `9429648735279342b4c32b8745f7904196607379`. Its [original GLB](https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/9429648735279342b4c32b8745f7904196607379/Models/Fox/glTF-Binary/Fox.glb) is 162,852 bytes with SHA-256 `d97044e701822bac5a62696459b27d7b375aada5de8574ed4362edbba94771f7`. The original binary is not checked into this repository.

Attribution, preserved from the source's [legal notice](https://github.com/KhronosGroup/glTF-Sample-Assets/blob/9429648735279342b4c32b8745f7904196607379/Models/Fox/README.md#legal):

- Model: PixelMannen, © 2014 Public, [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/legalcode).
- Rigging and animation: tomkranis, © 2014, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/legalcode).
- Conversion to glTF: @AsoboStudio and @scurest, © 2017, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/legalcode).

The upstream [license notice](LICENSE.md) is retained alongside this file with its malformed documentation-license link corrected. The conversion here imports the GLB with Blender 5.2.1, packs its texture, removes the importer's hidden Icosphere bone-display helper and its custom-shape references, scales the root objects uniformly by 0.01, adds a camera and two lights for inspection, assigns stable `ipp_id` custom properties, sets frame 0, and saves with compression. Original geometry is preserved as Basis, alongside the UVs, skin weights, original texture resolution and all three skeletal actions. The added `Chest puff` shape key and `FoxBreathing` action are local test-fixture modifications. These changes do not imply endorsement by the original contributors.

## Use and validation

Open `fox.blend` in Blender to inspect the authored animation, or use the [addon and viewer](../../../integrations/blender/README.md). `python tools/ipp.py test blender` loads this file directly and combines numerical conversion checks with real transport and rendered-frame assertions.

Regenerate the added shape-key animation in the existing fixture from the repository root with:

```sh
blender --background tests/fixtures/blender/fox.blend --python-exit-code 17 --python tests/blender/fox_fixture.py
```

This updates the fixture in place and saves frame 0. The script preserves Basis and refuses unexpected shape-key/action identities.

[fixture.py](../../blender/fixture.py) provides a smaller generated scene for controlled mesh, material, camera, light, skin and animation edits. [hierarchy_pose_fixture.py](../../blender/hierarchy_pose_fixture.py) and [particles_fixture.py](../../blender/particles_fixture.py) provide focused authoring inputs. Their test-only commands drive the same exporter used by interactive authoring.
