//! Method-signature adapters; Rust traits classify dependency types.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    FnArg, ImplItem, ItemImpl, Lifetime, Pat, Token, Type, parse::Parser, punctuated::Punctuated,
    visit_mut::VisitMut,
};

struct ParameterLifetime(Lifetime);

impl VisitMut for ParameterLifetime {
    fn visit_type_reference_mut(&mut self, reference: &mut syn::TypeReference) {
        reference.lifetime = Some(self.0.clone());
        syn::visit_mut::visit_type_reference_mut(self, reference);
    }
}

pub(super) fn expand(attributes: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let order = Punctuated::<syn::Expr, Token![,]>::parse_terminated.parse2(attributes)?;
    let implementation: ItemImpl = syn::parse2(input)?;
    if implementation.trait_.is_some() || !implementation.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &implementation,
            "system_update requires a concrete inherent impl",
        ));
    }
    let Type::Path(owner_path) = &*implementation.self_ty else {
        return Err(syn::Error::new_spanned(
            &implementation.self_ty,
            "expected concrete System type",
        ));
    };
    let owner = &implementation.self_ty;
    let handles = format_ident!(
        "{}UpdateBindings",
        owner_path.path.segments.last().unwrap().ident
    );
    let update = implementation
        .items
        .iter()
        .find_map(|item| match item {
            ImplItem::Fn(method) if method.sig.ident == "update" => Some(method),
            _ => None,
        })
        .ok_or_else(|| syn::Error::new_spanned(&implementation, "expected update method"))?;
    if !update.sig.generics.params.is_empty() || update.sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            &update.sig,
            "update must be synchronous and nongeneric",
        ));
    }
    let arguments: Vec<_> = update.sig.inputs.iter().collect();
    if arguments.len() < 3
        || !matches!(arguments[0], FnArg::Receiver(receiver) if matches!(receiver.kind, syn::ReceiverKind::Reference(_, _, Some(_))))
    {
        return Err(syn::Error::new_spanned(
            &update.sig,
            "expected update(&mut self, ecs, dependency parameters..., dt)",
        ));
    }
    let mut fields = Vec::new();
    let mut metadata = Vec::new();
    let mut services = Vec::new();
    let mut initializers = Vec::new();
    let mut borrows = Vec::new();
    let mut calls = Vec::new();
    for argument in &arguments[2..arguments.len() - 1] {
        let FnArg::Typed(argument) = argument else {
            unreachable!()
        };
        let Pat::Ident(name) = &*argument.pat else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "dependency parameter must have a name",
            ));
        };
        let name = &name.ident;
        let attributes = &argument.attrs;
        let ty = &argument.ty;
        let mut stored: Type = syn::parse2(quote! { #ty })?;
        ParameterLifetime(Lifetime::new("'static", proc_macro2::Span::call_site()))
            .visit_type_mut(&mut stored);
        let mut borrowed: Type = syn::parse2(quote! { #ty })?;
        ParameterLifetime(Lifetime::new("'__ipp", proc_macro2::Span::call_site()))
            .visit_type_mut(&mut borrowed);
        fields.push(quote! {
            #(#attributes)*
            pub #name: <#stored as ::ipp_core::systems::SystemUpdateParameter<'static>>::Binding
        });
        metadata.push(quote! {
            #(#attributes)*
            <#stored as ::ipp_core::systems::SystemUpdateParameter<'static>>::DEPENDENCY
        });
        services.push(quote! {
            #(#attributes)*
            <#stored as ::ipp_core::systems::SystemUpdateParameter<'static>>::SERVICE
        });
        initializers.push(quote! {
            #(#attributes)*
            #name: <#stored as ::ipp_core::systems::SystemUpdateParameter<'static>>::bind(context)?
        });
        borrows.push(quote! {
            #(#attributes)*
            let #name = <#borrowed as ::ipp_core::systems::SystemUpdateParameter<'__ipp>>::borrow(
                bindings.#name,
                &mut inputs,
            );
        });
        calls.push(quote! {
            #(#attributes)*
            #name
        });
    }
    metadata.extend(order.iter().map(|edge| quote! { Some(#edge) }));
    Ok(quote! {
        #implementation

        #[doc(hidden)]
        #[derive(Clone, Copy)]
        pub struct #handles {
            #(#fields,)*
        }

        impl ::ipp_core::systems::SystemBoundUpdate for #owner {
            type Bindings = #handles;

            fn dependencies() -> &'static [::ipp_core::systems::SystemDependency] {
                const OPTIONS: &[Option<::ipp_core::systems::SystemDependency>] = &[
                    #(#metadata,)*
                ];
                const METADATA: ([::ipp_core::systems::SystemDependency; OPTIONS.len()], usize) =
                    ::ipp_core::systems::compact_dependencies([#(#metadata,)*]);

                &METADATA.0[..METADATA.1]
            }

            fn bind(
                context: &::ipp_core::systems::SystemInitContext<'_>,
            ) -> Result<Self::Bindings, ::ipp_core::systems::SystemInitError> {
                ::ipp_core::systems::validate_parameter_services(&[#(#services,)*])?;

                Ok(#handles {
                    #(#initializers,)*
                })
            }

            fn update_bound<'__ipp>(
                &mut self,
                bindings: Self::Bindings,
                context: &'__ipp mut ::ipp_core::systems::SystemUpdateContext<'_, '_>,
            ) {
                let (ecs, mut inputs, dt) = context.inputs();
                #(#borrows)*

                <#owner>::update(self, ecs, #(#calls,)* dt);
            }
        }
    })
}
