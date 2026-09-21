# Blender authoring integration

The addon serves detached scene descriptions over WSS and immutable assets over HTTPS from Blender's main thread. The shared [client adapter](client/adapter.ts) translates them through the receiving generated SDK; the [browser viewer](../../examples/blender-viewer/README.md) and disk importer use that same authoring code. Blender is authoring tooling; the IPP Host owns simulation time. The installable Python addon remains separate from the TypeScript client and example UI.

## Build and use

The maintained extension targets Blender 5.2 on Linux x64 with its Python 3.13 wheel set. Other platforms need verified dependency locks and extension builds. The [extension manifest](ipp_blender/blender_manifest.toml) owns the Blender version range; the [development guide](../../docs/development/blender.md) owns installation and environment setup.

From the repository root:

```sh
python tools/ipp.py build blender-addon
python tools/ipp.py dev blender-viewer
```

Install the ZIP produced under `target/blender` using Blender's Install from Disk action and enable IPP Scene Sync. The 3D View sidebar's IPP tab provides Start, Sync Scene, Stop and Open Viewer. Configure the viewer URL in addon preferences; its default is `http://127.0.0.1:5178/`. The viewer origin is allowed automatically; additional exact origins can be configured. Restart sync after changing connection settings.

Open Viewer starts sync if needed and opens the addon's local HTTPS page. By default the addon generates and retains local TLS credentials in Blender's user configuration. Accept the browser certificate warning and local-network permission when requested, then follow the redirect to the viewer. Endpoint and token travel in its URL fragment. Once the extension and prebuilt viewer are available, interactive use needs no Node or helper process.

For browser-trusted developer certificates, use `python tools/ipp.py setup certificates` with mkcert installed and trust established in the browser environment. Configure the returned certificate/key paths in preferences or through `IPP_BLENDER_CERTIFICATE` and `IPP_BLENDER_PRIVATE_KEY`. Explicit invalid credentials fail rather than being silently replaced. See [certificate onboarding](../../docs/development/blender.md#local-certificates-and-viewer-onboarding) for trust setup and limitations.

## Development and lifetime

`python tools/ipp.py setup blender` verifies and extracts pinned development dependencies under ignored `target/blender`; packaging bundles the wheels and their licenses. These dependencies remain in the addon distribution. [tools/blender.py](../../tools/blender.py) owns preparation, packaging and the background runner.

```sh
python tools/ipp.py dev blender --args --ready-file target/blender/run/ready.json --port 8118 --viewer-url http://127.0.0.1:5178/ --blend tests/fixtures/blender/fox.blend
```

The readiness file supplies the endpoint and token for the local onboarding page. The background runner drives the main-thread loop explicitly; GUI mode yields through Blender timers. A blocking background MCP loop must likewise pump the server. No Python worker threads are started, and heavy extraction or synchronous file reads can still pause Blender.

The [exporter](ipp_blender/EXPORTER.md) owns conversion and unsupported-data diagnostics. The [detached interface](client/types.ts) separates stable producer identities, editable names and session-local runtime handles. Object renames preserve identity. Export sessions and runtime sessions are fenced independently; old authenticated asset URLs cannot be reused against a replacement addon session.

The [server](ipp_blender/server.py) publishes complete snapshots and immutable content, retaining assets until shutdown. Failed publication preserves the previous snapshot. Transfer chunks and pending updates are bounded for backpressure without imposing scene or retained-asset quotas; allocation/storage failures remain observable. Stop or disable closes connections and removes temporary exports, while retained TLS identity has a separate lifetime. The [authoring architecture](../../docs/architecture/authoring.md#process-ownership) owns these boundaries.

The same extraction supports [disk export and import](../../examples/blender-viewer/README.md#disk-export-and-import) for deployment without a running Blender process.

## Validation

Run `python tools/ipp.py test blender` with Blender on PATH (or `BLENDER_BIN`) and browser-trusted credentials. The maintained suite builds prerequisites and exercises real Blender, HTTPS/WSS, the generated client, worker and completed WebGL frames; server checks cover publication, authentication and cleanup. Use `python tools/ipp.py test particles-blender` for particle conversion. The [suite registry](../../tools/pipeline/suites.json) owns exact coverage.

The checked-in [Fox fixture](../../tests/fixtures/blender/README.md) contains its packed texture and actions, with preserved attribution and license notice. Generated fixtures provide smaller assertions. These tests establish specific supported behavior, not arbitrary Blender scene compatibility or renderer equivalence.
