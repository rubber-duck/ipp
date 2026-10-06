//! Direct bindings into stable component storage: typed cells, retained typed
//! queries, compiled property access and the fixed numeric property ranges they
//! validate against.

pub(in crate::world) mod component_binding;
pub(in crate::world) mod component_query;
pub(in crate::world) mod numeric_properties;
pub(in crate::world) mod property_binding;
