//! Proc-macro entry points for `bevy_gearbox`.
//!
//! The generated code refers to gearbox through whatever name the calling
//! crate uses for the `bevy_gearbox` dependency. The logic itself lives in
//! `bevy_gearbox_macros_impl`; crates that re-export gearbox can wrap that
//! crate with their own root path so their users never need a direct
//! `bevy_gearbox` dependency.

use proc_macro::TokenStream;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;

/// Path to the `bevy_gearbox` crate as seen from the calling crate.
fn gearbox_root() -> proc_macro2::TokenStream {
    match crate_name("bevy_gearbox") {
        Ok(FoundCrate::Itself) => quote! { ::bevy_gearbox },
        Ok(FoundCrate::Name(name)) => {
            let ident = proc_macro2::Ident::new(&name, proc_macro2::Span::call_site());
            quote! { ::#ident }
        }
        Err(_) => quote! { ::bevy_gearbox },
    }
}

/// Derive macro that implements `GearboxMessage` for a message struct and
/// registers it with `GearboxPlugin` via `inventory`.
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
    bevy_gearbox_macros_impl::derive_gearbox_message(input.into(), gearbox_root()).into()
}

/// Attribute macro that registers a type for use in `StateComponent<T>` /
/// `StateInactiveComponent<T>` via `inventory`.
///
/// # Example
///
/// ```ignore
/// #[state_component]
/// #[derive(Component, Clone)]
/// struct Walking;
/// ```
#[proc_macro_attribute]
pub fn state_component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bevy_gearbox_macros_impl::state_component(item.into(), gearbox_root()).into()
}

/// Attribute macro that registers a Bevy `States` type to be driven by a
/// gearbox state carrying it as a component, via `inventory`.
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
    bevy_gearbox_macros_impl::state_bridge(item.into(), gearbox_root()).into()
}
