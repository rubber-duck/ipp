use crate::paths;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// One declared row property: `schema(rotation)` marks a Vec4 quaternion and
/// `schema(text = N)` gives a text property its UTF-8 byte bound.
struct RowField {
    ident: syn::Ident,
    ty: syn::Type,
    rotation: bool,
    text: Option<u32>,
}

pub(super) fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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
        let mut text = None;
        for attr in &field.attrs {
            if attr.path().is_ident("schema") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rotation") {
                        rotation = true;
                        Ok(())
                    } else if meta.path.is_ident("text") {
                        let bound: syn::LitInt = meta.value()?.parse()?;
                        text = Some(bound.base10_parse::<u32>()?);
                        Ok(())
                    } else {
                        Err(meta.error(
                            "only schema(rotation) or schema(text = N) is supported on a row property",
                        ))
                    }
                })?;
            }
        }

        if rotation && text.is_some() {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "a row property cannot be both rotation and text",
            ));
        }

        let ident = field.ident.expect("named field");
        if ident.to_string().starts_with("r#") {
            return Err(syn::Error::new_spanned(
                ident,
                "raw schema identifiers are unsupported",
            ));
        }

        properties.push(RowField {
            ident,
            ty: field.ty,
            rotation,
            text,
        });
    }

    if properties.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "schema rows require at least one property",
        ));
    }

    let path = paths::rows();
    let schema = paths::schema();
    let components = paths::components();
    let asset_management = paths::asset_management();

    let kind = quote!(#components::DynamicPropertyKind);

    let layout = properties.iter().map(|property| {
        let RowField {
            ident,
            ty,
            rotation,
            text,
        } = property;
        let hint = if *rotation {
            quote!(#path::RowPropertyHint::Rotation)
        } else {
            quote!(#path::RowPropertyHint::None)
        };
        let max_bytes = text.unwrap_or(0);
        quote! {
            #path::RowProperty {
                name: stringify!(#ident),
                kind: <#ty as #path::RowPropertyField>::KIND,
                optional: <#ty as #path::RowPropertyField>::OPTIONAL,
                hint: #hint,
                max_bytes: #max_bytes,
            }
        }
    });

    let kind_checks = properties.iter().map(|property| {
        let ty = &property.ty;
        let field_kind = quote!(<#ty as #path::RowPropertyField>::KIND);
        if property.rotation {
            quote! {
                const _: () = assert!(
                    matches!(#field_kind, #kind::Vec4),
                    "schema(rotation) requires a Vec4 row property",
                );
            }
        } else if let Some(bound) = property.text {
            quote! {
                const _: () = assert!(
                    matches!(#field_kind, #kind::Text),
                    "schema(text = N) requires an Arc<str> row property",
                );

                const _: () = assert!(
                    #bound >= 1 && #bound <= #path::MAX_ROW_TEXT_BYTES,
                    "schema(text = N) requires a bound between 1 and MAX_ROW_TEXT_BYTES",
                );
            }
        } else {
            quote! {
                const _: () = assert!(
                    !matches!(#field_kind, #kind::Text),
                    "an Arc<str> row property requires schema(text = N)",
                );
            }
        }
    });

    let indices: Vec<_> = (0..properties.len() as u32).collect();

    let getters = properties.iter().zip(&indices).map(|(property, index)| {
        let RowField {
            ident,
            ty,
            ..
        } = property;
        quote! { #index => Ok(<#ty as #path::RowPropertyField>::get(&self.#ident)), }
    });

    let setters = properties.iter().zip(&indices).map(|(property, index)| {
        let RowField {
            ident,
            ty,
            ..
        } = property;
        let check = property.text.map(|bound| {
            quote! { #path::check_row_text(&value, #bound)?; }
        });
        quote! {
            #index => {
                #check
                <#ty as #path::RowPropertyField>::set(&mut self.#ident, value)
            }
        }
    });

    let clears = properties.iter().zip(&indices).map(|(property, index)| {
        let RowField {
            ident,
            ty,
            ..
        } = property;
        quote! { #index => <#ty as #path::RowPropertyField>::clear(&mut self.#ident), }
    });

    let assets = properties.iter().map(|property| {
        let RowField {
            ident,
            ty,
            ..
        } = property;
        quote! {
            if let Some(asset) = <#ty as #path::RowPropertyField>::asset(&self.#ident) {
                visit(asset);
            }
        }
    });

    let retained = properties.iter().map(|property| {
        let RowField {
            ident,
            ty,
            ..
        } = property;
        quote! {
            total += <#ty as #path::RowPropertyField>::heap_bytes(&self.#ident);
        }
    });

    let property_count = properties.len();

    Ok(quote! {
        #(#kind_checks)*

        const _: () = assert!(
            #property_count <= #path::MAX_ROW_PROPERTIES,
            "schema rows declare more than MAX_ROW_PROPERTIES properties"
        );

        impl #path::SchemaRow for #name {
            const LAYOUT: #path::RowsLayout = #path::RowsLayout {
                properties: &[#(#layout),*],
            };

            fn property(
                &self,
                index: u32,
            ) -> Result<Option<#components::DynamicValue>, #schema::FieldError> {
                match index {
                    #(#getters)*
                    _ => Err(#schema::FieldError::UnknownField),
                }
            }

            fn set_property(
                &mut self,
                index: u32,
                value: #components::DynamicValue,
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
                visit: &mut dyn FnMut(&#asset_management::AssetSource),
            ) {
                #(#assets)*
            }
        }
    })
}
