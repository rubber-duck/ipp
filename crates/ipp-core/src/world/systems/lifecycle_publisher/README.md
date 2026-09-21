# Lifecycle publisher

`LifecyclePublisherSystem` publishes owned observations of applied entity, component and asset changes. It owns subscription filtering and per-session queues; it owns neither loading nor synchronous invalidation. Slow clients cannot retain component or resource borrows. Ordinary evaluation does not publish authored component changes.

Observations retain their effect tick and World sequence; filtering can leave sequence gaps. Failed batches preserve observations of applied changes. Subscriptions are session-scoped, do not replay history and are removed on session release. Unsubscribe discards matching queued observations; inspection supplies initial state when needed.

Queues are bounded. Overflow clears the affected session's subscriptions and pending observations and emits one terminal notification. New subscriptions can be established after that notification drains. Transport congestion and encoding failures remain separate concerns.

Source entrypoints: [observation/filter types](mod.rs), [publication and queue policy](system.rs), and [focused tests](lifecycle_publisher_tests.rs). `python tools/ipp.py test lifecycle` exercises generated native WebSocket and browser worker clients with real resources, partial failures and reattachment. The [System lifecycle contract](../../../../../../docs/architecture/runtime.md#lifecycle-and-state-access) owns the cleanup boundary.
