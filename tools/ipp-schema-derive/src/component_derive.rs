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
    let mut row_fields = Vec::new();
    for field in fields.named {
        let mut ignore = false;
        let mut row_field = false;
        for attr in &field.attrs {
            if attr.path().is_ident("schema") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("ignore") {
                        ignore = true;
                        Ok(())
                    } else if meta.path.is_ident("rows") {
                        row_field = true;
                        Ok(())
                    } else {
                        Err(meta.error("only schema(ignore) and schema(rows) are supported"))
                    }
                })?;
            }
        }
        if ignore && row_field {
            return Err(syn::Error::new_spanned(
                field.ident,
                "schema(rows) fields cannot be ignored",
            ));
        }
        if !ignore {
            let ident = field.ident.unwrap();
            if ident.to_string().starts_with("r#") {
                return Err(syn::Error::new_spanned(
                    ident,
                    "raw schema identifiers are unsupported",
                ));
            }
            if row_field {
                row_fields.push(exposed.len());
            }
            exposed.push((ident, field.ty));
        }
    }

    // Region k of rows field k starts at 0x1000_0000 * (k + 1), below the dynamic bit.
    let rows: Vec<_> = row_fields.iter().map(|&index| &exposed[index]).collect();
    if rows.len() > 7 {
        return Err(syn::Error::new_spanned(
            &rows[7].0,
            "schema components support at most seven rows fields",
        ));
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

    let rows_path = quote!(::ipp_core::components::rows);
    let row_index = |field: &Ident| rows.iter().position(|(row, _)| row == field);

    let row_has = rows.iter().enumerate().map(|(k, (_, t))| {
        quote! {
            if let Some(relative) = #rows_path::row_region_relative(offset, #k) {
                return <#t as #rows_path::SchemaRowsField>::has_row_field(relative);
            }
        }
    });

    let row_getters = rows.iter().enumerate().map(|(k, (f, t))| {
        quote! {
            if let Some(relative) = #rows_path::row_region_relative(offset, #k) {
                return <#t as #rows_path::SchemaRowsField>::row_field(&self.#f, relative);
            }
        }
    });

    let row_setters = rows.iter().enumerate().map(|(k, (f, t))| {
        quote! {
            if let Some(relative) = #rows_path::row_region_relative(offset, #k) {
                return <#t as #rows_path::SchemaRowsField>::set_row_field(&mut self.#f, relative, value);
            }
        }
    });

    let row_validation = rows.iter().enumerate().map(|(k, (_, t))| {
        quote! {
            if let Some(relative) = #rows_path::row_region_relative(offset, #k) {
                return <#t as #rows_path::SchemaRowsField>::validate_row_field(relative, kind);
            }
        }
    });

    let row_assets = rows.iter().map(|(f, t)| {
        quote! {
            <#t as #rows_path::SchemaRowsField>::visit_assets(&self.#f, visit);
        }
    });
    let visit_row_assets = (!rows.is_empty()).then(|| {
        quote! {
            fn visit_row_assets(
                &self,
                visit: &mut dyn FnMut(&::ipp_core::services::asset_management::AssetSource),
            ) {
                #(#row_assets)*
            }
        }
    });

    let writes = exposed.iter().map(|(f, t)| {
        let layout = row_index(f).map(|k| {
            quote! {
                <#t as #rows_path::SchemaRowsField>::write_row_contract(#k, sink);
            }
        });
        quote! {
            ::ipp_core::components::schema::write_string(sink, stringify!(#f));
            sink.write(&(::core::mem::offset_of!(Self, #f) as u32).to_le_bytes());
            sink.write(&(::core::mem::size_of::<#t>() as u32).to_le_bytes());
            sink.write(&(::core::mem::align_of::<#t>() as u32).to_le_bytes());
            sink.write(&[<#t as ::ipp_core::components::schema::SchemaField>::KIND as u8]);
            #layout
            if let Some(defaults) = &defaults {
                <#t as ::ipp_core::components::schema::SchemaField>::write_default(&defaults.#f, sink);
            }
        }
    });

    Ok(quote! {
        impl ::ipp_core::components::schema::SchemaComponent for #name {
            const FIELD_COUNT: usize = #count as usize;

            fn create() -> Option<Self> { #creation }

            fn has_field(offset: u32) -> bool {
                #(#row_has)*

                [#(#offsets),*].contains(&offset)
            }

            fn fields(&self) -> Vec<(u32, ::ipp_core::components::schema::FieldValue)> {
                vec![#(#reads),*]
            }

            fn field(&self, offset: u32) -> Result<::ipp_core::components::schema::FieldValue, ::ipp_core::components::schema::FieldError> {
                #(#getters)*

                #(#row_getters)*

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

                #(#row_setters)*

                Err(::ipp_core::components::schema::FieldError::UnknownField)
            }

            fn validate_field(
                offset: u32,
                kind: ::ipp_core::components::schema::FieldKind,
            ) -> Result<(), ::ipp_core::components::schema::FieldError> {
                #(#validation)*

                #(#row_validation)*

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

            #visit_row_assets
        }
    })
}
