# StateOverlay system

StateOverlay owns declaration identities, entity bindings, component overrides, Auto fallbacks, precedence and ownership cleanup. It is standard runtime behavior. The [runtime architecture](../../../../../../docs/architecture/runtime.md#component-modes) defines Bound/Owned/Auto semantics and their relationship to entity ownership.

Ordinary component values occupy one stable typed store. Only overridden properties retain hidden producer values; temporary activation values belong to affected components. Releasing an override reveals the next contribution or latest producer value. Animation and constraints own their separate sparse restoration values, and inspection/persistence reconstructs producer inputs through those systems.

Lifecycle work happens at mutation boundaries. Failed ordered work retains its applied effects. Mandatory owner cleanup releases departing ownership even when a surviving declaration cannot activate; unrelated evaluated values remain intact. Outcome aliases preserve cleanup identities for partially applied attachments.

Start with [System participation](system.rs), [sparse component inputs](component_inputs.rs) and [attachment/release lifecycle](lifecycle.rs). Asset resources themselves belong to [AssetManagementService](../../../services/asset_management/README.md).

[Storage tests](storage_tests.rs) and [preparation tests](preparation_tests.rs) cover sparse retention and stable addresses. Maintained lifecycle, animation and React scenarios exercise ownership through real clients and rendering.
