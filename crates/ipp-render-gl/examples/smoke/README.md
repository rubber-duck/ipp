# Native GLES scenarios

The Linux/macOS [egl_smoke runner](../egl_smoke.rs) supplies a real context, Host-owned Worlds, renderer, immutable fixtures and completed framebuffer capture. [Context setup](egl.rs) is separate from scenario operations and assertions. These runs prove core/GLES behavior; the maintained native/browser suites provide generated-client and transport coverage.

## Run from the repository root

On Linux, provide a directory containing `libEGL.so` / `libGLESv2.so` or their `.so.1` / `.so.2` counterparts. The runner supports an advertised ANGLE platform with SwiftShader/Vulkan or Mesa surfaceless EGL. On macOS, provide `libEGL.dylib` and `libGLESv2.dylib` from ANGLE with its Metal backend. Libraries and their dependencies must already be installed; the runner does not download them.

### macOS libraries

Build standalone libraries using the [upstream ANGLE setup](https://chromium.googlesource.com/angle/angle/+/main/doc/DevSetup.md), with the Metal backend enabled. The loader selects the advertised [ANGLE Metal platform](https://chromium.googlesource.com/angle/angle/+/main/extensions/EGL_ANGLE_platform_angle_metal.txt); it does not require Vulkan, a display server or a native window. Recent Chrome for Testing macOS bundles link ANGLE internally and do not supply these standalone libraries.

An Apple Silicon alternative verified locally is the third-party [EdgeFirst ANGLE package](https://github.com/EdgeFirstAI/angle-package/releases/tag/v2.1.28252). Preserve its original signed frameworks, then copy the macOS binaries into a flat directory, following the publisher's [flat-library guidance](https://github.com/EdgeFirstAI/hal/blob/main/scripts/fetch-angle.sh). ANGLE locates GLES beside its own binary, so symlinks into separate frameworks do not work. Flattened copies need their own local ad-hoc signatures because the original signatures seal framework metadata:

```sh
ipp_angle="$HOME/.local/share/ipp/angle-v2.1.28252"
mkdir -p "$ipp_angle/lib"
curl -fL https://github.com/EdgeFirstAI/angle-package/releases/download/v2.1.28252/angle-xcframeworks-v2.1.28252.zip -o "$ipp_angle/angle.zip"
echo "59e4b1f68956c92441cde4dca0e9eb1a835bbccd107cefdd1d3d3d60e27410be  $ipp_angle/angle.zip" | shasum -a 256 -c -
ditto -x -k "$ipp_angle/angle.zip" "$ipp_angle"
codesign --verify --deep --strict "$ipp_angle/dist/EGL.xcframework/macos-arm64/libEGL.framework"
codesign --verify --deep --strict "$ipp_angle/dist/GLESv2.xcframework/macos-arm64/libGLESv2.framework"
cp "$ipp_angle/dist/EGL.xcframework/macos-arm64/libEGL.framework/libEGL" "$ipp_angle/lib/libEGL.dylib"
cp "$ipp_angle/dist/GLESv2.xcframework/macos-arm64/libGLESv2.framework/libGLESv2" "$ipp_angle/lib/libGLESv2.dylib"
codesign --force --sign - "$ipp_angle/lib/libGLESv2.dylib"
codesign --force --sign - "$ipp_angle/lib/libEGL.dylib"
export IPP_EGL_LIBRARY_DIR="$ipp_angle/lib"
python tools/ipp.py test shared-host
```

The shared Host and standalone EGL runners selected by the `gles` regression group use the same loader on Linux and macOS. Run the maintained scenarios to verify actual backend behavior; successfully loading EGL alone does not establish rendering coverage.

### Linux libraries

Export ordinary fixtures, then run against a system Mesa installation (replace the library directory for your system):

```sh
cargo run -q -p ipp-core --example export_builtin --locked -- \
  mesh 'ipp://mesh/cube?width=2&height=2&length=2' > /tmp/cube.mesh
cargo run -q -p ipp-core --example export_builtin --locked -- \
  texture 'ipp://texture/checkerboard?width=256&height=256&cellsX=8&cellsY=8' > /tmp/checker.texture
LIBGL_ALWAYS_SOFTWARE=1 \
  cargo run -p ipp-render-gl --example egl_smoke --locked -- \
  /usr/lib/x86_64-linux-gnu /tmp/cube.mesh \
  target/integration-artifacts/render/native /tmp/checker.texture
```

The texture argument is optional. For a supplied ANGLE distribution, use its library directory and omit the Mesa software-selection variable. An extracted Mesa installation may additionally need `LIBGL_DRIVERS_PATH` for its DRI directory, `__EGL_VENDOR_LIBRARY_FILENAMES` for its vendor JSON and `LD_LIBRARY_PATH` for its libraries. Record actual environment/package identities with run evidence.

For shapes and spotlight shadows, build the maintained corpus and supply its directory as the final argument:

```sh
node tools/products/shapes.mjs
LIBGL_ALWAYS_SOFTWARE=1 \
  cargo run -p ipp-render-gl --example egl_smoke --locked -- \
  /usr/lib/x86_64-linux-gnu target/shapes-build/cube.mesh \
  target/integration-artifacts/render/native-shapes \
  target/shapes-build/checker.texture target/shapes-build
```

`--lighting-only` before the positional arguments selects focused lighting scenarios. The [runner](../egl_smoke.rs) owns the complete CLI and scenario selection.

The regression selections `check:gles-spatial` and `check:gles-custom-materials` build the shared gallery cube through `build:render-fixtures`, without compiling browser adapters. The fixture builder imports the maintained gallery asset generator rather than duplicating its mesh data. Publication composition captures use `check:gles-publications` with the same EGL driver. Run these selections through `python tools/ipp.py regression --only <selection>`; their direct-core/GLES scope does not establish transport coverage.

`check:gles-publications` also runs the [ordinary layout scene](gui_publications.rs): parent and nested Canvas Worlds combine `GuiLayout`, boxes, converted-font text and a Surface attachment. Completed captures and headless geometry check padding, clipping, visual transforms and text changes; retained storage and work counters check warm reuse.

The [ordinary control scene](gui_control_publications.rs) joins real `GuiCanvasPublication` observations to the same completed Canvas hits and paint. Independent geometry and pixel assertions cover checked/unchecked and tall checkboxes, an explicit unchecked theme indicator, slider rail/thumb placement, disabled/button paint and converted-font labels. Field writes change checkbox, slider and text values; unchanged glyphs/parts remain retained, boolean/scalar edits avoid reflow, and warm captures remain identical with no uploads. These scenes prove producer/renderer integration, not routed input, semantic action admission or platform adapters. Focus coverage is unfocused paint only until ordinary focus admission is available.

## Evidence and scope

Scenarios cover ready/pending resources, transforms, textures, lighting, private geometry visualization and recovery with independent image assertions. [Deferred removals](deferred_removal.rs) verify that later Systems can still read queued targets, invalidation sees old storage, the next presentation contains no stale mesh, and a peer World keeps drawing shared resources.

Artifacts include PPM captures, sample/operation records and graphics environment identity in the chosen directory. The runner owns and releases its context and Worlds. Resource reconstruction within the native context and actual browser context-loss/restoration are distinct evidence; neither a failed run nor software GL timing establishes hardware performance.
