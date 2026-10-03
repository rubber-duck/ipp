# Data Sources and Derived Views

[Architecture overview](../architecture.md) · [Implementation strategy](../plans/runtime-and-rendering.md#data-sources-and-visualization)

This topic defines the data subsystem boundary; the [build guide](../development/building.md#toolchain-and-scope) records delivered capabilities.

## Service and source ownership

The Host's Data Service owns typed data sources across Worlds: identity, schema, payload, producer admission, lifetime and retention. Schemas define column names and core value types, independently of display dimensions. Sources are not assets and use no AssetRef. Like Asset Management, Data Service builds on the generic [IoService](assets.md#source-references-and-resource-providers): dataset protocol transfers use its chunked delivery, cancellation and backpressure, as client asset sources do over the data plane, and Data Service decodes their typed deltas.

Source names are Host-wide stable strings resolved by Data Service; unlike session-scoped client asset IDs, they let Worlds share sources. IoService routes byte resources and provides generic transfer machinery; live dataset names do not require byte-reader registrations. Data Service owns typed resolution, readiness and validation. Which connection may produce or bind a source awaits the same access scoping as assets.

Streaming data sources are append-only and expire samples through [shared retention](#streaming-windows-and-shared-retention). Buffer data sources permit append or insert, edit and removal and never expire automatically; as with [assets](assets.md#assets-shared-between-worlds), memory bounds them and allocation failure is observable. Both kinds expose one typed read contract. Each source has one registered producer and any number of consumers across Worlds; Data Service is its sole mutation owner, serializing producer updates with expiry.

Source-row identities are per-incarnation sequence numbers assigned in commit order and never reused; a buffer insert also takes the next number, since identity is not position. The single producer sends ordered updates, so it can predict identities without acknowledgement. Identities survive value edits; labels and picking refer to them.

A source lives while its producer or a consumer retains it; explicit destruction is a separate operation. Schema is fixed within an incarnation. Changing schema, or creating a source under a name whose producer is gone, creates a fresh incarnation under that name; clients resupply data this way on reconnect or restore. Consumers read the old incarnation's data, subject to streaming expiry, until it is replaced. Bindings re-resolve by name and revalidate; stale work never retargets the new incarnation. Creating a name another live producer holds is refused.

## Data bindings and evaluation

`StreamingDataSourceBinding` and `BufferDataSourceBinding` are separate components implementing one data-binding contract that consumers read, as renderable presentation components share theirs. Each holds a source name and [dynamic properties](protocol-and-schema.md#generated-field-access) for column authoring; only `StreamingDataSourceBinding` has windows. An entity has at most one data-source binding of either kind, and that binding has zero or one presentation consumer on the entity. It may remain unconsumed for headless use or incomplete authoring. A name that resolves to the other source kind leaves the binding unavailable and reported, like a schema mismatch. Separate representations use separate bindings, sharing sources but not derived computation.

Each named column-binding property references an [immutable column-binding definition](assets.md#column-binding-definitions): a pure row-wise projection, identity being the simplest. An optional companion typed property, associated by naming convention, holds an animatable parameter shared across the binding's rows; expressions read it alongside source-row columns, and protocol writes and animation target that stored property. Definitions resolve inputs separately for each binding.

The World-local Data Binding System evaluates windows and column bindings through the [pure expression evaluator](runtime.md#pure-expression-evaluation), materializing computed columns in reusable buffers when their inputs change; identity bindings may read source storage directly. Prepared views, buffers and evaluators are reconstructible runtime state of the binding, never authored, persisted or part of source schema.

One dirty flag, owned by the binding component's runtime state rather than a System-side map keyed by entity, carries the handoff: Data Binding System sets it when the prepared view changes, and the consumer clears it only after successfully updating its render state. Read-only queries are observations and never clear the flag. The flag is not persisted; new, restored or replaced consumers perform initial preparation. The consumer's own presentation properties invalidate its geometry independently.

Column bindings read only declared inputs from one source row and component properties. Cross-row reads, joins, aggregation and fitting would need separately defined evaluation semantics; clients own such analysis and supply its results as source data.

Data Binding System reads animated and driven properties under the [frame-order contract](runtime.md#frame-order), so bindings over one unchanged source animate independently. Seeking recomputes from current rows and property values, with no accumulated or transition state.

## Streaming windows and shared retention

Each `StreamingDataSourceBinding` selects its own window, independently of viewport and other bindings. Count windows use insertion order. Range windows use a raw numeric or timestamp source column, a width and a forward-moving supplied or data-driven anchor; Host time may supply the anchor, making time windows a special case. Constraints within one binding intersect. A binding with no window retains up to a Host-configurable default size cap in bytes, beyond which the oldest samples expire as with a count window; explicit windows are not limited by that default.

Data Service retains the union of windows requested by bindings across all Worlds and expires a sample only when no binding needs it. Arriving samples go through the same window rules: a sample outside every retained window is ignored, and a late sample never moves a data-driven anchor backwards. A new or widened window exposes retained history and fills with future arrivals; it never requests replay or resurrects expired samples. Windows belong to bindings, not source schema.

Without registered bindings a streaming data source retains no samples, even while its producer is connected, though its identity and schema remain alive. Background history therefore needs a registered binding, even one whose consumer is not rendered. Dataset updates are not ordered against World commands, so clients create the binding and await its World outcome before streaming samples they need retained.

## Dataset protocol and update boundary

A dedicated logical dataset protocol creates sources and sends ordered bulk updates outside World command batches, possibly over the same physical connection; World commands configure bindings. One update carries many deltas, with no reply per delta. Bounded ingestion queues apply producer backpressure and never drop accepted data. Local runtime producers use the same validated service boundary without wire serialization.

Updates follow the [World batch rule](runtime.md#mutation-and-evaluation-boundaries): deltas apply in order without rollback, the update stops at its first invalid delta and reports one outcome, and failure is loud and scoped to the producer's connection. Only validated committed updates become visible. They reach a World at its [mutation boundary](runtime.md#frame-order), like completed assets, so a World sees one source state per tick, and notify dependent bindings there.

## Persistence

Bindings persist under the ordinary [World persistence boundary](protocol-and-schema.md#snapshots-and-world-replacement): source name, windows, column-binding asset references and parameter values, never runtime handles. Source schemas and samples, derived columns and prepared evaluators are not persisted, and dataset-content export is not required. Clients resupply source data on reconnect or restore through fresh incarnations; restored bindings stay observably unavailable until a compatible source and definition assets are available.
