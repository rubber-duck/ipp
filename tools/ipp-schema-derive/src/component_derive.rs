use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident};

pub(super) fn derive_component(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let mut no_create = false;
    for attr in &input.attrs {
        if attr.path().is_ident("schema") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("no_create") {
                    no_create = true;
                    Ok(())
                } else {
                    Err(meta.error("only schema(no_create) is supported on a component"))
                }
            })?;
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
            "schema components cannot be generic",
        ));
    }

    let repr_c = input
        .attrs
        .iter()
        .any(|a| a.path().is_ident("repr") && a.parse_args::<Ident>().is_ok_and(|i| i == "C"));
    if !repr_c {
        return Err(syn::Error::new_spanned(
            name,
            "schema components require exactly #[repr(C)]",
        ));
    }

    let Data::Struct(data) = input.data else {
        return Err(syn::Error::new_spanned(name, "expected a struct"));
    };
    let Fields::Named(fields) = data.fields else {
        return Err(syn::Error::new_spanned(name, "expected named fields"));
    };

    let mut exposed = Vec::new();
    for field in fields.named {
        let mut ignore = false;
        for attr in &field.attrs {
            if attr.path().is_ident("schema") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("ignore") {
                        ignore = true;
                        Ok(())
                    } else {
                        Err(meta.error("only schema(ignore) is supported"))
                    }
                })?;
            }
        }
        if !ignore {
            let ident = field.ident.unwrap();
            if ident.to_string().starts_with("r#") {
                return Err(syn::Error::new_spanned(
                    ident,
                    "raw schema identifiers are unsupported",
                ));
            }
            exposed.push((ident, field.ty));
        }
    }

    let count = exposed.len() as u16;
    let creation = if no_create {
        quote! { None }
    } else {
        quote! { Some(Self::default()) }
    };
    let offsets = exposed
        .iter()
        .map(|(f, _)| quote! { ::core::mem::offset_of!(Self, #f) as u32 });

    let setters = exposed.iter().map(|(f, t)| {
        quote! {
            if offset == ::core::mem::offset_of!(Self, #f) as u32 {
                self.#f = <#t as ::ipp_core::components::schema::SchemaField>::from_value(value)?;
                return Ok(());
            }
        }
    });

    let validation = exposed.iter().map(|(f, t)| {
        quote! {
            if offset == ::core::mem::offset_of!(Self, #f) as u32 {
                return <#t as ::ipp_core::components::schema::SchemaField>::validate_kind(kind);
            }
        }
    });

    let reads = exposed.iter().map(|(f, t)| {
        quote! {
            (
                ::core::mem::offset_of!(Self, #f) as u32,
                <#t as ::ipp_core::components::schema::SchemaField>::to_value(&self.#f),
            )
        }
    });

    let getters = exposed.iter().map(|(f, t)| {
        quote! {
            if offset == ::core::mem::offset_of!(Self, #f) as u32 {
                return Ok(<#t as ::ipp_core::components::schema::SchemaField>::to_value(&self.#f));
            }
        }
    });

    let retained = exposed.iter().map(|(f, t)| {
        quote! {
            total = total.checked_add(
                <#t as ::ipp_core::components::schema::SchemaField>::retained_bytes(&self.#f)?,
            )?;
        }
    });

    let writes = exposed.iter().map(|(f, t)| {
        quote! {
            ::ipp_core::components::schema::write_string(sink, stringify!(#f));
            sink.write(&(::core::mem::offset_of!(Self, #f) as u32).to_le_bytes());
            sink.write(&(::core::mem::size_of::<#t>() as u32).to_le_bytes());
            sink.write(&(::core::mem::align_of::<#t>() as u32).to_le_bytes());
            sink.write(&[<#t as ::ipp_core::components::schema::SchemaField>::KIND as u8]);
            if let Some(defaults) = &defaults {
                <#t as ::ipp_core::components::schema::SchemaField>::write_default(&defaults.#f, sink);
            }
        }
    });

    Ok(quote! {
        impl ::ipp_core::components::schema::SchemaComponent for #name {
            const FIELD_COUNT: usize = #count as usize;

            fn create() -> Option<Self> { #creation }

            fn has_field(offset: u32) -> bool { [#(#offsets),*].contains(&offset) }

            fn fields(&self) -> Vec<(u32, ::ipp_core::components::schema::FieldValue)> {
                vec![#(#reads),*]
            }

            fn field(&self, offset: u32) -> Result<::ipp_core::components::schema::FieldValue, ::ipp_core::components::schema::FieldError> {
                #(#getters)*

                Err(::ipp_core::components::schema::FieldError::UnknownField)
            }

            fn retained_bytes(&self) -> Option<usize> {
                let mut total = 0usize;
                #(#retained)*
                Some(total)
            }

            fn set_field(
                &mut self,
                offset: u32,
                value: ::ipp_core::components::schema::FieldValue,
            ) -> Result<(), ::ipp_core::components::schema::FieldError> {
                #(#setters)*

                Err(::ipp_core::components::schema::FieldError::UnknownField)
            }

            fn validate_field(
                offset: u32,
                kind: ::ipp_core::components::schema::FieldKind,
            ) -> Result<(), ::ipp_core::components::schema::FieldError> {
                #(#validation)*

                Err(::ipp_core::components::schema::FieldError::UnknownField)
            }

            fn write_contract(sink: &mut impl ::ipp_core::components::schema::ContractSink) {
                ::ipp_core::components::schema::write_string(sink, stringify!(#name));
                sink.write(&(::core::mem::size_of::<Self>() as u32).to_le_bytes());
                sink.write(&(::core::mem::align_of::<Self>() as u32).to_le_bytes());
                sink.write(&#count.to_le_bytes());

                let defaults = <Self as ::ipp_core::components::schema::SchemaComponent>::create();
                sink.write(&[u8::from(defaults.is_some())]);
                #(#writes)*
            }
        }
    })
}
