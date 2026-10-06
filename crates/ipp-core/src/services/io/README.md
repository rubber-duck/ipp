# Generic byte I/O

`IoService` provides owned readers and writers independently of Worlds, asset types, executors and transports. Hosts supply external I/O; sources interpret identifiers and grant write access. [IoReadOptions](mod.rs) separates an optional caller byte bound from the requirement that recovery reproduce immutable content.

Registration uses disjoint literal prefixes and forwards the complete identifier unchanged. Removing a registration cancels its active I/O and aborts unpublished writers. Replacement registrations receive fresh identities so stale completions cannot affect them. Stream backpressure bounds individual chunks, not total content or the number of source users.

[Memory sources](memory.rs) support immutable registration and optional atomic publication. Writable memory sources reject immutable recovery requests.

[Finite upload assembly](upload.rs) owns bytes up to an exact caller-validated length, accepts ordered chunks and returns bytes only on consuming completion. Dropping staging cancels it. Consumers own transfer identities, admission, error policy and semantic publication.

The native [filesystem source](../../../../ipp-server/src/services/io/file_system.rs) requires a Host-authorized root. The Host must control filesystem namespace changes during open/publication. Mutable paths cannot promise immutable recovery; use immutable owned bytes when that guarantee is needed. Filesystem writers stage a sibling temporary file and publish only after successful completion.

Source entrypoints: [registration and routing](service.rs), [input and backpressure](reader.rs), and [output progress/publication](writer.rs). Dropping an unfinished write job aborts publication. The [asset architecture](../../../../../docs/architecture/assets.md#source-references-and-resource-providers) owns the cross-service contract; [data source tests](../../../tests/io.rs) exercise these adapters.
