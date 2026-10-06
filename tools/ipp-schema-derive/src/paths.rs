//! Paths of the `ipp-core` items that generated code names.
//!
//! The derives and `system_update` emit `::ipp_core::` paths so they expand in any
//! crate; `ipp-core` resolves them in itself through `extern crate self as ipp_core`.
//! `component_registry!` is invoked only inside `ipp-core` and also emits `crate::`
//! paths, which name the registered component types and crate-private items.

use proc_macro2::TokenStream;
use quote::quote;

/// `::ipp_core::systems`: typed update parameters, bindings and dependency metadata.
pub(super) fn systems() -> TokenStream {
    quote!(::ipp_core::systems)
}

/// `::ipp_core::components`: the dynamic property kinds and values.
pub(super) fn components() -> TokenStream {
    quote!(::ipp_core::components)
}

/// `::ipp_core::components::schema`: field access, target contracts and lifecycle.
pub(super) fn schema() -> TokenStream {
    quote!(::ipp_core::components::schema)
}

/// `::ipp_core::components::rows`: schema rows layouts and their property access.
pub(super) fn rows() -> TokenStream {
    quote!(::ipp_core::components::rows)
}

/// `::ipp_core::components::storage`: the paged stable component stores.
pub(super) fn storage() -> TokenStream {
    quote!(::ipp_core::components::storage)
}

/// `::ipp_core::components::dynamic_properties`: dynamic property addressing.
pub(super) fn dynamic_properties() -> TokenStream {
    quote!(::ipp_core::components::dynamic_properties)
}

/// `::ipp_core::services::asset_management`: the `AssetSource` visited in rows.
pub(super) fn asset_management() -> TokenStream {
    quote!(::ipp_core::services::asset_management)
}

/// `crate`: root re-exports of `ipp-core`, such as `ErrorReason` and `DynamicProperties`.
pub(super) fn crate_root() -> TokenStream {
    quote!(crate)
}

/// `crate::components`: the registered component types and the registry inside `ipp-core`.
pub(super) fn crate_components() -> TokenStream {
    quote!(crate::components)
}

/// `crate::services::asset_management`: `AssetSource` and the crate-private
/// `AssetDemandSelection` inside `ipp-core`.
pub(super) fn crate_asset_management() -> TokenStream {
    quote!(crate::services::asset_management)
}
