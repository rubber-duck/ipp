# Generic data sources

`DataSourceManagementService` provides owned readers and writers independently of Worlds, asset types, executors and transports. Hosts supply external I/O; sources interpret identifiers and grant write access. [DataReadOptions](mod.rs) separates an optional caller byte bound from the requirement that recovery reproduce immutable content.

Registration uses disjoint literal prefixes and forwards the complete identifier unchanged. Removing a registration cancels its active I/O and aborts unpublished writers. Replacement registrations receive fresh identities so stale completions cannot affect them. Stream backpressure bounds individual chunks, not total content or the number of source users.

[Memory sources](memory.rs) support immutable registration and optional atomic publication. Writable memory sources reject immutable recovery requests. The optional [ZIP source](zip.rs) owns an immutable archive and supports stored and DEFLATE entries, validating entry lengths and checksums before exposing bytes. It buffers a complete entry; encrypted, multi-disk, ZIP64 and non-UTF-8-name archives are unsupported. Dependency selection belongs to the [crate manifest](../../../Cargo.toml).

The native [filesystem source](../../../../ipp-server/src/services/data_source.rs) requires a Host-authorized root. The Host must control filesystem namespace changes during open/publication. Mutable paths cannot promise immutable recovery; use immutable owned bytes or ZIP input when that guarantee is needed. Filesystem writers stage a sibling temporary file and publish only after successful completion.

Source entrypoints: [registration and routing](service.rs), [input and backpressure](reader.rs), and [output progress/publication](writer.rs). Dropping an unfinished write job aborts publication. The [asset architecture](../../../../../docs/architecture/assets.md#source-references-and-resource-providers) owns the cross-service contract; [data source tests](../../../tests/data_sources.rs) exercise these adapters.
