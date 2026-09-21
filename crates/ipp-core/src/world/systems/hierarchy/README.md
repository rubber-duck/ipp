# Object hierarchy

Hierarchy composes local transforms through non-owning, same-World parent relationships. Reparenting preserves local TRS; an absent Transform contributes identity for grouping. Optional bone parenting attaches to the parent's evaluated joint frame and requires `skeletal-animation`. Missing joint data leaves that subtree unevaluable rather than falling back to the parent origin.

Parent deletion preserves surviving children as roots without changing their local transforms. Cycles and unusable relationships remain observable and suppress affected evaluation until corrected. Generational references never retarget a reused entity slot. The [runtime architecture](../../../../../../docs/architecture/runtime.md#object-hierarchy) owns these lifetime rules.

Initial propagation precedes [terminal LookAt](../look_at/README.md); final propagation carries its result to descendants before downstream consumers. Full affine composition preserves nonuniform scale and shear. Rendering, cameras, geometry and skinning consume the same final placement; evaluated results do not overwrite authored transforms.

The [compiled propagation path](propagation.rs) retains parent order and [typed transform bindings](transform_binding.rs) until lifecycle changes invalidate them. Consumers borrow cached affine placement within their read phase.

Source entrypoints: [component](component.rs), [graph maintenance](system_state.rs), [attachment-frame evaluation](update.rs), and [initial/final passes](system.rs). `python tools/ipp.py test hierarchy` covers real generated clients, lifecycle, persistence, queries and independent WebGL comparisons; [core tests](../../../../tests/hierarchy.rs) cover graph and numerical invariants.
