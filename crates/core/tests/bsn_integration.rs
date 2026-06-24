//! Spike: can the new Bevy 0.19 BSN system express the entity hierarchies that
//! diesel currently builds imperatively with its `template_*` functions?
//!
//! Validated against the REAL gearbox components, these tests exercise the
//! mechanics the diesel→BSN mapping depends on:
//!
//!   1. State tree via the `SubstateOf`/`Substates` relationship block.
//!   2. `#Name` references resolving into entity-valued components
//!      (`InitialState`, `Target`).
//!   3. Transition entities via the `Source`/`Transitions` relationship block
//!      (the `Source` back-reference is auto-set by the block).
//!   4. Applying a scene onto a *pre-existing* entity — diesel templates take
//!      `entity: Option<Entity>` and build onto it.
//!   5. A full magic_missile-shaped chart authored in a single `bsn!` scope.
//!   6. Cross-scope references threaded through scene-function `EntityTemplate`
//!      parameters — the composition seam (BSN name scopes are per-`bsn!`).
//!
//! Modeled on `bevy/examples/scene/bsn.rs`.
//!
//! Spike prerequisites (added to gearbox components for this to compile):
//!   * `StateMachine`: `+ Clone`        (bsn! bare components need Default+Clone)
//!   * `AlwaysEdge`:   `+ Default, Clone`
//!   * `Target`, `InitialState`: `+ FromTemplate` (to accept `#Name` refs)
//! `MessageEdge<M>` would likewise need `+ Clone` to be used directly in bsn!;
//! these tests use `AlwaysEdge` because the edge structure (Source/Target/marker)
//! is identical regardless of the trigger marker.

use bevy::app::TaskPoolPlugin;
use bevy::asset::AssetPlugin;
use bevy::ecs::template::EntityTemplate;
use bevy::prelude::*;
use bevy::scene::prelude::{bsn, EntityWorldMutSceneExt, Scene, WorldSceneExt};
use bevy::scene::ScenePlugin;

use bevy_gearbox_core::{
    AlwaysEdge, EdgeKind, InitialState, Source, StateMachine, SubstateOf, Substates, Target,
    Transitions,
};

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), ScenePlugin));
    app
}

/// Find a direct substate of `parent` whose `Name` matches.
fn substate_named(world: &World, parent: Entity, name: &str) -> Entity {
    world
        .entity(parent)
        .get::<Substates>()
        .expect("parent has Substates")
        .into_iter()
        .copied()
        .find(|&e| {
            world
                .entity(e)
                .get::<Name>()
                .is_some_and(|n| n.as_str() == name)
        })
        .unwrap_or_else(|| panic!("no substate named {name}"))
}

fn collect<R>(world: &World, parent: Entity) -> Vec<Entity>
where
    R: bevy::ecs::component::Component,
    for<'a> &'a R: IntoIterator<Item = &'a Entity>,
{
    world
        .entity(parent)
        .get::<R>()
        .map(|r| r.into_iter().copied().collect())
        .unwrap_or_default()
}

