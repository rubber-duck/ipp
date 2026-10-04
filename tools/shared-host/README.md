# Native gallery sessions

The shared gallery uses the same [scene definitions](../../examples/world-gallery/scene-registry.ts) as the browser. This adapter keeps one scene mounted in a Node session against the existing native GLES Host. The Host advances animation and simulation; the session authors options, invokes scene actions, and captures completed output.

The Host owner prepares and starts it once:

```sh
node tools/shared-host/shared-host.mjs host start --gallery --egl-dir /lib64
```

`--gallery` builds the matching native Host, application assets and target-correct saved Platformer World. It authorizes read-only access to the generated `target/` assets and the Platformer's immutable source namespace. Other sessions join that Host using `--host CHECKOUT` or `IPP_SHARED_HOST`; the Host owner restarts it after Rust or generated-contract changes.

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

Select `shapes`, `lighting`, `particles`, `platformer`, `gui`, `charts` at startup. `--width` and `--height` set the viewport, `960 × 640` by default. `--options '{...}'` supplies initial high-level options. Inspect reports the scene's current options, available actions and runtime state; action arguments are optional JSON. Captures write a PNG named for the selected scene. Running particles and animation remain active: capture waits for authoring readiness and an actual completed frame without waiting for stationary pixels.

The `charts` scene places its inward-facing flat and volumetric plots around the camera in a spaced ring, with flat plots presented on Canvas child Worlds. Use `session action NAME focus '"bars"'` to fly to a chart over two seconds, `focus '"center"'` to return to the center, `focus '"overview"'` for an elevated view of the whole ring, and `session action NAME playback '{"playing":false,"time":0}'` to inspect a fixed data phase. The Host owns both camera and data animation clocks. Chart actions `hover` and `select` accept normalized root coordinates and report source row identity through inspect.

`session repl NAME` accepts `options JSON`, `action NAME [JSON]`, `inspect`, `capture` and `reload`. `quit` leaves the session running. Start with `--watch` to rebuild and reload when the selected scene's source bundle changes. Explicit reload recreates runtime handles and preserves high-level options; a compilation or module-validation failure leaves the current scene mounted. A failed remount reports its error, cleans the attempted scene, and can be retried with `reload` after correcting the source. Stopping waits for explicit scene cleanup before acknowledging completion.

The adapter reuses the shared Host's matching generated client and [presentation lock](../../docs/development/shared-host.md#concurrent-presentation-and-capture). Scene operations serialize inside the session, while other clients continue evaluating independently. Prepared local resources use session-owned immutable client sources, released with the mount. Loaded World graphs and additional scene Worlds are explicitly destroyed during cleanup. The native adapter and the browser share authoring; browser navigation and physical input remain in the browser shell.

For a Host configuration outside the gallery convenience command, repeat `host start --io-read PREFIX DIRECTORY` for each required read-only namespace. Asset reads use the Host checkout's generated assets by default; `gallery ... --asset-root DIRECTORY` selects a different local build product directory. The saved Platformer needs the configured `https://platformer.ipp.invalid/` namespace and a World generated for the running native contract.

The maintained [native gallery harness](../../tests/render/native-gallery.test.ts) launches its own real GLES Host, exercises session options/actions/reload and failed compilation, captures all scenes, and checks cleanup after repeated remounts. Run it through `python tools/ipp.py test native-gallery`. Shared development screenshots support iteration; maintained scenario reports provide integration evidence.
