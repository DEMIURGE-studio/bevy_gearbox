use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Item, ItemImpl, Meta, Expr, Path, Data, Fields};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::Token;

/// Attribute macro that turns a struct into a gearbox transition message.
///
/// The struct must have a field named `machine` of type `Entity`.
/// This generates:
/// - `#[derive(Message, Clone)]` on the struct
/// - `impl GearboxMessage` with `type Validator = AcceptAll`
/// - An inventory auto-registration entry
///
/// # Example
///
/// ```rust
/// use bevy::prelude::*;
///
/// #[gearbox_message]
/// struct Attack {
///     machine: Entity,
///     damage: f32,
/// }
/// ```
#[proc_macro_attribute]
pub fn gearbox_message(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: Item = syn::parse(item)
        .expect("#[gearbox_message] must be applied to a struct");

    let name = match &parsed {
        Item::Struct(s) => {
            // Verify it has a `machine` field
            match &s.fields {
                Fields::Named(fields) => {
                    let has_machine = fields.named.iter().any(|f| {
                        f.ident.as_ref().map(|i| i == "machine").unwrap_or(false)
                    });
                    if !has_machine {
                        return syn::Error::new_spanned(
                            &s.ident,
                            "#[gearbox_message] requires a `machine: Entity` field",
                        )
                        .to_compile_error()
                        .into();
                    }
                }
                _ => {
                    return syn::Error::new_spanned(
                        &s.ident,
                        "#[gearbox_message] can only be applied to structs with named fields",
                    )
                    .to_compile_error()
                    .into();
                }
            }
            s.ident.clone()
        }
        _ => panic!("#[gearbox_message] can only be applied to structs"),
    };

    let expanded = quote! {
        #[derive(bevy::prelude::Message, Clone)]
        #parsed

        impl bevy_gearbox::GearboxMessage for #name {
            type Validator = bevy_gearbox::AcceptAll;

            fn machine(&self) -> bevy::prelude::Entity {
                self.machine
            }
        }

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::TransitionInstaller {
                install: bevy_gearbox::registration::register_transition::<#name>
            }
        }
    };

    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a transition message type via inventory.
///
/// Use this on types that already implement `GearboxMessage` manually
/// (e.g. with a custom validator). The macro just submits the inventory installer.
///
/// # Example
///
/// ```rust
/// #[transition_message]
/// #[derive(Message, Clone)]
/// struct Attack {
///     machine: Entity,
///     damage: f32,
/// }
///
/// impl GearboxMessage for Attack {
///     type Validator = MyCustomValidator;
///     fn machine(&self) -> Entity { self.machine }
/// }
/// ```
#[proc_macro_attribute]
pub fn transition_message(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: Item = syn::parse(item).expect("#[transition_message] must be applied to a type item");
    let name = match &parsed {
        Item::Struct(s) => &s.ident,
        Item::Enum(e) => &e.ident,
        _ => panic!("#[transition_message] supports only structs or enums"),
    };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::TransitionInstaller {
                install: bevy_gearbox::registration::register_transition::<#name>
            }
        }
    };
    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a state component type via inventory.
///
/// # Example
///
/// ```rust
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

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::StateInstaller {
                install: bevy_gearbox::registration::register_state_component::<#name>
            }
        }
    };
    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a parameter with guard wiring and optional sync binding.
