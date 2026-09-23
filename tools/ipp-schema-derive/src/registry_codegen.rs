use crate::identifier::{snake_case, upper_snake_case};
use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, Token, Visibility};

struct Entry {
    attrs: Vec<Attribute>,
    name: Ident,
    id: syn::LitInt,
}

struct Registry {
    vis: Visibility,
    name: Ident,
    entries: Vec<Entry>,
}

impl Parse for Registry {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let vis = input.parse()?;
        let name = input.parse()?;
        let body;
        syn::braced!(body in input);

        let mut entries = Vec::new();
        while !body.is_empty() {
            let attrs = body.call(Attribute::parse_outer)?;
            let name = body.parse()?;
            body.parse::<Token![=]>()?;
            let id = body.parse()?;
            body.parse::<Token![,]>()?;
            entries.push(Entry {
                attrs,
                name,
                id,
            });
        }

        Ok(Self {
            vis,
            name,
            entries,
        })
    }
}

/// Generates the only component ID map, typed sum, factories, and dispatch.
/// Example: `component_registry! { pub ComponentValue { Scalar = 1, } }`.
pub(super) fn expand(input: TokenStream) -> TokenStream {
    let Registry {
        vis,
        name,
        mut entries,
    } = syn::parse_macro_input!(input as Registry);

    let mut runtimes = std::collections::BTreeMap::new();
    for entry in &mut entries {
        let mut attrs = Vec::new();
        for attr in std::mem::take(&mut entry.attrs) {
            if attr.path().is_ident("runtime") {
                match attr.parse_args::<syn::Type>() {
                    Ok(ty) => {
                        runtimes.insert(entry.name.to_string(), ty);
                    }
                    Err(error) => return error.into_compile_error().into(),
                }
            } else {
                attrs.push(attr);
            }
        }
        entry.attrs = attrs;
    }

    let mut ids = std::collections::BTreeSet::new();
    for e in &entries {
        let id = match e.id.base10_parse::<u16>() {
            Ok(x) => x,
            Err(e) => return e.into_compile_error().into(),
        };
        if id == 0 || !ids.insert(id) {
            return syn::Error::new_spanned(
                &e.id,
                "component IDs must be unique nonzero u16 values",
            )
            .into_compile_error()
            .into();
        }
    }

    let variants = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            ..
        } = e;
        quote! {
            #(#attrs)*
            #name(crate::components::#name),
        }
    });

    let storage_fields = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let field = snake_case(component);
        let runtime = runtimes
            .get(&component.to_string())
            .map_or_else(|| quote!(()), |ty| quote!(#ty));
        quote! {
            #(#attrs)*
            #field: ::ipp_core::components::storage::Paged<crate::components::#component, #runtime>,
        }
    });

    let storage_reserve = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let field = snake_case(&e.name);
        quote! {
            #(#attrs)*
            self.#field.reserve(slots);
        }
    });

    let storage_try_reserve = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let field = snake_case(&e.name);
        quote! {
            #(#attrs)*
            self.#field.try_reserve(slots)?;
        }
    });

    let storage_clear = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #id => self.#field.set(index, None),
        }
    });

    let storage_set = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #name::#component(mut value) => {
                if let Some(previous) = self.#field.get_mut(index) {
                    ::ipp_core::components::schema::ComponentLifecycle::preserve_runtime(&mut value, previous);
                }

                self.#field.set(index, Some(value));
            },
        }
    });

    let storage_field_getters = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #id => {
                let value = self.#field.get(index)?;
                ::ipp_core::components::schema::SchemaComponent::field(value, offset).ok().or_else(|| {
                    ::ipp_core::components::schema::ComponentLifecycle::dynamic_properties(value)?.field(offset).ok()
                })
            },
        }
    });

    let storage_numeric_support = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        quote! {
            #(#attrs)*
            #id => <crate::components::#component as ::ipp_core::components::schema::ComponentLifecycle>::supports_numeric_property(offset),
        }
    });

    let storage_numeric_validation = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #id => ::ipp_core::components::schema::ComponentLifecycle::validate_numeric_properties(
                self.#field.get(index).ok_or(crate::ErrorReason::MissingComponent)?,
                fields,
            ),
        }
    });

    let storage_numeric_writes = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #id => {
                let value = self.#field.get_mut(index).ok_or(crate::ErrorReason::MissingComponent)?;

                for (offset, field) in fields {
                    if ::ipp_core::components::dynamic_properties::is_dynamic_field(*offset) {
                        ::ipp_core::components::schema::ComponentLifecycle::dynamic_properties_mut(value)
                            .ok_or(crate::ErrorReason::InvalidField)?
                            .set_field(*offset, field.clone())
                            .map_err(|_| crate::ErrorReason::InvalidField)?;
                    } else {
                        ::ipp_core::components::schema::SchemaComponent::set_field(value, *offset, field.clone())
                            .map_err(|_| crate::ErrorReason::InvalidField)?;
                    }
                }

                Ok(())
            },
        }
    });

    let storage_get = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #id => self.#field.get(index).cloned().map(#name::#component),
        }
    });

    let storage_resource_demand = entries.iter().map(|e| {
        let Entry {
            attrs,
            name: component,
            id,
        } = e;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            #id => {
                if let Some(value) = self.#field.get(index) {
                    ::ipp_core::components::schema::ComponentLifecycle::resource_demand(value, demand);
                }
            },
        }
    });

    let storage_inspect = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let field = snake_case(component);
        quote! {
            #(#attrs)*
            if let Some(value) = self.#field.get(index) {
                output.push(#name::#component(value.clone()));
            }
        }
    });

    let storage_accessors = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let field = snake_case(component);
        let mutable = Ident::new(&format!("{field}_mut"), field.span());
        let pointer = Ident::new(&format!("{field}_ptr"), field.span());
        let runtime_access = runtimes.get(&component.to_string()).map(|runtime| {
            let getter = Ident::new(&format!("{field}_runtime"), field.span());
            let mutable = Ident::new(&format!("{field}_runtime_mut"), field.span());
            let pointer = Ident::new(&format!("{field}_runtime_ptr"), field.span());
            quote! {
                #(#attrs)*
                pub(crate) fn #getter(&self, index: usize) -> Option<&#runtime> {
                    self.#field.runtime(index)
                }

                #(#attrs)*
                pub(crate) fn #mutable(&mut self, index: usize) -> Option<&mut #runtime> {
                    self.#field.runtime_mut(index)
                }

                #(#attrs)*
                #[allow(dead_code)]
                pub(crate) fn #pointer(&self, index: usize) -> Option<std::ptr::NonNull<#runtime>> {
                    self.#field.runtime_ptr(index)
                }
            }
        });
        quote! {
            #runtime_access
            #(#attrs)*
            pub(crate) fn #field(&self, index: usize) -> Option<&crate::components::#component> {
                self.#field.get(index)
            }

            #(#attrs)*
            pub(crate) fn #mutable(
                &mut self,
                index: usize,
            ) -> Option<&mut crate::components::#component> {
                self.#field.get_mut(index)
            }

            #(#attrs)*
            #[allow(dead_code)] // Only compiled numeric consumers bind pointers.
            pub(crate) fn #pointer(&self, index: usize) -> Option<std::ptr::NonNull<crate::components::#component>> {
                self.#field.bound_ptr(index)
            }
        }
    });

    let factories = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            id,
        } = e;
        quote! {
            #(#attrs)*
            #id => <crate::components::#name as ::ipp_core::components::schema::SchemaComponent>::create()
                .map(Self::#name).ok_or(::ipp_core::components::schema::FieldError::CreationUnavailable),
        }
    });

    let type_ids = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            id,
        } = e;
        quote! {
            #(#attrs)*
            Self::#name(_) => #id,
        }
    });

    let id_constants = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let constant = upper_snake_case(&e.name);
        let id = &e.id;
        quote! {
            #(#attrs)*
            /// Compiled identity of this registered component.
            pub const #constant: u16 = #id;
        }
    });

    let setters = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            ..
        } = e;

        quote! {
            #(#attrs)*
            Self::#name(v) => ::ipp_core::components::schema::SchemaComponent::set_field(v, offset, value),
        }
    });

    let reads = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            ..
        } = e;
        quote! {
            #(#attrs)*
            Self::#name(v) => ::ipp_core::components::schema::SchemaComponent::fields(v),
        }
    });

    let getters = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            ..
        } = e;
        quote! {
            #(#attrs)*
            Self::#name(v) => ::ipp_core::components::schema::SchemaComponent::field(v, offset),
        }
    });

    let field_counts = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            id,
        } = e;
        quote! {
            #(#attrs)*
            #id => Ok(<crate::components::#name as ::ipp_core::components::schema::SchemaComponent>::FIELD_COUNT),
        }
    });

    let has_fields = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let id = &e.id;
        quote! { #(#attrs)* #id => <crate::components::#component as ::ipp_core::components::schema::SchemaComponent>::has_field(offset), }
    });

    let dynamic_reads = entries.iter().map(|e| {
        let attrs = &e.attrs; let component = &e.name;
        quote! { #(#attrs)* Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::dynamic_properties(value), }
    });
    let dynamic_writes = entries.iter().map(|e| {
        let attrs = &e.attrs; let component = &e.name;
        quote! { #(#attrs)* Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::dynamic_properties_mut(value), }
    });
    let dynamic_support = entries.iter().map(|e| {
        let attrs = &e.attrs; let component = &e.name; let id = &e.id;
        quote! { #(#attrs)* #id => <crate::components::#component as ::ipp_core::components::schema::ComponentLifecycle>::supports_dynamic_properties(), }
    });

    let activation_bytes = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! { #(#attrs)* Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::activation_bytes(value), }
    });

    let deferred_preparations = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! { #(#attrs)* Self::#component(_) => <crate::components::#component as ::ipp_core::components::schema::ComponentLifecycle>::defers_preparation(), }
    });

    let preparations = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! { #(#attrs)* Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::prepare_effective(value, max_activation_bytes).map(Self::#component), }
    });

    let lifecycle_validation = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! {
            #(#attrs)*
            Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::validate(value),
        }
    });

    let null_entity = entries.iter().map(|e| {
        let Entry { attrs, name, id } = e;
        quote! {
            #(#attrs)*
            #id => <crate::components::#name as ::ipp_core::components::schema::ComponentLifecycle>::accepts_null_entity(offset),
        }
    });

    let lifecycle_field_validation = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! {
            #(#attrs)*
            Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::validate_field(value, offset),
        }
    });

    let retained_bytes = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! {
            #(#attrs)*
            Self::#component(value) => ::ipp_core::components::schema::SchemaComponent::retained_bytes(value),
        }
    });

    let required_components = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let id = &e.id;
        quote! {
            #(#attrs)*
            #id => <crate::components::#component as ::ipp_core::components::schema::ComponentLifecycle>::required_components(),
        }
    });

    let asset_references = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        let id = &e.id;
        quote! {
            #(#attrs)*
            #id => <crate::components::#component as ::ipp_core::components::schema::ComponentLifecycle>::asset_references(),
        }
    });

    let resource_demand = entries.iter().map(|e| {
        let attrs = &e.attrs;
        let component = &e.name;
        quote! {
            #(#attrs)*
            Self::#component(value) => ::ipp_core::components::schema::ComponentLifecycle::resource_demand(value, demand),
        }
    });

    let validates = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            id,
        } = e;

        quote! {
            #(#attrs)*
            #id => <crate::components::#name as ::ipp_core::components::schema::SchemaComponent>::validate_field(
                offset,
                kind,
            ),
        }
    });

    let write = entries.iter().map(|e| {
        let Entry {
            attrs,
            name,
            id,
        } = e;

        quote! {
            #(#attrs)*
            {
                sink.write(&(#id as u16).to_le_bytes());
                <crate::components::#name as ::ipp_core::components::schema::SchemaComponent>::write_contract(sink);
                sink.write(&[u8::from(<crate::components::#name as ::ipp_core::components::schema::ComponentLifecycle>::supports_dynamic_properties())]);
            }
        }
    });

    let count = entries.iter().map(|e| {
        let attrs = &e.attrs;
        quote! {
            #(#attrs)*
            {
                count += 1;
            }
        }
    });

    quote! {
        #[derive(Clone, Debug, PartialEq)]
        #[allow(missing_docs)]
        #vis enum #name {
            #(#variants)*
        }

        /// Registry-generated stable effective stores, one typed paged store per component.
        #[derive(Default)]
        pub(crate) struct ComponentStorage {
            #(#storage_fields)*
        }

        impl ComponentStorage {
            pub(crate) fn try_reserve(&mut self, slots: usize) -> Result<(), crate::ErrorReason> {
                #(#storage_try_reserve)*
                Ok(())
            }

            pub(crate) fn reserve(&mut self, slots: usize) {
                #(#storage_reserve)*
            }

            pub(crate) fn clear(
                &mut self,
                component: u16,
                index: usize,
            ) -> Result<(), ::ipp_core::components::schema::FieldError> {
                match component {
                    #(#storage_clear)*
                    _ => return Err(::ipp_core::components::schema::FieldError::UnknownComponent),
                }
                Ok(())
            }

            pub(crate) fn set(&mut self, index: usize, value: #name) {
                match value {
                    #(#storage_set)*
                }
            }

            pub(crate) fn resource_demand(
                &self,
                component: u16,
                index: usize,
                demand: &mut std::collections::BTreeSet<crate::services::asset_management::service::AssetDemandSelection>,
            ) {
                match component {
                    #(#storage_resource_demand)*
                    _ => {},
                }
            }

            pub(crate) fn field(&self, component: u16, index: usize, offset: u32) -> Option<::ipp_core::components::schema::FieldValue> {
                match component { #(#storage_field_getters)* _ => None }
            }

            pub(crate) fn supports_numeric_property(component: u16, offset: u32) -> bool {
                match component { #(#storage_numeric_support)* _ => false }
            }

            pub(crate) fn validate_numeric_properties(&self, component: u16, index: usize, fields: &[(u32, ::ipp_core::components::schema::FieldValue)]) -> Result<(), crate::ErrorReason> {
                match component { #(#storage_numeric_validation)* _ => Err(crate::ErrorReason::InvalidField) }
            }

            pub(crate) fn write_numeric_properties(&mut self, component: u16, index: usize, fields: &[(u32, ::ipp_core::components::schema::FieldValue)]) -> Result<(), crate::ErrorReason> {
                match component { #(#storage_numeric_writes)* _ => Err(crate::ErrorReason::InvalidField) }
            }

            pub(crate) fn get(&self, component: u16, index: usize) -> Option<#name> {
                match component {
                    #(#storage_get)*
                    _ => None,
                }
            }

            pub(crate) fn inspect(&self, index: usize, output: &mut Vec<#name>) {
                #(#storage_inspect)*
            }

            #(#storage_accessors)*
        }

        impl #name {
            #(#id_constants)*

            /// Own exposed values for read-only inspection and export.
            pub fn fields(&self) -> Vec<(u32, ::ipp_core::components::schema::FieldValue)> {
                let mut fields = match self { #(#reads)* };

                if let Some(properties) = self.dynamic_properties() {
                    fields.extend(properties.fields());
                }

                fields
            }

            /// Own only the selected value. Numeric reads do not allocate.
            pub fn field(&self, offset: u32) -> Result<::ipp_core::components::schema::FieldValue, ::ipp_core::components::schema::FieldError> {
                let result = match self { #(#getters)* };
                match result {
                    Err(::ipp_core::components::schema::FieldError::UnknownField) => self.dynamic_properties()
                        .ok_or(::ipp_core::components::schema::FieldError::UnknownField)?.field(offset),
                    result => result,
                }
            }

            /// Borrow the instance dynamic-property store when supported.
            pub fn dynamic_properties(&self) -> Option<&crate::DynamicProperties> {
                match self { #(#dynamic_reads)* }
            }

            /// Mutably borrow named properties at an exclusive mutation boundary.
            pub fn dynamic_properties_mut(&mut self) -> Option<&mut crate::DynamicProperties> {
                match self { #(#dynamic_writes)* }
            }

            /// Whether the compiled type exposes component-owned named properties.
            pub fn supports_dynamic_properties(id: u16) -> bool {
                match id { #(#dynamic_support)* _ => false }
            }

            /// Number of exposed fields in one compiled component.
            pub fn field_count(id: u16) -> Result<usize, ::ipp_core::components::schema::FieldError> {
                match id {
                    #(#field_counts)*
                    _ => Err(::ipp_core::components::schema::FieldError::UnknownComponent),
                }
            }

            /// Check an exposed offset independently of optional creation.
            pub fn has_field(id: u16, offset: u32) -> bool {
                if ::ipp_core::components::dynamic_properties::is_dynamic_field(offset) {
                    return Self::supports_dynamic_properties(id);
                }

                match id { #(#has_fields)* _ => false }
            }

            /// Prepare typed effective resources before installing an affected component.
            pub(crate) fn prepare_effective(&self, max_activation_bytes: usize) -> Result<Self, crate::ErrorReason> {
                match self { #(#preparations)* }
            }

            /// Whether effective preparation is an infallible copy made once per commit.
            pub(crate) fn defers_preparation(&self) -> bool {
                match self { #(#deferred_preparations)* }
            }

            /// Owned allocations created by effective activation, excluding exposed fields.
            pub(crate) fn activation_bytes(&self) -> usize {
                match self { #(#activation_bytes)* }
            }

            /// Run typed lifecycle validation without descriptive schema traversal.
            pub(crate) fn validate_lifecycle(&self) -> Result<(), crate::ErrorReason> {
                match self {
                    #(#lifecycle_validation)*
                }
            }

            /// Resolve optional entity references using the component's lifecycle policy.
            pub(crate) fn accepts_null_entity(id: u16, offset: u32) -> bool {
                match id { #(#null_entity)* _ => false }
            }

            /// Run typed field-local lifecycle validation after replacement.
            pub(crate) fn validate_field_lifecycle(
                &self,
                offset: u32,
            ) -> Result<(), crate::ErrorReason> {
                match self {
                    #(#lifecycle_field_validation)*
                }
            }

            /// Owned heap bytes retained by exposed component fields.
            pub(crate) fn retained_bytes(&self) -> Option<usize> {
                let bytes: Option<usize> = match self { #(#retained_bytes)* };
                bytes?.checked_add(self.dynamic_properties().map_or(0, |p| p.retained_bytes()))
            }

            /// Default dependencies declared by the compiled component implementation.
            pub(crate) fn required_components(id: u16) -> &'static [u16] {
                match id {
                    #(#required_components)*
                    _ => &[],
                }
            }

            /// Typed asset reference declaration for one compiled component.
            pub fn asset_references(id: u16) -> &'static [::ipp_core::components::schema::ComponentAssetReference] {
                match id {
                    #(#asset_references)*
                    _ => &[],
                }
            }

            /// Append typed resource selections required by this component.
            pub(crate) fn resource_demand(
                &self,
                demand: &mut std::collections::BTreeSet<crate::services::asset_management::service::AssetDemandSelection>,
            ) {
                match self {
                    #(#resource_demand)*
                }
            }

            /// Creates the registered component's authored default.
            pub fn create(id: u16) -> Result<Self, ::ipp_core::components::schema::FieldError> {
                match id {
                    #(#factories)*
                    _ => Err(::ipp_core::components::schema::FieldError::UnknownComponent),
                }
            }

            /// Compiled component identity.
            pub fn type_id(&self) -> u16 {
                match self {
                    #(#type_ids)*
                }
            }

            /// Replaces exactly one exposed field through its Rust type.
            pub fn set_field(
                &mut self,
                offset: u32,
                value: ::ipp_core::components::schema::FieldValue,
            ) -> Result<(), ::ipp_core::components::schema::FieldError> {
                if ::ipp_core::components::dynamic_properties::is_dynamic_field(offset) {
                    return self.dynamic_properties_mut()
                        .ok_or(::ipp_core::components::schema::FieldError::UnknownField)?
                        .set_field(offset, value);
                }

                match self { #(#setters)* }
            }

            /// Validates a wire address without creating a component.
            pub fn validate_field(
                id: u16,
                offset: u32,
                kind: ::ipp_core::components::schema::FieldKind,
            ) -> Result<(), ::ipp_core::components::schema::FieldError> {
                if ::ipp_core::components::dynamic_properties::is_dynamic_field(offset) && Self::supports_dynamic_properties(id) {
                    let expected = if offset == ::ipp_core::components::dynamic_properties::DYNAMIC_METADATA {
                        ::ipp_core::components::schema::FieldKind::Bytes
                    } else {
                        ::ipp_core::components::schema::FieldKind::Dynamic
                    };

                    return if kind == expected {
                        Ok(())
                    } else {
                        Err(::ipp_core::components::schema::FieldError::WrongType)
                    };
                }

                match id {
                    #(#validates)*
                    _ => Err(::ipp_core::components::schema::FieldError::UnknownComponent),
                }
            }

            /// Streams the compiled registry and target layouts in declaration order.
            pub fn write_contract(sink: &mut impl ::ipp_core::components::schema::ContractSink) {
                let mut count = 0u16;
                #(#count)*

                sink.write(&count.to_le_bytes());
                #(#write)*
            }
        }
    }
    .into()
}