// 1. State tree via the SubstateOf/Substates relationship block.
#[test]
fn substates_block_builds_tree_with_backrefs() {
    let mut app = test_app();
    let world = app.world_mut();

    let root = world
        .spawn_scene(bsn! {
            StateMachine
            Substates [ #Ready, #Invoking, #Cooldown ]
        })
        .unwrap()
        .id();

    let subs = collect::<Substates>(world, root);
    assert_eq!(subs.len(), 3, "three substates spawned");
    for child in subs {
        assert_eq!(
            world.entity(child).get::<SubstateOf>().unwrap().0,
            root,
            "block auto-set the SubstateOf back-reference"
        );
    }
}

// 2 + 3. #Name resolves into InitialState/Target; Transitions block auto-sets Source.
#[test]
fn name_refs_resolve_initial_state_and_transition_target() {
    let mut app = test_app();
    let world = app.world_mut();

    let root = world
        .spawn_scene(bsn! {
            StateMachine InitialState(#Ready)
            Substates [
                #Ready Transitions [ (Target(#Invoking) AlwaysEdge) ],
                #Invoking,
            ]
        })
        .unwrap()
        .id();

    // InitialState(#Ready) resolved to the Ready child entity.
    let initial = world.entity(root).get::<InitialState>().unwrap().0;
    assert_eq!(world.entity(initial).get::<Name>().unwrap().as_str(), "Ready");

    // The transition nested under #Ready has Source auto-set to Ready and
    // Target resolved to the #Invoking sibling.
    let edges = collect::<Transitions>(world, initial);
    assert_eq!(edges.len(), 1);
    let edge = world.entity(edges[0]);
    assert_eq!(edge.get::<Source>().unwrap().0, initial, "Source back-ref");
    assert!(edge.contains::<AlwaysEdge>());
    let target = edge.get::<Target>().unwrap().0;
    assert_eq!(world.entity(target).get::<Name>().unwrap().as_str(), "Invoking");
}

// 4. Apply a scene onto a pre-existing entity (diesel's `entity: Option<Entity>`).
#[test]
fn apply_scene_onto_existing_entity() {
    let mut app = test_app();
    let world = app.world_mut();

    let entity = world.spawn_empty().id();
    world
        .entity_mut(entity)
        .apply_scene(bsn! {
            StateMachine
            Substates [ #Only ]
        })
        .unwrap();

    assert!(world.entity(entity).contains::<StateMachine>());
    let subs = collect::<Substates>(world, entity);
    assert_eq!(subs.len(), 1);
    assert_eq!(world.entity(subs[0]).get::<SubstateOf>().unwrap().0, entity);
}

// 5. Full magic_missile-shaped chart in a single bsn! scope.
//    Ability → {Ready, Invoking → Repeater → {Idle, Fire}, Cooldown}
#[test]
fn full_invoked_chart_single_scope() {
    let mut app = test_app();
    let world = app.world_mut();

    let root = world
        .spawn_scene(bsn! {
            StateMachine InitialState(#Ready)
            Substates [
                #Ready Transitions [ (Target(#Invoking) AlwaysEdge) ],
                #Invoking InitialState(#Repeater)
                          Transitions [ (Target(#Cooldown) AlwaysEdge) ]
                          Substates [
                    #Repeater InitialState(#Idle) Substates [
                        #Idle Transitions [ (Target(#Fire) AlwaysEdge) ],
                        #Fire Transitions [ (Target(#Repeater) AlwaysEdge) ],
                    ],
                ],
                #Cooldown Transitions [ (Target(#Ready) AlwaysEdge) ],
            ]
        })
        .unwrap()
        .id();

    // Top level
    assert_eq!(collect::<Substates>(world, root).len(), 3);
    let invoking = substate_named(world, root, "Invoking");
    let cooldown = substate_named(world, root, "Cooldown");

    // Invoking → Repeater → {Idle, Fire}
    let repeater = world.entity(invoking).get::<InitialState>().unwrap().0;
    assert_eq!(world.entity(repeater).get::<Name>().unwrap().as_str(), "Repeater");
    assert_eq!(collect::<Substates>(world, repeater).len(), 2);
    let idle = substate_named(world, repeater, "Idle");
    let fire = substate_named(world, repeater, "Fire");

    // The Fire → Repeater bounce target is the ancestor Repeater entity
    // (an ancestor reference resolved within the same scope).
    let fire_edge = collect::<Transitions>(world, fire)[0];
    assert_eq!(world.entity(fire_edge).get::<Target>().unwrap().0, repeater);

    // The Idle → Fire edge.
    let idle_edge = collect::<Transitions>(world, idle)[0];
    assert_eq!(world.entity(idle_edge).get::<Target>().unwrap().0, fire);

    // Invoking → Cooldown.
    let inv_edge = collect::<Transitions>(world, invoking)[0];
    assert_eq!(world.entity(inv_edge).get::<Target>().unwrap().0, cooldown);
}

// 5b. EdgeKind::Internal lowers bare in a transition tuple (FromTemplate derive
//     on the enum). A self-internal edge keeps the source active rather than
//     exiting/re-entering — the structure here just asserts it resolves.
#[test]
fn edge_kind_internal_lowers_bare() {
    let mut app = test_app();
    let world = app.world_mut();

    let root = world
        .spawn_scene(bsn! {
            StateMachine
            Substates [
                #Busy Transitions [ (Target(#Busy) AlwaysEdge EdgeKind::Internal) ],
            ]
        })
        .unwrap()
        .id();

    let busy = substate_named(world, root, "Busy");
    let edge = collect::<Transitions>(world, busy)[0];
    assert!(
        matches!(world.entity(edge).get::<EdgeKind>(), Some(EdgeKind::Internal)),
        "EdgeKind::Internal resolved onto the edge"
    );
}

// 6. Cross-scope references: BSN name scopes are per-bsn!, so a sub-scene cannot
//    see the caller's `#Name`s directly. References are threaded as `EntityTemplate`
//    parameters — this is the seam diesel's composition helpers would use.
#[test]
fn cross_scope_reference_via_entity_template_param() {
    let mut app = test_app();
    let world = app.world_mut();

    // A reusable edge sub-scene whose target is supplied by the caller.
    fn edge_to(target: EntityTemplate) -> impl Scene {
        bsn! { Target(target) AlwaysEdge }
    }

    let root = world
        .spawn_scene(bsn! {
            StateMachine
            Substates [ #Home, #Away ]
            // `#Home` is resolved in THIS scope and threaded into edge_to's scope.
            Transitions [ edge_to(#Home) ]
        })
        .unwrap()
        .id();

    let home = substate_named(world, root, "Home");
    let edges = collect::<Transitions>(world, root);
    assert_eq!(edges.len(), 1);
    let edge = world.entity(edges[0]);
    assert_eq!(edge.get::<Source>().unwrap().0, root, "edge Source = root");
    assert_eq!(
        edge.get::<Target>().unwrap().0,
        home,
        "caller's #Home reference threaded across the scene-function boundary"
    );
}