///
/// # Example
///
/// ```rust
/// #[gearbox_param(kind = "float", source = Hitpoints)]
/// #[derive(Component)]
/// struct HpParam;
/// ```
#[proc_macro_attribute]
pub fn gearbox_param(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse(attr)
        .expect("failed to parse #[gearbox_param] arguments");
    let parsed: Item = syn::parse(item).expect("#[gearbox_param] must be applied to a type item");
    let name = match &parsed {
        Item::Struct(s) => &s.ident,
        Item::Enum(e) => &e.ident,
        _ => panic!("#[gearbox_param] supports only structs or enums"),
    };

    let mut kind: Option<String> = None;
    let mut source_path: Option<Path> = None;
    for meta in args {
        match meta {
            Meta::NameValue(nv) if nv.path.is_ident("kind") => {
                if let Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(lit_str), .. }) = nv.value {
                    kind = Some(lit_str.value());
                } else { panic!("kind must be a string literal"); }
            }
            Meta::NameValue(nv) if nv.path.is_ident("source") => {
                if let Expr::Path(p) = nv.value { source_path = Some(p.path); }
                else { panic!("source must be a type path"); }
            }
            _ => {}
        }
    }

    let kind = kind.expect("#[gearbox_param] requires kind = \"bool|int|float\"");

    let guard_install = if kind == "bool" {
        quote! { bevy_gearbox::registration::register_bool_param::<#name> }
    } else if kind == "int" {
        quote! { bevy_gearbox::registration::register_int_param::<#name> }
    } else if kind == "float" {
        quote! { bevy_gearbox::registration::register_float_param::<#name> }
    } else {
        panic!("invalid kind; expected bool|int|float");
    };

    let sync_install = if let Some(src_ty) = source_path.clone() {
        if kind == "int" {
            quote! { bevy_gearbox::registration::register_int_param_binding::<#src_ty, #name> }
        } else if kind == "float" {
            quote! { bevy_gearbox::registration::register_float_param_binding::<#src_ty, #name> }
        } else {
            quote!{}
        }
    } else {
        quote!{}
    };

    let binding_installer = if source_path.is_some() {
        if kind == "int" || kind == "float" {
            let ty = if kind == "int" {
                quote! { bevy_gearbox::registration::IntParamBindingInstaller }
            } else {
                quote! { bevy_gearbox::registration::FloatParamBindingInstaller }
            };
            quote! {
                bevy_gearbox::inventory::submit! { #ty { install: #sync_install } }
            }
        } else {
            quote!{}
        }
    } else { quote!{} };

    let guard_installer_ty = if kind == "bool" {
        quote! { bevy_gearbox::registration::BoolParamInstaller }
    } else if kind == "int" {
        quote! { bevy_gearbox::registration::IntParamInstaller }
    } else {
        quote! { bevy_gearbox::registration::FloatParamInstaller }
    };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            #guard_installer_ty { install: #guard_install }
        }

        #binding_installer
    };
    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a Bevy `States` bridge via inventory.
///
/// # Example
///
/// ```rust
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

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::StateBridgeInstaller {
                install: bevy_gearbox::registration::register_state_bridge::<#name>
            }
        }
    };
    TokenStream::from(expanded)
}

/// Attribute macro to auto-register a side effect via inventory.
///
/// Place on an `impl SideEffect<M> for S` block. The macro extracts both
/// the message type `M` and the side effect type `S` from the impl header
/// and submits an inventory installer.
///
/// # Example
///
/// ```rust
/// #[side_effect]
/// impl SideEffect<StartInvoke> for GoOff {
///     fn produce(matched: &Matched<StartInvoke>) -> Option<Self> {
///         Some(GoOff::new(matched.target, matched.message.targets.clone()))
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn side_effect(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: ItemImpl = syn::parse(item)
        .expect("#[side_effect] must be applied to an `impl SideEffect<M> for S` block");

    // Extract the self type (S — the side effect type)
    let self_ty = &parsed.self_ty;

    // Extract M from the trait path: SideEffect<M>
    let (_, trait_path, _) = parsed.trait_.as_ref()
        .expect("#[side_effect] must be on a trait impl (impl SideEffect<M> for S)");

    let last_segment = trait_path.segments.last()
        .expect("#[side_effect] could not parse trait path");

    let message_ty = match &last_segment.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            args.args.first()
                .expect("#[side_effect] SideEffect must have a type parameter")
                .clone()
        }
        _ => panic!("#[side_effect] expected SideEffect<M> with angle-bracketed type parameter"),
    };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::SideEffectInstaller {
                install: bevy_gearbox::registration::register_side_effect::<#message_ty, #self_ty>
            }
        }
    };
    TokenStream::from(expanded)
}
