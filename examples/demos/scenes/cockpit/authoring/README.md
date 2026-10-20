# Cockpit authoring

[cockpit-projected.blend](cockpit-projected.blend) is the active scene. The fixed-size console uses a complete [standalone cutout](console-standalone.png) derived from the approved [reference](cockpit-reference.png), with rounded endcaps and a finished apron. The bridge owns the console, integrated instrument mounts and carousel mount; changing viewport aspect changes camera framing, never their dimensions or relative placement.

[project_cockpit.py](project_cockpit.py) builds the scene. [spatial_environment.py](spatial_environment.py) projects the generated transparent-window panorama into six matching cubemap faces and adds the oblate planet, atmosphere shell and distant sky. The runtime supplies planetary illumination, independently drifting clouds and slow rotation through ordinary custom materials and Host animation. [standalone_console.py](standalone_console.py) places the console and registers its independently animated lamps. Native GUI surfaces occupy the authored empties; Blender preview outlines never export.

Generated sources and their exact built-in imagegen edit prompts are kept beside the scene: [standalone console](console-standalone.png) ([prompt](console-standalone-prompt.txt)) and [bridge panorama](bridge-panorama.png) ([prompt](bridge-panorama-prompt.txt)). The six cube images are deterministic reprojections. Earlier generated planet studies, projected plates and the Boolean model in [cockpit.blend](cockpit.blend) remain visual provenance.

Earth uses NASA imagery: Blue Marble Next Generation by Reto Stöckli (July 2004 surface), the Blue Marble cloud composite (2002 release), and Black Marble VIIRS night lights (2016). [Source URLs, credits and checksums](earth/sources.json) accompany the prepared maps; usage follows [NASA's media guidelines](https://www.nasa.gov/nasa-brand-center/images-and-media/). [cockpit_earth.py](../../../../../tools/assets/cockpit_earth.py) downloads originals into the ignored build cache and prepares 8K/4K surface and cloud maps plus 4K/2K night lights. [Prepared checksums](earth/prepared.json) record the outputs. Normal builds use these local derivatives without downloading from NASA. The demo uses the lighter 4K surface/cloud pair and 2K night lights; set `EARTH_QUALITY` in the scene builder to `8k` before rebuilding for a higher-detail export. Cloud coverage occupies the surface texture's unused alpha channel while retaining independent animation coordinates.

The distant sky uses [Hipparcos positions and photometry](stars/source.json) (ESA 1997, distributed by CDS), prepared by [cockpit_stars.py](../../../../../tools/assets/cockpit_stars.py). [starfield.py](starfield.py) builds one mesh of tiny steady points, with restrained exposure and colour variation. The catalogue is stored locally; there is no sky texture download or animated twinkle.

From the repository root, using the pinned Blender installation:

```sh
# Only when regenerating the Earth derivatives:
blender -b --factory-startup --python-exit-code 1 --python tools/assets/cockpit_earth.py
blender -b --factory-startup --python-exit-code 1 --python examples/demos/scenes/cockpit/authoring/project_cockpit.py
python tools/ipp.py build demos-cockpit-native-assets demos-cockpit-assets
```

The builder also runs through Blender MCP with `runpy.run_path(...)["build"]()`. Preserve unrelated unsaved scenes first. Regenerate after changing sources, geometry or mounts. [export_projected.py](export_projected.py) uses the standard scene exporter and adds the RGBA inputs consumed by the app-owned window cutout shader. Runtime metadata accompanies both target-correct asset products. The authoring scripts and runtime types own the detailed dimensions and material inputs.
