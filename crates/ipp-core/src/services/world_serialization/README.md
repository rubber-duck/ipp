# World persistence

Persistence captures owned underlying World state without acquiring or embedding assets. [Host save/load](../../host/persistence.rs) encodes a detached capture or restores a private candidate, publishing a new World only after validation succeeds. Errors preserve existing Worlds. Host connection transfers attach a fresh session on successful load; see the [client guide](../../../../../packages/ipp-client/README.md).

Saved state includes durable identities, metadata, configured capacity hints, underlying components and selected Systems' persistent contributions. References remap to fresh runtime handles. Animation restores controller descriptions and playback position, rebuilding bindings when resources are available. Internal/evaluated buffers, overlays, owned declarations and Auto fallbacks are excluded; references to excluded state reject export. Camera selection and presentation settings are not restored.

Asset source strings, types and variants remain unchanged. Missing resources do not prevent save/load; ordinary acquisition later reports readiness or failure. External animation payloads, including entity-valued keys, are not rewritten. Bundling, relocation, merging and cross-schema migration remain unsupported.

## Compatibility and publication

Files require the exact compiled target contract, including selected capabilities and field layouts. Native and WASM files are not interchangeable merely because their container version matches. The checksum detects corruption, not authenticity. [Container encoding](container.rs) owns format details; [World capture/restoration](../../world/serialization.rs) coordinates System validation.

Capture, encoding and restoration are synchronous whole-candidate operations and can pause other Worlds. Transfer backpressure does not make those operations incremental. [Host connection transfer policy](../../../../ipp-host-session/src) separately controls admission, retained buffers, cancellation and expiry. Later edits cannot change a completed capture; cancelled or detached originating sessions discard unpublished transfers. [Data writers](../data_source/README.md) own destination publication guarantees.

`python tools/ipp.py test snapshots` exercises native WebSocket and browser worker persistence with external resources and completed WebGL frame comparisons. The [protocol architecture](../../../../../docs/architecture/protocol-and-schema.md#snapshots-and-world-replacement) owns the durable contract.
