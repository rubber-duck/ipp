use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// Property count bound; one presence mask byte covers eight properties.
const MAX_ROW_PROPERTIES: usize = 256;

pub(super) fn derive_row(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    for attr in &input.attrs {
        if attr.path().is_ident("schema") {
            return Err(syn::Error::new_spanned(
                attr,
                "schema rows accept no struct-level schema attributes",
            ));
        }
    }

    let name = input.ident;
    if name.to_string().starts_with("r#") {
        return Err(syn::Error::new_spanned(
            name,
            "raw schema identifiers are unsupported",
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "schema rows cannot be generic",
        ));
    }

    let Data::Struct(data) = input.data else {
        return Err(syn::Error::new_spanned(name, "expected a struct"));
    };
    let Fields::Named(fields) = data.fields else {
        return Err(syn::Error::new_spanned(name, "expected named fields"));
    };

    let mut properties = Vec::new();
    for field in fields.named {
        let mut rotation = false;
        for attr in &field.attrs {
            if attr.path().is_ident("schema") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rotation") {
                        rotation = true;
                        Ok(())
                    } else {
                        Err(meta.error("only schema(rotation) is supported on a row property"))
                    }
                })?;
            }
        }

        let ident = field.ident.expect("named field");
        if ident.to_string().starts_with("r#") {
            return Err(syn::Error::new_spanned(
                ident,
                "raw schema identifiers are unsupported",
            ));
        }

        properties.push((ident, field.ty, rotation));
    }

    if properties.is_empty() || properties.len() > MAX_ROW_PROPERTIES {
        return Err(syn::Error::new_spanned(
            name,
            "schema rows require between 1 and 256 properties",
        ));
    }

    let path = quote!(::ipp_core::components::rows);
    let schema = quote!(::ipp_core::components::schema);

    let layout = properties.iter().map(|(ident, ty, rotation)| {
        let hint = if *rotation {
            quote!(#path::RowPropertyHint::Rotation)
        } else {
            quote!(#path::RowPropertyHint::None)
        };
        quote! {
            #path::RowProperty {
                name: stringify!(#ident),
                kind: <#ty as #path::RowPropertyField>::KIND,
                optional: <#ty as #path::RowPropertyField>::OPTIONAL,
                hint: #hint,
            }
        }
    });

    let rotation_checks =
        properties
            .iter()
            .filter(|(_, _, rotation)| *rotation)
            .map(|(_, ty, _)| {
                quote! {
                    const _: () = assert!(
                        matches!(
                            <#ty as #path::RowPropertyField>::KIND,
                            ::ipp_core::components::DynamicPropertyKind::Vec4
                        ),
                        "schema(rotation) requires a Vec4 row property",
                    );
                }
            });

    let indices: Vec<_> = (0..properties.len() as u32).collect();

    let getters = properties
        .iter()
        .zip(&indices)
        .map(|((ident, ty, _), index)| {
            quote! { #index => Ok(<#ty as #path::RowPropertyField>::get(&self.#ident)), }
        });

    let setters = properties
        .iter()
        .zip(&indices)
        .map(|((ident, ty, _), index)| {
            quote! { #index => <#ty as #path::RowPropertyField>::set(&mut self.#ident, value), }
        });

    let clears = properties
        .iter()
        .zip(&indices)
        .map(|((ident, ty, _), index)| {
            quote! { #index => <#ty as #path::RowPropertyField>::clear(&mut self.#ident), }
        });

    let assets = properties.iter().map(|(ident, ty, _)| {
        quote! {
            if let Some(asset) = <#ty as #path::RowPropertyField>::asset(&self.#ident) {
                visit(asset);
            }
        }
    });

    let retained = properties.iter().map(|(ident, ty, _)| {
        quote! {
            if let Some(asset) = <#ty as #path::RowPropertyField>::asset(&self.#ident) {
                total += asset.uri.capacity();
            }
        }
    });

    Ok(quote! {
        #(#rotation_checks)*

        impl #path::SchemaRow for #name {
            const LAYOUT: #path::RowsLayout = #path::RowsLayout {
                properties: &[#(#layout),*],
            };

            fn property(
                &self,
                index: u32,
            ) -> Result<Option<::ipp_core::components::DynamicValue>, #schema::FieldError> {
                match index {
                    #(#getters)*
                    _ => Err(#schema::FieldError::UnknownField),
                }
            }

            fn set_property(
                &mut self,
                index: u32,
                value: ::ipp_core::components::DynamicValue,
            ) -> Result<(), #schema::FieldError> {
                match index {
                    #(#setters)*
                    _ => Err(#schema::FieldError::UnknownField),
                }
            }

            fn clear_property(&mut self, index: u32) -> Result<(), #schema::FieldError> {
                match index {
                    #(#clears)*
                    _ => Err(#schema::FieldError::UnknownField),
                }
            }

            fn retained_bytes(&self) -> usize {
                let mut total = 0usize;
                #(#retained)*
                total
            }

            fn visit_assets(
                &self,
                visit: &mut dyn FnMut(&::ipp_core::services::asset_management::AssetSource),
            ) {
                #(#assets)*
            }
        }
    })
}
