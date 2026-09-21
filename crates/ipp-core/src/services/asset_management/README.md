# Shared asset ownership

`AssetManagementService` owns Host-wide resource identities, providers and consumer demand. Worlds retain typed source references; providers acquire immutable content through the separately owned [data source service](../data_source/README.md). The [asset architecture](../../../../../docs/architecture/assets.md) defines the ownership and recovery contract.

Copying an asset key does not retain its resource. Unloading preserves identities retained by demand or ownership; an unreferenced idle identity may be discarded to avoid reacquisition. Final release invalidates the generation before reuse. The Host completes synchronous invalidation across all Worlds before releasing shared payloads. Embedders that progress services directly must also complete that Host lifecycle boundary, including dispatch of pending observations.

Decoded CPU availability and graphics readiness are independent. Consumers can use retained CPU data during graphics recovery; failed acquisition preserves authored references and reports failure. Immutable client sources can be prepared before component selection, and releasing producer ownership preserves recovery data still required by other consumers. Completed externally recoverable resources may remain in an unused cache under the Host-configurable soft memory target documented with the [catalog implementation](catalog.rs). That target governs idle retention rather than admission: active demand and producer ownership may exceed it. Resource counts have no IPP quota; memory, representable identities and device capabilities bound growth.

Start with [service and demand](service.rs), [provider loading/recovery](resource.rs) and the [release barrier](lifecycle.rs). Renderer-owned loaders provide GPU representations through the same lifecycle.

The [mesh](MESH_FORMAT.md), [texture](TEXTURE_FORMAT.md) and [skeletal](SKELETAL_FORMATS.md) format references describe the binary payloads beside their decoders.
