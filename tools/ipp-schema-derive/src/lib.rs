//! Target-evaluated component access, registry generation and typed System adapters.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod component_registry;
mod identifier;
mod paths;
mod schema_component;
mod schema_row;
mod system_update;

/// Derives exact typed access and target layout export for a named `repr(C)` struct.
/// Fields marked `#[schema(ignore)]` remain local. Supported fields use `SchemaField`;
/// `#[schema(rows)]` fields (`Rows<R>`, at most seven) also own a property-address region.
#[proc_macro_derive(SchemaComponent, attributes(schema))]
pub fn component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match schema_component::expand(input) {
        Ok(v) => v.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

/// Derives a schema row layout and indexed property access for a named struct whose
/// fields are required (`T`) or optional (`Option<T>`) row property values.
/// `#[schema(rotation)]` marks a Vec4 quaternion property, and
/// `#[schema(text = N)]` declares an `Arc<str>` text property's UTF-8 byte bound.
#[proc_macro_derive(SchemaRow, attributes(schema))]
pub fn row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match schema_row::expand(input) {
        Ok(v) => v.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

/// Generates the only component ID map, typed sum, factories, and dispatch.
/// Example: `component_registry! { pub ComponentValue { Scalar = 1, } }`.
#[proc_macro]
pub fn component_registry(input: TokenStream) -> TokenStream {
    component_registry::expand(input)
}

/// Generate dependency metadata and update adapters from typed method parameters.
#[proc_macro_attribute]
pub fn system_update(attributes: TokenStream, input: TokenStream) -> TokenStream {
    system_update::expand(attributes.into(), input.into())
        .unwrap_or_else(|error| error.into_compile_error())
        .into()
}
