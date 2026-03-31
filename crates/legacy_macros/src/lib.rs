use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Item, Meta, Expr, Path};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::Token;

/// Derive macro for simple events that don't need phase-specific payloads.
/// 
/// This macro implements `TransitionEvent` for simple events by setting all
/// associated types to `NoEvent` and returning `None` for all phase methods.
/// 
/// # Example
///  
/// ```rust
/// use bevy::prelude::*;
/// use bevy_gearbox_macros::SimpleTransition;
/// 
/// #[derive(Event, Clone, SimpleTransition)]
/// struct MySimpleEvent;
/// ```
/// 
/// This is equivalent to manually implementing:
/// 
/// ```rust
/// impl TransitionEvent for MySimpleEvent {
///     type ExitEvent = NoEvent;
///     type EdgeEvent = NoEvent;
///     type EntryEvent = NoEvent;
///     
///     fn to_exit_event(&self, _exiting: Entity, _entering: Entity, _edge: Entity) -> Option<Self::ExitEvent> { None }
///     fn to_edge_event(&self, _edge: Entity) -> Option<Self::EdgeEvent> { None }
///     fn to_entry_event(&self, _entering: Entity, _exiting: Entity, _edge: Entity) -> Option<Self::EntryEvent> { None }
/// }
/// ```
#[proc_macro_derive(SimpleTransition)]
pub fn derive_simple_transition(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    
    let expanded = quote! {
        impl bevy_gearbox::TransitionEvent for #name {
            type ExitEvent = bevy_gearbox::NoEvent;
            type EdgeEvent = bevy_gearbox::NoEvent;
            type EntryEvent = bevy_gearbox::NoEvent;
            type Validator = bevy_gearbox::AcceptAll;
            
            fn to_exit_event(&self, _exiting: bevy::prelude::Entity, _entering: bevy::prelude::Entity, _edge: bevy::prelude::Entity) -> Option<Self::ExitEvent> { None }
            fn to_edge_event(&self, _edge: bevy::prelude::Entity) -> Option<Self::EdgeEvent> { None }
            fn to_entry_event(&self, _entering: bevy::prelude::Entity, _exiting: bevy::prelude::Entity, _edge: bevy::prelude::Entity) -> Option<Self::EntryEvent> { None }
        }

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::TransitionInstaller { install: bevy_gearbox::registration::register_transition::<#name> }
        }
    };
    
    TokenStream::from(expanded)
}

/// Attribute macro variant to auto-register a state component type `T`.
///
/// Usage:
/// ```rust
/// #[register_state_component]
/// #[derive(Component, Reflect, FromReflect, Clone)]
/// struct MyFlag;
/// ```
#[proc_macro_attribute]
pub fn state_component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut parsed: Item = syn::parse(item.clone()).expect("#[register_state_component] must be applied to a type item");

    let name_ident = match &mut parsed {
        Item::Struct(s) => { s.ident.clone() }
        Item::Enum(e) => { e.ident.clone() }
        _ => panic!("#[bevy_state_bridge] supports only structs or enums"),
    };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::StateInstaller { install: bevy_gearbox::registration::register_state_component::<#name_ident> }
        }
    };
    TokenStream::from(expanded)
}

/// Apply to the event type definition. It implements the marker and submits an installer.
#[proc_macro_attribute]
pub fn transition_event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: Item = syn::parse(item.clone()).expect("#[register_transition] must be applied to a type item");
    let name = match &parsed {
        Item::Struct(s) => &s.ident,
        Item::Enum(e) => &e.ident,
        _ => panic!("#[register_transition] supports only structs or enums"),
    };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::TransitionInstaller { install: bevy_gearbox::registration::register_transition::<#name> }
        }
    };
    TokenStream::from(expanded)
}

/// Attribute on a parameter marker that wires guards and optionally the sync binding.
/// Usage examples:
///   #[gearbox_param(kind = "bool", source = Hitpoints)]
#[proc_macro_attribute]
pub fn gearbox_param(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse(attr)
        .expect("failed to parse #[gearbox_param] arguments");
    let parsed: Item = syn::parse(item.clone()).expect("#[gearbox_param] must be applied to a type item");
    let name = match &parsed {
        Item::Struct(s) => &s.ident,
        Item::Enum(e) => &e.ident,
        _ => panic!("#[gearbox_param] supports only structs or enums"),
    };

    // Parse args: kind = "bool"|"int"|"float", optional source = Type
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
            // No bool binding registration function in the new API
            quote!{}
        }
    } else {
        quote!{}
    };

    let binding_installer = if let Some(_) = source_path {
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
            // Skip bool bindings to match the available registration methods
            quote!{}
        }
    } else { quote!{} };

    let guard_installer_ty = if kind == "bool" {
        quote! { bevy_gearbox::registration::BoolParamInstaller }
    } else if kind == "int" { quote! { bevy_gearbox::registration::IntParamInstaller } }
    else { quote! { bevy_gearbox::registration::FloatParamInstaller } };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            #guard_installer_ty { install: #guard_install }
        }

        #binding_installer
    };
    TokenStream::from(expanded)
}

/// Attribute macro variant to auto-register a Bevy `States` bridge and inject derives.
/// Ensures `#[derive(States)]`, `#[derive(Component)]`, `#[derive(Default)]`, and `#[derive(Clone)]` exist.
#[proc_macro_attribute]
pub fn state_bridge(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut parsed: Item = syn::parse(item.clone()).expect("#[bevy_state_bridge] must be applied to a type item");

    let name_ident = match &mut parsed {
        Item::Struct(s) => { s.ident.clone() }
        Item::Enum(e) => { e.ident.clone() }
        _ => panic!("#[bevy_state_bridge] supports only structs or enums"),
    };

    let expanded = quote! {
        #parsed

        bevy_gearbox::inventory::submit! {
            bevy_gearbox::registration::StateBridgeInstaller { install: bevy_gearbox::registration::register_state_bridge::<#name_ident> }
        }
    };
    TokenStream::from(expanded)
}
