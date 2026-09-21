# Blender viewer

This viewer applies detached Blender exports through the target-generated client owned by `IppCanvas`. The addon supplies authoring data and immutable assets; the worker Host owns runtime evaluation and rendering. The same adapter also supports disk imports.

## Run

From the repository root:

```sh
python tools/ipp.py dev blender-viewer
```

This builds the matching runtime and viewer and serves `http://127.0.0.1:5178/`. Configure that viewer URL in the [addon](../../integrations/blender/README.md), then use Open Viewer to complete local HTTPS onboarding. The redirect carries endpoint and token in the URL fragment, keeping credentials out of the viewer server's request URL.

Use `python tools/ipp.py dev blender-viewer --build` for a build without serving. To host the result elsewhere, serve the complete `target/blender-viewer` directory. Certificate preparation, browser trust and isolated-worktree test settings belong in the [Blender development guide](../../docs/development/blender.md).

## Authoring boundary

[types.ts](../../integrations/blender/client/types.ts) defines the detached scene and animation descriptions; [adapter.ts](../../integrations/blender/client/adapter.ts) translates them through the receiving contract. Full desired snapshots arrive over HTTPS/WSS, while content-addressed assets are fetched from the authenticated addon origin. Animation JSON is encoded by the receiving SDK, so Python never constructs target-specific runtime commands.

Stable producer identities are separate from runtime handles and editable object names. Ordinary edits preserve acknowledged entities and components; renames update symbolic names. Hierarchy remains explicit, including bone attachments. Meshes, poses, skins and other resources use immutable references. Conversion choices and unsupported-data diagnostics belong to the [exporter guide](../../integrations/blender/ipp_blender/EXPORTER.md).

Revisions apply serially. A semantic failure retains acknowledged partial effects and leaves the authoring connection open for a corrected full revision. The adapter advances its applied revision only after reconciliation succeeds; it neither rolls back nor automatically retries. Pending revisions are bounded for backpressure, and runtime framing/admission limits still apply. Reconnect or export-session replacement requires fresh runtime identities.

Scene acknowledgement, resource readiness, playback and completed frames are distinct observations. The live viewer controls active exported actions; the reusable clip catalog is available independently for application-selected playback. Supported export categories do not imply equivalence to Blender's renderer.

## Disk export and import

Run the same exporter and adapter without a live addon connection:

```sh
blender -b SCENE.blend --python-exit-code 1 \
  --python integrations/blender/export_scene.py -- EXPORT_DIR
python tools/ipp.py build browser:render-expanded
python tools/ipp.py import-blender EXPORT_DIR OUTPUT_DIR --namespace NAME
```

Use `ipp-browser-env` before the importer where Chromium requires the repository's environment wrapper. The [import command](../../tools/import_blender_scene.mjs) uses [disk-import.ts](../../integrations/blender/client/disk-import.ts) to publish immutable assets before ordinary Host World serialization.

The output includes content-addressed assets, `world.ipp` and `manifest.json` with camera, ambient fill and reusable clips. `--world FILE.ipp` selects the filename; `--clips-only` leaves playback controller creation to the receiving application. The saved World and encoded clips must match the receiving target contract.

Runtime references use `https://NAME.ipp.invalid/`; configure the browser Host's `resourceUrls` mapping to the deployed asset directory before loading. World saving itself preserves references and does not bundle or relocate data. The [hierarchy gallery](../world-gallery/README.md#rebuilding-the-saved-scene) demonstrates this publication path.

## Validation

`python tools/ipp.py test blender` exercises real Blender, trusted local TLS, the adapter, generated client, worker and completed WebGL frames. It uses generated fixtures and the checked-in [Fox fixture](../../tests/fixtures/blender/README.md), with state and image assertions for conversion, edits, lifecycle and recovery. Tests do not bypass certificate errors. The [suite registry](../../tools/pipeline/suites.json) owns exact cases; failure artifacts are retained under `target/integration-artifacts/blender`.
