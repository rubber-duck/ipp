//! Target-evaluated component access, registry generation and typed System adapters.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod component_derive;
mod identifier;
mod registry_codegen;
mod system;

/// Derives exact typed access and target layout export for a named `repr(C)` struct.
/// Fields marked `#[schema(ignore)]` remain local. Supported fields use `SchemaField`.
#[proc_macro_derive(SchemaComponent, attributes(schema))]
pub fn component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match component_derive::derive_component(input) {
        Ok(v) => v.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

/// Generates the only component ID map, typed sum, factories, and dispatch.
/// Example: `component_registry! { pub ComponentValue { Scalar = 1, } }`.
#[proc_macro]
pub fn component_registry(input: TokenStream) -> TokenStream {
    registry_codegen::expand(input)
}

/// Generate dependency metadata and update adapters from typed method parameters.
#[proc_macro_attribute]
pub fn system_update(attributes: TokenStream, input: TokenStream) -> TokenStream {
    system::expand(attributes.into(), input.into())
        .unwrap_or_else(|error| error.into_compile_error())
        .into()
}

#[cfg(test)]
mod tests {
    use super::identifier::snake_case;
    use quote::format_ident;

    #[test]
    fn storage_names_preserve_word_and_acronym_boundaries() {
        assert_eq!(snake_case(&format_ident!("UnlitTexture")), "unlit_texture");
        assert_eq!(snake_case(&format_ident!("HTTPServer")), "http_server");
    }
}
