# Developing the React Integration

[Core lifetime](../architecture/runtime.md#entity-lifetime) · [Strategy](../plans/react-reconciler.md) · [React package](../../packages/ipp-react/README.md)

The package uses React's mutation reconciler to submit committed declarations through the generated client. React writes ordinary component fields and deletes what it created or adopted; core owns entity/component lifetime and required components. The package guide owns public props, asset/animation declarations and browser composition APIs.

## Using a root

Build a matching target and the package with `python tools/ipp.py build browser:headless` and `python tools/ipp.py build react`. Serve the browser artifacts with the application; adjust the generated-module import to its source location:

```tsx
import { createRoot, Entity, Scalar } from "@ipp/react";
import { IppClient } from "./target/browser-build/headless/generated.js";

const client = await IppClient.connectWorker(
  "/target/browser-build/headless/wasm-worker.js",
  "/target/browser-build/headless/runtime.wasm",
);
const root = createRoot(client, {
  onError: (error) => console.error(error),
});

try {
  await root.render(
    <Entity id="react-owned">
      <Scalar value={4} />
    </Entity>,
  );
} finally {
  try {
    await root.unmount();
  } finally {
    await client.close();
  }
}
```

`render()` settles committed intent after the Host result; `flush()` also waits for scheduled React/hook work. `render(null)` clears a reusable root, while `unmount()` permanently releases it after pending work. Neither advances runtime time. React acknowledgement, asset readiness and completed GPU frames are separate barriers.

## Real integration harness

With the [pinned toolchain](building.md):

```sh
python tools/ipp.py setup node
python tools/ipp.py setup browser --with-deps
python tools/ipp.py test react canvas
```

Development/production fixtures exercise real commits → generated client → MessagePort → worker/WASM. Production fixtures consume the public package and application React peer. Independent client batches test last-write-wins and lifecycle; real response gates test pending attachment/unmount and failures.

| Maintained harness | Focus |
| --- | --- |
| [Reconciliation](../../tests/react/react.browser.test.ts) | Acknowledged identities, concurrent writes, rejection/correction and cleanup |
| [Canvas](../../tests/rendering/canvas/canvas.test.ts) | DOM composition, context/error routing, multiple canvases, resize, startup/StrictMode and runtime replacement |
| [Display density](../../tests/rendering/canvas/dpi.test.ts) | Completed WebGL frames at emulated densities, CSS resize and proportional caps |
| [Custom materials](../../tests/rendering/custom-materials.test.ts) | Shader/asset declarations, parameter edits, readiness, fallback and recovery |
| [Gallery animation](../../tests/gallery/gallery-animation.test.ts) | Playback declarations/controls and completed frames |

Use `python tools/ipp.py test custom-materials animation` for the latter behaviors. [Rendering scenarios](rendering.md) add asset and image evidence. The [testing policy](integration-testing.md) owns synchronization, failure artifacts and cleanup requirements. Remote refs remain outside current support; a replacement canvas session starts a fresh World.

## Builds and artifacts

`python tools/ipp.py build react` builds the client dependency and React package. Public entries and dependency pins live in [package.json](../../packages/ipp-react/package.json); applications supply React and React DOM where used. The source `development` condition and packaged production entries are both exercised by maintained fixtures.

Reports: `packages/ipp-react/dist/build-report.json`, `target/react-build/build-report.json` and `target/browser-build/build-report.json`. Count shared chunks once. These record build sizes and identities, not frame performance.
