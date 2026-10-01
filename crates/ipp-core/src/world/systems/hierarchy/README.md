# Object hierarchy

Hierarchy composes local transforms through the [core ordered entity links](../../entity_links.rs), not a relationship component. `PlaceEntity` resolves placement once; animation drivers of structural tracks place their entity when they select a key and leave it where they placed it when they stop; a placement whose parent or sibling is gone fails like an ordinary `PlaceEntity` and leaves the entity where it is. Reparenting preserves local TRS; an absent Transform contributes identity for grouping. `ParentJoint` selects the parent's evaluated Skeleton joint frame. Missing joint data leaves that subtree unevaluable rather than falling back to the parent origin.

Parent deletion preserves surviving children as roots without changing their local transforms. Cycles and unusable relationships remain observable and suppress affected evaluation until corrected. Generational references never retarget a reused entity slot. The [runtime architecture](../../../../../../docs/architecture/runtime.md#object-hierarchy) owns these lifetime rules.

Initial propagation precedes [terminal LookAt](../look_at/README.md); final propagation carries its result to descendants before downstream consumers. Full affine composition preserves nonuniform scale and shear. Rendering, cameras, geometry and skinning consume the same final placement; evaluated results do not overwrite authored transforms.

The [compiled propagation path](propagation.rs) retains parent order and [typed transform bindings](transform_binding.rs) until lifecycle changes invalidate them. Lazy, stable entity-local spatial cells outlive synchronous invalidation and are released before entity slots are reused. Consumers borrow cached affine placement within their read phase.

The link store keeps one stored value per entity and one derived ordered-child index. Fixed-width order labels are spaced on append. Exhausted gaps and durable-identity ties trigger local relabeling with an expanding numeric window, not growing keys or routine whole-list sorting. Callers retain binding tokens, not order-label snapshots, across edits.

Source entrypoints: [component](component.rs), [graph maintenance](system_state.rs), [attachment-frame evaluation](update.rs), and [initial/final passes](system.rs). `python tools/ipp.py test hierarchy` covers real generated clients, lifecycle, persistence, queries and independent WebGL comparisons; [core tests](../../../../tests/hierarchy.rs) cover graph and numerical invariants.
