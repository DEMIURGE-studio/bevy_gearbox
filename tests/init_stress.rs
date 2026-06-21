//! Tests that stress the *initialization* path specifically — matching the
//! scenario in the survivors game where many items are spawned from a
//! Startup system with queued commands, each getting their own state
//! machine rooted in an `InBackpack` initial state. The user's symptom is
//! that items entering `InBackpack` via the init path don't get their
//! `StateComponent<InBackpack>` marker on the machine root, while items
//! that go through a message-driven transition (EquipIt, UnequipIt) work.

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Component, Clone, Debug, PartialEq)]
struct InBag;

#[derive(Component, Clone, Debug, PartialEq)]
struct Equipped;

#[derive(Component, Clone)]
struct Item;

/// Baseline: spawn a single item from a startup-like system using nested
/// `with_children` commands. Does the `InBag` marker land on the item root
/// after one update?
#[test]
fn single_item_spawned_via_commands_gets_state_component() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<InBag>();

    app.add_systems(Startup, |mut commands: Commands| {
        let item = commands.spawn(Item).id();
        commands.entity(item).with_children(|parent| {
            let in_bag = parent
                .spawn((SubstateOf(item), StateComponent(InBag)))
                .id();
            parent
                .commands_mut()
                .entity(item)
                .init_state_machine(in_bag);
        });
    });

    app.update();

    // Find the item.
    let mut q = app.world_mut().query_filtered::<Entity, With<Item>>();
    let item = q.iter(app.world()).next().expect("should have spawned item");

    assert!(
        app.world().get::<InBag>(item).is_some(),
        "InBag marker should be on the item root after one update"
    );
}

/// Stress: spawn SEVEN items in one Startup system (matching survivors'
/// `spawn_starter_items`). Every item should end up with `InBag` on its
/// root entity.
#[test]
fn seven_items_spawned_same_frame_all_get_state_component() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<InBag>();

    app.add_systems(Startup, |mut commands: Commands| {
        for _ in 0..7 {
            let item = commands.spawn(Item).id();
            commands.entity(item).with_children(|parent| {
                let in_bag = parent
                    .spawn((SubstateOf(item), StateComponent(InBag)))
                    .id();
                parent
                    .commands_mut()
                    .entity(item)
                    .init_state_machine(in_bag);
            });
        }
    });

    app.update();

    let mut q = app.world_mut().query_filtered::<Entity, With<Item>>();
    let items: Vec<Entity> = q.iter(app.world()).collect();
    assert_eq!(items.len(), 7, "seven items should have been spawned");

    for (idx, item) in items.iter().enumerate() {
        assert!(
            app.world().get::<InBag>(*item).is_some(),
            "item {idx} ({item:?}) should have InBag marker on root"
        );
    }
}

/// Stress + message-driven transition mix: six items start in `InBag`, one
/// item gets an `EquipIt` message written in the same frame it was spawned.
/// All six bagged items should end with `InBag` on their root; the seventh
/// should end with `Equipped` on its root.
///
/// This mirrors survivors' `spawn_starter_items`: six items stay in the
/// bag and the starter wand is equipped immediately.
#[test]
fn six_items_stay_in_bag_one_gets_equipped() {
    #[derive(Message, Clone, Reflect)]
    struct EquipIt {
        item: Entity,
    }
    impl GearboxMessage for EquipIt {
        type Validator = AcceptAll;
        fn target(&self) -> Entity {
            self.item
        }
    }

    #[derive(Resource)]
    struct Wand(Entity);
    #[derive(Resource)]
    struct BagItems(Vec<Entity>);

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<InBag>();
    app.register_state_component::<Equipped>();
    app.register_transition::<EquipIt>();

    app.add_systems(Startup, |mut commands: Commands| {
        // Spawn the wand first and queue an EquipIt for it.
        let wand = commands.spawn(Item).id();
        commands.entity(wand).with_children(|parent| {
            let in_bag = parent
                .spawn((SubstateOf(wand), StateComponent(InBag)))
                .id();
            let equipped = parent
                .spawn((SubstateOf(wand), StateComponent(Equipped)))
                .id();
            parent.spawn_transition::<EquipIt>(in_bag, equipped);
            parent
                .commands_mut()
                .entity(wand)
                .init_state_machine(in_bag);
        });
        commands.insert_resource(Wand(wand));

        // Spawn six more items that just live in the bag.
        let mut bagged = Vec::new();
        for _ in 0..6 {
            let item = commands.spawn(Item).id();
            commands.entity(item).with_children(|parent| {
                let in_bag = parent
                    .spawn((SubstateOf(item), StateComponent(InBag)))
                    .id();
                parent
                    .commands_mut()
                    .entity(item)
                    .init_state_machine(in_bag);
            });
            bagged.push(item);
        }
        commands.insert_resource(BagItems(bagged));

        // Fire EquipIt for the wand. The message lands in the buffer before
        // the gearbox schedule runs this frame.
        commands.queue(|world: &mut World| {
            let wand = world.resource::<Wand>().0;
            world.write_message(EquipIt { item: wand });
        });
    });

    app.update();

    let wand = app.world().resource::<Wand>().0;
    let bagged: Vec<Entity> = app.world().resource::<BagItems>().0.clone();

    assert!(
        app.world().get::<Equipped>(wand).is_some(),
        "wand should have Equipped marker"
    );
    assert!(
        app.world().get::<InBag>(wand).is_none(),
        "wand should NOT still have InBag marker"
    );

    for (idx, item) in bagged.iter().enumerate() {
        assert!(
            app.world().get::<InBag>(*item).is_some(),
            "bagged item {idx} ({item:?}) should have InBag marker"
        );
    }
}
