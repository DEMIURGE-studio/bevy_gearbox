use bevy_gearbox_protocol::components as c;
use crate::model::StateMachineGraph;
use crate::types::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItemKind {
    MakeLeaf,
    MakeParent,
    MakeParallel,
    Rename,
    Save,
    SaveAs,
    SaveSubstates,
    Delete,
    MakeInitial { parent: EntityId },
    AddChild,
    AutoLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub kind: MenuItemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuSelection {
    MakeLeaf { target: EntityId },
    MakeParent { target: EntityId },
    MakeParallel { target: EntityId },
    RenameEntity { target: EntityId },
    SaveStateMachine { target: EntityId },
    SaveStateMachineAs { target: EntityId },
    SaveSubstates { target: EntityId },
    DeleteEntity { target: EntityId },
    MakeInitial { parent: EntityId, new_initial: EntityId },
    AddChildStateMachine { target: EntityId },
    AutoLayoutSubtree { target: EntityId },
}

/// Build context menu items for a right-clicked node using only the cached model.
/// No side effects; the returned items should be used to emit selection events.
pub fn build_context_menu(graph: &StateMachineGraph, id: EntityId) -> Vec<MenuItem> {
    let mut items: Vec<MenuItem> = Vec::new();

    if !graph.nodes.contains_key(&id) { return items; }
    let has_children = !graph.get_children(&id).is_empty();
    let has_initial_state = graph.has_component(&id, c::INITIAL_STATE);
    let is_parallel = has_children && !has_initial_state;
    // Root node detection: server does not include an explicit bevy_gearbox::StateMachine marker
    // in the graph snapshot, so treat the graph root as the state machine owner.
    let has_state_children_capability = graph.has_component(&id, c::STATE_CHILDREN);

    let parent_for_make_initial = graph
        .get_parent(&id)
        .filter(|pid| graph.initial_child(pid) != Some(id));

    // Make Leaf (only when there are children)
    if has_children {
        items.push(MenuItem { label: "Make Leaf", kind: MenuItemKind::MakeLeaf });
    }

    // Make Parent (when this node does not have InitialState)
    if !has_initial_state {
        items.push(MenuItem { label: "Make Parent", kind: MenuItemKind::MakeParent });
    }

    // Make Parallel (only when it has children and is not already parallel)
    if !is_parallel {
        items.push(MenuItem { label: "Make Parallel", kind: MenuItemKind::MakeParallel });
    }

    // Save: one click when the node already has a StateMachineId; Save As
    // prompts for one. Both write `assets/<id>.scn.ron` + `assets/<id>.sm.ron`
    // on the game side.
    if graph.machine_id(&id).is_some() {
        items.push(MenuItem { label: "Save", kind: MenuItemKind::Save });
    }
    items.push(MenuItem { label: "Save As\u{2026}", kind: MenuItemKind::SaveAs });

    // Save Substates: available if any descendant has a StateMachineId
    let mut has_descendant_with_id = false;
    if has_children {
        let mut stack: Vec<EntityId> = graph.get_children(&id);
        while let Some(cid) = stack.pop() {
            if graph.entity_data.get(&cid).map(|b| b.contains(c::STATE_MACHINE_ID)).unwrap_or(false) { has_descendant_with_id = true; break; }
            let kids = graph.get_children(&cid);
            if !kids.is_empty() { stack.extend(kids.into_iter()); }
        }
    }
    if has_descendant_with_id {
        items.push(MenuItem { label: "Save Substates", kind: MenuItemKind::SaveSubstates });
    }

    // Rename (always available; inserts/updates Name on write)
    items.push(MenuItem { label: "Rename", kind: MenuItemKind::Rename });

    // Delete (always)
    items.push(MenuItem { label: "Delete", kind: MenuItemKind::Delete });

    // Make Initial (when the node has a parent and is not already its initial child).
    // On a parallel parent this also makes the parent sequential.
    if let Some(parent) = parent_for_make_initial {
        items.push(MenuItem { label: "Make Initial", kind: MenuItemKind::MakeInitial { parent } });
    }

    // Add Child (when node has Substates capability)
    if has_state_children_capability {
        items.push(MenuItem { label: "Add Child", kind: MenuItemKind::AddChild });
    }

    // Auto-layout: always available. Lays out the subtree rooted at this node
    // (or this node's parent if it's a leaf — see auto_layout::auto_layout_subtree).
    // To lay out the entire document, right-click the root node.
    items.push(MenuItem { label: "Auto Layout", kind: MenuItemKind::AutoLayout });

    items
}


