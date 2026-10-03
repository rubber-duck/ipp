# World persistence

Persistence captures the serializable authored attachment graph without acquiring or embedding assets. [Host save/load](../../host/persistence.rs) takes one exclusive applied-state cut between whole batches. It restores all Worlds and entities privately, remaps references and restores System contributions before publishing any of the graph. Errors preserve existing Worlds.

Saved state includes durable identities, metadata, configured capacity hints, typed stored entity links, components and selected Systems' persistent contributions. Graph-local World identities distinguish independent copies sharing durable metadata. Typed World/output references remap to fresh World-qualified runtime handles, never fabricated native handles. Animation restores controller descriptions, playback position and each controller's applied contributions, rebuilding bindings when resources are available; fields hold the saved values, contributions included. Internal/evaluated buffers are excluded; references to excluded state reject export. Authored nested output selections survive; Host root presentation does not.

Bounded metadata inspection exposes node identities and names before load name replacements are chosen. Inspection reserves nothing; load rechecks all names and rejects collisions or conflicting replacements. Successful load returns the root and the complete node-to-created-World lifetime map. Callers journal this scope before fallible delivery and perform explicit exact-lifetime cleanup when their ownership policy requires it. Dropping the result does nothing, and destroying the root does not destroy its independently living descendants. [Graph types](graph.rs) own this boundary; System save/load hooks remain World-local.

Asset source strings, types and variants remain unchanged. Missing resources do not prevent save/load; ordinary acquisition later reports readiness or failure. External animation payloads, including entity-valued keys, are not rewritten. Bundling, relocation, merging and cross-schema migration remain unsupported.

## Compatibility and publication

Files require the exact compiled target contract, including its registered components and field layouts. Native and WASM files are not interchangeable merely because their container version matches. The checksum detects corruption, not authenticity. [Container encoding](container.rs) owns format details; [World capture/restoration](../../world/serialization.rs) coordinates System validation.

Capture, encoding and restoration are synchronous whole-candidate operations and can pause other Worlds. Transfer backpressure does not make those operations incremental. [Host connection transfer policy](../../../../ipp-host-session/src) separately controls admission, retained buffers, cancellation and expiry. Later edits cannot change a completed capture; cancelled or detached originating sessions discard unpublished transfers. [I/O writers](../io/README.md) own destination publication guarantees.

`python tools/ipp.py test snapshots` exercises native WebSocket and browser worker persistence with external resources and completed WebGL frame comparisons. The [protocol architecture](../../../../../docs/architecture/protocol-and-schema.md#snapshots-and-world-replacement) owns the durable contract.
