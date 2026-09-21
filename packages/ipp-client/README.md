# IPP client

`@ipp/client` supplies shared session and transport support for target-generated IPP clients. Applications author state and observe outcomes; the Host owns runtime clocks, evaluation and presentation.

## Build and connect

Use a generated client matching the Host's actual target and capabilities. Native and WASM layouts are not interchangeable. Generated contracts and assembled distributions are build artifacts; maintain shared support here and generation in [ipp-schema-gen](../../tools/ipp-schema-gen/README.md).

Build the matching [headless example](../../examples/headless-client/README.md) from the repository root:

```sh
python tools/ipp.py dev headless-client --build
```

For browser use, serve a complete assembled distribution with its generated module, worker and WASM. The [gallery](../../examples/world-gallery/README.md) provides a working entry point; the [build guide](../../docs/development/building.md) covers prerequisites. Connection factories negotiate compatibility before returning. The [public source entry](src/index.ts) and [client interfaces](src/client.ts) define transport, diagnostic and authoring APIs.

## Sessions and outcomes

`IppClient` convenience connections create a temporary World and own its Host connection. `IppHostClient` separates connection lifetime from World discovery, creation, attachment and destruction. A connection attaches to one World at a time. Ordinary Worlds outlive connections; multiple connections can share the same World.

Entity identities are World-local and can survive detach. Session-owned handles and pending work must not cross attachments; saved-World loads produce fresh runtime handles. Close declaration roots before closing their client. Worker connections own startup and teardown, including failure cleanup. [host-client.ts](src/host-client.ts) implements these connection boundaries.

Batches apply in order without rollback. A semantic failure returns its scope and surviving identities; earlier effects, including partial effects of the failing operation, remain. Keep returned identities for cleanup or correction. Transport failure is not proof that nothing committed. See the [outcome types](src/types.ts) and [mutation contract](../../docs/architecture/runtime.md#mutation-and-evaluation-boundaries).

`batch()` automatically pages large edits through the shared [command writer](src/command-pages.ts), pipelines a bounded window, and resolves with aggregated outcomes after completion. The explicit `beginBatch()`, `batchChunk()` and `endBatch()` operations support producers whose logical batch spans multiple writes. Buffer length never implies completion. The first buffer holds that World’s evaluation and unrelated commands until termination or the Host’s two-second deadline. `onBatchAborted()` reports expiry; applied effects remain available for correction. See the [client methods](src/client.ts) for correlation and per-buffer outcomes.

Acknowledgement, resource readiness and completed rendering are distinct. Clients observe Host progress without advancing time. Inspection pages are independent observations, not an atomic World snapshot; completed images belong to the presentation boundary.

## Host connections and World files

Resources identify immutable content. Changed bytes require a new identity; releasing producer ownership preserves actual consumers. Pending resources do not prevent valid state changes, and ready draws can continue independently. The [asset architecture](../../docs/architecture/assets.md) defines retention and recovery. HTTP providers may advertise pending immutable sources with a source availability channel; the [worker provider](src/resource-worker.ts) waits independently of transfer concurrency and resumes ordinary reads when production completes.

`registerAsset()` delivers a named immutable source through the separate [source data plane](src/asset-sources.ts); `createAsset()` allocates a fresh source name and returns its descriptor. Delivery uses bounded chunks and completes registration independently of World command batches. Observe resource events for decode and graphics readiness. `releaseAsset()` releases producer ownership. Asset payloads never enter the generated World command codec.

Generated Host clients expose save/load through [world-persistence-client.ts](src/world-persistence-client.ts). Files preserve underlying authored state and controller descriptions, excluding owner-scoped UI declarations and presentation selections. Saving preserves resource references without fetching, bundling or relocating bytes; applications publish durable assets separately.

Transfers pipeline a bounded window of ordered chunks, draining submitted requests before cleanup. Loading still publishes and attaches only after complete private validation and restoration.

Loading validates a compatible new World before attachment. Failure preserves existing Worlds and leaves the Host connection usable. Large capture and restore operations are synchronous and can pause the Host.

## Validation

Run `python tools/ipp.py test native`, `python tools/ipp.py test browser` or `python tools/ipp.py test snapshots` for the relevant real Host/client path. `python tools/ipp.py test command-streaming` combines bounded storage and Host isolation checks with the real native and worker scenarios. The [suite registry](../../tools/pipeline/suites.json) defines coverage and prerequisites.
