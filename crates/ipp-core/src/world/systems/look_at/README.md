# Terminal LookAt

LookAt aims an object's local −Z axis at a same-World entity origin or, with `has_point`, at an explicit World-space point, using World +Y to choose roll. A point aim needs no target entity and takes precedence over one declared alongside it. It contributes evaluated local orientation while preserving independent translation/scale and authored inputs. It consumes initial hierarchy placement and is followed by final descendant propagation.

Targets may be grouping objects. Joint-local declarations and writes to Skeleton pose buffers are unsupported. Disabled, null, deleted or coincident targets withdraw the orientation contribution, as does a point coincident with the object. Poles use a deterministic cardinal-axis fallback. Parent-space conversion uses the full inverse affine transform, including nonuniform scale and shear.

A declaration cannot depend on another enabled LookAt output through its target or either relevant ancestry chain; targeting its own descendant is invalid. Invalid dependencies suppress affected placement until corrected. See the [hierarchy guide](../hierarchy/README.md) and [runtime contract](../../../../../../docs/architecture/runtime.md#object-hierarchy).

Source entrypoints: [component](component.rs), [evaluation](system.rs), [dependency diagnosis](system_state.rs), [target reference cleanup](entity_references.rs) and [aim mathematics](math.rs). The maintained `hierarchy` suite covers this pass together with propagation.
