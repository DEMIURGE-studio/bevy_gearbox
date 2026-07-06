use proc_macro::TokenStream;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Item, Type};

fn named(name: &str) -> proc_macro2::TokenStream {
    let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
    quote! { ::#ident }
}

fn gearbox_root() -> proc_macro2::TokenStream {
    match crate_name("bevy_diesel") {
        Ok(FoundCrate::Itself) => return quote! { ::bevy_diesel::gearbox },
        Ok(FoundCrate::Name(name)) => {
            let base = named(&name);
            return quote! { #base::gearbox };
        }
        Err(_) => {}
    }
    match crate_name("bevy_gearbox") {
        Ok(FoundCrate::Itself) => quote! { ::bevy_gearbox },
        Ok(FoundCrate::Name(name)) => named(&name),
        Err(_) => quote! { ::bevy_gearbox },
    }
}

/// Derive macro that implements [`GearboxMessage`] for a message struct and
/// auto-registers it with the gearbox schedule via `inventory`.
///
/// Mark the `Entity` field the message is addressed to with `#[gearbox(target)]`.
/// By default the message uses the `AcceptAll` validator; override it with a
/// container-level `#[gearbox(validator = MyValidator)]`.
///
/// The struct must also derive `Message` and `Clone` (and satisfy `TypePath`,
/// usually via `Reflect`) to meet `GearboxMessage`'s supertrait bounds. This
/// derive intentionally does *not* inject those for you.
///
/// # Example
///
/// ```ignore
/// use bevy::prelude::*;
/// use bevy_gearbox::GearboxMessage;
///
/// #[derive(Message, Clone, Reflect, GearboxMessage)]
/// struct Attack {
///     #[gearbox(target)]
///     machine: Entity,
///     damage: f32,
/// }
/// ```
///
/// With a custom validator:
///
/// ```ignore
/// #[derive(Message, Clone, Reflect, GearboxMessage)]
/// #[gearbox(validator = MyValidator)]
/// struct Fire {
///     #[gearbox(target)]
///     machine: Entity,
/// }
/// ```
#[proc_macro_derive(GearboxMessage, attributes(gearbox))]
pub fn derive_gearbox_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident.clone();

    // Container attribute: #[gearbox(validator = SomeType)] (optional).
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
            return e.to_compile_error().into();
        }
    }

    // Find the target field: exactly one field marked #[gearbox(target)].
    let data = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return syn::Error::new_spanned(
                &name,
                "#[derive(GearboxMessage)] supports only structs",
            )
            .to_compile_error()
            .into();
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
            .to_compile_error()
            .into();
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
                return e.to_compile_error().into();
            }
            if is_target {
                if target_field.is_some() {
                    return syn::Error::new_spanned(
                        field,
                        "multiple #[gearbox(target)] fields; exactly one is required",
                    )
                    .to_compile_error()
                    .into();
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
        .to_compile_error()
        .into();
    };

    let gearbox = gearbox_root();
    let validator_ty = match validator {
        Some(ty) => quote! { #ty },
        None => quote! { _gearbox::AcceptAll },
    };

    let expanded = quote! {
        const _: () = {
            use #gearbox as _gearbox;

            impl _gearbox::GearboxMessage for #name {
                type Validator = #validator_ty;

                fn target(&self) -> bevy::prelude::Entity {
                    self.#target_field
                }
            }

            _gearbox::inventory::submit! {
                _gearbox::registration::TransitionInstaller {
                    install: _gearbox::registration::register_transition::<#name>
                }
            }
        };
    };

    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a state component type via inventory.
///
/// # Example
///
/// ```ignore
/// #[state_component]
/// #[derive(Component, Reflect, Clone)]
/// struct MyFlag;
/// ```
#[proc_macro_attribute]
pub fn state_component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: Item = syn::parse(item).expect("#[state_component] must be applied to a type item");
    let name = match &parsed {
        Item::Struct(s) => &s.ident,
        Item::Enum(e) => &e.ident,
        _ => panic!("#[state_component] supports only structs or enums"),
    };

    let gearbox = gearbox_root();
    let expanded = quote! {
        #parsed

        const _: () = {
            use #gearbox as _gearbox;
            _gearbox::inventory::submit! {
                _gearbox::registration::StateInstaller {
                    install: _gearbox::registration::register_state_component::<#name>
                }
            }
        };
    };
    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a Bevy `States` bridge via inventory.
///
/// # Example
///
/// ```ignore
/// #[state_bridge]
/// #[derive(States, Component, Default, Clone, Hash, PartialEq, Eq, Debug)]
/// enum GameState { #[default] Menu, Playing }
/// ```
#[proc_macro_attribute]
pub fn state_bridge(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: Item = syn::parse(item).expect("#[state_bridge] must be applied to a type item");
    let name = match &parsed {
        Item::Struct(s) => &s.ident,
        Item::Enum(e) => &e.ident,
        _ => panic!("#[state_bridge] supports only structs or enums"),
    };

    let gearbox = gearbox_root();
    let expanded = quote! {
        #parsed

        const _: () = {
            use #gearbox as _gearbox;
            _gearbox::inventory::submit! {
                _gearbox::registration::StateBridgeInstaller {
                    install: _gearbox::registration::register_state_bridge::<#name>
                }
            }
        };
    };
    TokenStream::from(expanded)
}
