//! Token-level implementation of the `bevy_gearbox` macros.
//!
//! This crate contains no `#[proc_macro]` entry points. It exposes plain
//! functions over [`proc_macro2::TokenStream`] so that more than one
//! proc-macro crate can wrap the same logic while emitting a different root
//! path to gearbox.
//!
//! `bevy_gearbox_macros` is the wrapper used by crates that depend on
//! `bevy_gearbox` directly. A crate that re-exports gearbox (for example as
//! `my_crate::gearbox`) and wants its own users to write
//! `#[derive(GearboxMessage)]` without adding `bevy_gearbox` to their
//! `Cargo.toml` writes a thin wrapper of its own:
//!
//! ```ignore
//! // in my_crate_macros (a proc-macro crate)
//! #[proc_macro_derive(GearboxMessage, attributes(gearbox))]
//! pub fn derive_gearbox_message(input: TokenStream) -> TokenStream {
//!     let root = quote! { ::my_crate::gearbox };
//!     bevy_gearbox_macros_impl::derive_gearbox_message(input.into(), root).into()
//! }
//! ```
//!
//! Every function takes the macro input and a `root` path that must resolve,
//! at the call site, to the `bevy_gearbox` crate root (or a module that
//! re-exports all of it). Errors are returned as `compile_error!` tokens.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Item, Type};

/// Implementation of `#[derive(GearboxMessage)]`.
///
/// Emits an `impl GearboxMessage` that returns the field marked
/// `#[gearbox(target)]`, using `AcceptAll` as the validator unless the
/// container carries `#[gearbox(validator = Ty)]`, and submits an
/// `inventory` registration so the message listener is installed by
/// `GearboxPlugin`.
pub fn derive_gearbox_message(input: TokenStream, root: TokenStream) -> TokenStream {
    let input: DeriveInput = match syn::parse2(input) {
        Ok(i) => i,
        Err(e) => return e.to_compile_error(),
    };
    let name = input.ident.clone();

    let mut validator: Option<Type> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("gearbox") {
            continue;
        }
        let res = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("validator") {
                validator = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown `gearbox` attribute; expected `validator = <Type>`"))
            }
        });
        if let Err(e) = res {
            return e.to_compile_error();
        }
    }

    let data = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return syn::Error::new_spanned(&name, "#[derive(GearboxMessage)] supports only structs")
                .to_compile_error();
        }
    };
    let fields = match &data.fields {
        Fields::Named(f) => &f.named,
        _ => {
            return syn::Error::new_spanned(
                &name,
                "#[derive(GearboxMessage)] requires named fields; mark the addressed \
                 entity field with #[gearbox(target)]",
            )
            .to_compile_error();
        }
    };

    let mut target_field = None;
    for field in fields {
        for attr in &field.attrs {
            if !attr.path().is_ident("gearbox") {
                continue;
            }
            let mut is_target = false;
            let res = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("target") {
                    is_target = true;
                    Ok(())
                } else {
                    Err(meta.error("unknown `gearbox` field attribute; expected `target`"))
                }
            });
            if let Err(e) = res {
                return e.to_compile_error();
            }
            if is_target {
                if target_field.is_some() {
                    return syn::Error::new_spanned(
                        field,
                        "multiple #[gearbox(target)] fields; exactly one is required",
                    )
                    .to_compile_error();
                }
                target_field = field.ident.clone();
            }
        }
    }

    let Some(target_field) = target_field else {
        return syn::Error::new_spanned(
            &name,
            "#[derive(GearboxMessage)] requires exactly one field marked \
             #[gearbox(target)] (the Entity the message is addressed to)",
        )
        .to_compile_error();
    };

    let validator_ty = match validator {
        Some(ty) => quote! { #ty },
        None => quote! { _gearbox::AcceptAll },
    };

    quote! {
        const _: () = {
            use #root as _gearbox;

            impl _gearbox::GearboxMessage for #name {
                type Validator = #validator_ty;

                fn target(&self) -> _gearbox::__bevy::prelude::Entity {
                    self.#target_field
                }
            }

            _gearbox::inventory::submit! {
                _gearbox::registration::TransitionInstaller {
                    install: _gearbox::registration::register_transition::<#name>
                }
            }
        };
    }
}

/// Implementation of `#[state_component]`.
///
/// Re-emits the item unchanged and submits an `inventory` registration so
/// `GearboxPlugin` installs the `StateComponent<T>` systems for it.
pub fn state_component(item: TokenStream, root: TokenStream) -> TokenStream {
    register_type_item(
        item,
        root,
        "#[state_component]",
        quote! { StateInstaller },
        quote! { register_state_component },
    )
}

/// Implementation of `#[state_bridge]`.
///
/// Re-emits the item unchanged and submits an `inventory` registration so
/// `GearboxPlugin` installs the Bevy `States` bridge for it.
pub fn state_bridge(item: TokenStream, root: TokenStream) -> TokenStream {
    register_type_item(
        item,
        root,
        "#[state_bridge]",
        quote! { StateBridgeInstaller },
        quote! { register_state_bridge },
    )
}

fn register_type_item(
    item: TokenStream,
    root: TokenStream,
    attr_name: &str,
    installer: TokenStream,
    register_fn: TokenStream,
) -> TokenStream {
    let parsed: Item = match syn::parse2(item) {
        Ok(i) => i,
        Err(e) => return e.to_compile_error(),
    };
    let name = match &parsed {
        Item::Struct(s) => s.ident.clone(),
        Item::Enum(e) => e.ident.clone(),
        other => {
            return syn::Error::new_spanned(
                other,
                format!("{attr_name} supports only structs or enums"),
            )
            .to_compile_error();
        }
    };

    quote! {
        #parsed

        const _: () = {
            use #root as _gearbox;
            _gearbox::inventory::submit! {
                _gearbox::registration::#installer {
                    install: _gearbox::registration::#register_fn::<#name>
                }
            }
        };
    }
}
