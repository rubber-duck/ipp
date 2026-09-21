# React world components

`@ipp/react` turns committed React declarations into sparse IPP state overlays. Applications compose props; the core owns lifetime, fallback behavior and evaluation. Definitions resolve against each receiving generated client without retaining session identities.

Build with `python tools/ipp.py build react` from the repository root. Import scene declarations from `@ipp/react` and browser composition from `@ipp/react/web`. React is a peer dependency; the client and reconciler are package dependencies. Applications supply React DOM. [package.json](package.json) owns versions and exports; [index.ts](src/index.ts) defines the public entry point.

## Declaration roots

Connect a matching generated client:

```tsx
import { createRoot, Entity, Scalar } from "@ipp/react";

const root = createRoot(client, { onError: console.error });
try {
  await root.render(
    <Entity bindTo="producer">
      <Scalar value={3} />
    </Entity>,
  );
} finally {
  await root.unmount();
}
```

This temporarily overrides an existing producer entity. `Entity` requires `id` (create/own) or `bindTo` (bind/preserve). Component `bound` selects Bound (`true`), Owned (`false`) or Auto (omitted/null). Removing a prop withdraws its override. Parenting is explicit through `Children` or `Hierarchy`; nesting alone grants neither parenting nor ownership. [components.ts](src/components.ts) defines props; the [core lifetime contract](../../docs/architecture/runtime.md#component-modes) governs replacement and invalidation.

`render()` acknowledges declarations; `flush()` drains committed and ordinary scheduled hook work. Rapid renders replace superseded descriptions that have not started submission, and their promises settle with the description actually applied in that pending queue position. Explicit commands and cleanup preserve their order. Neither method waits for resource readiness or arbitrary application promises. Unmount roots before closing their client. Failed batches may retain partial effects: corrected declarations can recover, but unchanged rejected work is not retried. Indeterminate transport failure stops submission. Roots do not migrate between sessions, and general remote refs remain unsupported. See [commit handling](src/commits.ts).

## Canvas and nested worlds

`IppCanvas` owns presentation and the worker connection. Each nested `World` owns a declaration scope within that runtime World. DOM and scene declarations have separate React roots: pass context explicitly and use scene-compatible error/Suspense fallbacks. Canvas teardown releases its scopes before closing the client. [web.tsx](src/web.tsx) and [canvas-world-session.ts](src/canvas-world-session.ts) define startup, cancellation and cleanup.

Use `browserRuntime(baseUrl)` from `@ipp/client` with a complete browser distribution. `worldUrl` loads a compatible saved World before declarations bind; `initialize` can restore application presentation settings. Configure resource relocation before startup. Resizing retains the session; changed runtime configuration or World URL creates a fresh one. Acknowledgement, resource readiness and completed capture remain distinct. The [gallery](../../examples/world-gallery/README.md) supplies working browser composition and saved-World examples.

The runtime evicts unused assets on release unless `assetCacheBytes` is supplied at startup. For example, `<IppCanvas runtime={{ ...browserRuntime(baseUrl), assetCacheBytes: 3 * 1024 ** 3 }} />` retains up to 3 GiB of unused resident asset data before eviction. Eviction preserves lifecycle retirement and generational freshness on reacquisition.

## Resources and animation

[Asset declarations](src/assets.ts) have root-local IDs and immutable inputs. Replace data/clip identity when content changes; in-place mutation bypasses encoding caches. Named consumers retain their prior selection until a replacement is ready, including failed or superseded loads. Unmount releases producer ownership while preserving other consumers. Host-local resource references do not make saved Worlds portable.

`AnimationAsset` supplies a reusable clip; `Animation` owns a controller. Playback refs express intent while the Host owns time, including atomic signed-speed playback. An optional transition blends changed numeric, quaternion and pose bindings with an explicit duration, easing and destination-time policy; discrete and structural bindings reject transitions. Ready replacements preserve playback state; cleanup removes controllers before target overlays. [animation.ts](src/animation.ts) defines bindings and handles; the [lighting example](../../examples/world-gallery/worlds/lighting/animation-controller.tsx) demonstrates playback.

## Custom material parameters

`ShaderAsset` owns immutable stages and explicit recipe/parameter requirements. `CustomMaterial` supplies independently editable typed values, so parameter changes do not recompile the shared shader. [Shader declarations](src/shaders.ts), [value helpers](../ipp-client/src/dynamic-properties.ts) and the [material implementation guide](../../crates/ipp-render-gl/src/services/render/CUSTOM_MATERIALS.md) define that interface. External applications must configure their bundler for shader-text imports.

## Validation

Run `python tools/ipp.py test react` for reconciliation through the generated client and worker, or `python tools/ipp.py test canvas` for browser composition and lifecycle. The [suite registry](../../tools/pipeline/suites.json) includes additional material, animation and completed-frame coverage.
