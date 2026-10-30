# Native gallery sessions

The native adapter mounts the same [demo application](../../examples/demos/application.tsx) and [scene definitions](../../examples/demos/scene-registry.ts) as the browser. A persistent Canvas shell presents the active scene and its native navigation, display settings and scene controls against the existing GLES Host. The Host advances animation and simulation; the session authors options, invokes scene actions, and captures completed output.

The Host owner prepares and starts it once:

```sh
node tools/shared-host/shared-host.mjs host start --gallery --egl-dir /lib64
```

`--gallery` builds the matching native Host, shared fonts and drawings, and target-correct saved Platformer and Cockpit Worlds. It authorizes read-only access to the generated `target/` assets and both scenes' immutable source namespaces. Other sessions join that Host using `--host CHECKOUT` or `IPP_SHARED_HOST`; the Host owner restarts it after Rust or generated-contract changes.

On macOS, follow the [toolchain setup](../../docs/development/building.md#macos-setup) and select the ANGLE library directory through `IPP_EGL_LIBRARY_DIR` or `--egl-dir` instead of `/lib64`. For shapes, lighting, particles or charts without the Blender-dependent scenes, prepare only their shared assets:

```sh
python tools/ipp.py build gles-host font-assets gallery-assets demos-drawings
node tools/shared-host/shared-host.mjs host list
node tools/shared-host/shared-host.mjs host start --window --no-build --io-read ipp-gallery://assets/ target
node tools/shared-host/shared-host.mjs gallery charts --name demos
node tools/shared-host/shared-host.mjs session capture demos
```

`--window` opens an AppKit window on macOS. Start a gallery session below to fill it with live GPU-rendered content. Mouse buttons, dragging, wheel, keyboard navigation and committed text reach the existing GUI input API. Resizing updates the physical viewport and logical layout using the display scale. Closing the window stops its Host; the session watchdog then cleans up. The visible session holds the presentation lock and does not expire for inactivity. Stop it before presenting another session. Without `--window`, the Host keeps its offscreen behavior.

Linux accepts the option but returns an explicit not-implemented error; its [platform stub and interface](../../crates/ipp-server/examples/window/README.md) are ready for a separate implementation. macOS IME marked-text composition and clipboard shortcuts are not implemented yet.

Start a selected scene, then inspect and capture it:

```sh
node tools/shared-host/shared-host.mjs gallery shapes --name geometry
node tools/shared-host/shared-host.mjs session capture geometry
node tools/shared-host/shared-host.mjs session options geometry '{"shape":"cube"}'
node tools/shared-host/shared-host.mjs session action geometry resetCamera
node tools/shared-host/shared-host.mjs session inspect geometry --json
node tools/shared-host/shared-host.mjs session reload geometry
node tools/shared-host/shared-host.mjs session capture geometry --out target/gallery-review
node tools/shared-host/shared-host.mjs session stop geometry
```

Select `shapes`, `lighting`, `particles`, `platformer`, `charts` or `cockpit` at startup. `--width` and `--height` set the viewport, `960 × 640` by default. `--options '{...}'` supplies initial high-level options. Inspect reports the scene's current options, available actions and runtime state; action arguments are optional JSON. Captures write a PNG named for the selected scene. Running particles and animation remain active: capture waits for authoring readiness and an actual completed frame without waiting for stationary pixels.

The `charts` scene places its inward-facing flat and volumetric plots around the camera in a spaced ring, with flat plots presented on Canvas child Worlds. Use `session action NAME focus '"bars"'` to fly to a chart over two seconds, `focus '"center"'` to return to the center, and `focus '"overview"'` for an elevated view of the whole ring. The Host owns camera focus timing. Fixed charts show their source values; `changeSamples true` demonstrates a source edit. Use `dataSource '"streaming"'` for incoming synthetic samples, `streamPlayback false` to pause ingestion, and `dataWindow '"time"'` to select elapsed-time history; `dataSource '"buffer"'` returns to fixed samples. Chart actions `hover` and `select` accept normalized root coordinates and report source row identity through inspect.

`session repl NAME` accepts `options JSON`, `action NAME [JSON]`, `inspect`, `capture` and `reload`. `quit` leaves the session running. Start with `--watch` to rebuild and reload when the selected scene's source bundle changes. Explicit reload recreates runtime handles and preserves high-level options; a compilation or module-validation failure leaves the current scene mounted. A failed remount reports its error, cleans the attempted scene, and can be retried with `reload` after correcting the source. Stopping waits for explicit scene cleanup before acknowledging completion.

The adapter reuses the shared Host's matching generated client and [presentation lock](../../docs/development/shared-host.md#concurrent-presentation-and-capture). Scene operations serialize inside the session, while other clients continue evaluating independently. Prepared local resources use session-owned immutable client sources, released with the mount. Loaded World graphs and additional scene Worlds are explicitly destroyed during cleanup. Scene controls extend the shared overlay through each definition's React slot. Both adapters route physical input through the same application; ordinary scenes support camera navigation, while Cockpit preserves its authored camera pose and fits the reference composition to the viewport. Browser code supplies presentation, connection and input delivery.

For a Host configuration outside the gallery convenience command, repeat `host start --io-read PREFIX DIRECTORY` for each required read-only namespace. Asset reads use the Host checkout's generated assets by default; `gallery ... --asset-root DIRECTORY` selects a different local build product directory. Saved Platformer and Cockpit assets need their configured `https://platformer.ipp.invalid/` and `https://cockpit.ipp.invalid/` namespaces and Worlds generated for the running native contract.

The maintained [native gallery harness](../../tests/gallery/native-gallery.test.ts) exercises session options, actions, reload and cleanup against its own real GLES Host. The [cockpit scenario](../../tests/gallery/scenarios/cockpit.ts) adds projected control input, mask animation and completed-frame assertions at wide and narrow viewports. Run these through `python tools/ipp.py test native-gallery gallery-cockpit`. Shared development screenshots support iteration; maintained scenario reports provide integration evidence.

The [native window scenario](../../tests/runtime/native-window.test.mjs) uses actual AppKit events, image assertions, resize, reload and window-close shutdown. Run `python tools/ipp.py regression --only test:native-window:macos` on an interactive macOS desktop with ANGLE configured. Other platforms skip this scenario; that skip is not window-support evidence.

### Native CEF pages

On Apple Silicon, add `--cef` when starting the Host to enable the cockpit's Browser pane:

```sh
brew install cmake ninja
node tools/shared-host/shared-host.mjs host start --gallery --window --cef
node tools/shared-host/shared-host.mjs gallery cockpit --name demos
node tools/shared-host/shared-host.mjs session action demos carousel 3
```

The engine shares the verified CEF installation and keeps its shim/helpers in the Host's private snapshot. Local page registration and input use the same exclusive development channel as the window; page pixels travel directly through the GPU into ordinary CanvasHtml. The default page root is the cockpit's bundled pages. See the [native engine guide](../../crates/ipp-server/cef/README.md) for the supported platform and tests.
