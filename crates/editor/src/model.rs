use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::types::EntityId;
use bevy_gearbox_protocol::components as c;

/// Stable Rust type path of a component (e.g. "bevy_ecs::name::Name").
pub(crate) type TypePathString = String;

/// Tracks structural and data changes in the editor model.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DirtyFlags {
    /// True if the set of components or their values changed.
    pub(crate) components: bool,
    /// True if hierarchy or connections changed.
    pub(crate) structure: bool,
}

/// Per-component entry stored in a `ComponentBag`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ComponentEntry {
    /// Stable type path for this component.
    pub(crate) type_path: TypePathString,
    /// Component value as JSON; may be round-tripped even when unknown to the client.
    pub(crate) value_json: JsonValue,
    /// Dirty if value differs from last successful server state.
    pub(crate) dirty: bool,
    /// Optional validation errors for UI presentation.
    pub(crate) validation_errors: Vec<String>,
    /// Opaque server version/epoch for conflict detection (if available).
    pub(crate) server_version: Option<u64>,
}

impl ComponentEntry {
    pub(crate) fn new(type_path: impl Into<TypePathString>, value_json: JsonValue) -> Self {
        Self {
            type_path: type_path.into(),
            value_json,
            dirty: false,
            validation_errors: Vec::new(),
            server_version: None,
        }
    }
}

/// Bag of components for a node or edge, keyed by type path.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ComponentBag {
    pub(crate) entries: HashMap<TypePathString, ComponentEntry>,
}

impl ComponentBag {
    pub(crate) fn contains(&self, type_path: &str) -> bool { self.entries.contains_key(type_path) }
    pub(crate) fn get(&self, type_path: &str) -> Option<&ComponentEntry> { self.entries.get(type_path) }
    pub(crate) fn get_mut(&mut self, type_path: &str) -> Option<&mut ComponentEntry> { self.entries.get_mut(type_path) }
    pub(crate) fn insert(&mut self, entry: ComponentEntry) { self.entries.insert(entry.type_path.clone(), entry); }
    pub(crate) fn remove(&mut self, type_path: &str) -> Option<ComponentEntry> { self.entries.remove(type_path) }
    pub(crate) fn keys(&self) -> impl Iterator<Item=&str> { self.entries.keys().map(|s| s.as_str()) }
}

/// A state in the machine hierarchy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StateNode {
    pub(crate) id: EntityId,
    /// Dirty tracking for this node.
    pub(crate) dirty: DirtyFlags,
    /// Opaque server version for the node entity (not per-component).
    pub(crate) server_version: Option<u64>,
}

impl StateNode {
    pub(crate) fn new(id: EntityId) -> Self {
        Self {
            id,
            dirty: DirtyFlags::default(),
            server_version: None,
        }
    }
}

impl Default for StateNode {
    fn default() -> Self {
        Self::new(EntityId(0))
    }
}

/// A transition edge between states.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Edge {
    pub(crate) id: EntityId,
    pub(crate) source: EntityId,
    pub(crate) target: EntityId,
    pub(crate) components: ComponentBag,
    /// Derived label for display (e.g., event name), not authoritative.
    pub(crate) display_label: Option<String>,
    pub(crate) dirty: DirtyFlags,
    pub(crate) server_version: Option<u64>,
}

impl Edge {
    pub(crate) fn new(id: EntityId, source: EntityId, target: EntityId) -> Self {
        Self {
            id,
            source,
            target,
            components: ComponentBag::default(),
            display_label: None,
            dirty: DirtyFlags::default(),
            server_version: None,
        }
    }
}

/// Graph container with adjacency indices for efficient queries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StateMachineGraph {
    /// The root state of the machine. The root is also present in `nodes`.
    pub(crate) root: EntityId,
    pub(crate) nodes: HashMap<EntityId, StateNode>,
    pub(crate) edges: HashMap<EntityId, Edge>,
    /// Cached adjacency for quick traversal; rebuild or keep in sync on edits.
    pub(crate) adjacency_out: HashMap<EntityId, Vec<EntityId>>,
    pub(crate) adjacency_in: HashMap<EntityId, Vec<EntityId>>,
    /// Central, per-entity component store kept in sync via snapshots and watches.
    pub(crate) entity_data: HashMap<EntityId, ComponentBag>,
    /// Current active state set derived from StateMachine.active (doc-local cache)
    pub(crate) active_nodes: std::collections::HashSet<EntityId>,
}

impl StateMachineGraph {
    /// Create a new graph with the provided root state. The root is inserted into `nodes`.
    pub(crate) fn new(root: StateNode) -> Self {
        let root_id = root.id;
        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);
        Self {
            root: root_id,
            nodes,
            edges: HashMap::new(),
            adjacency_out: HashMap::new(),
            adjacency_in: HashMap::new(),
            entity_data: HashMap::new(),
            active_nodes: std::collections::HashSet::new(),
        }
    }

    /// Returns the component bag for an entity if present in the central store.
    pub(crate) fn component_bag(&self, id: &EntityId) -> Option<&ComponentBag> {
        self.entity_data.get(id)
    }

    /// True if the entity has a component with the given type path.
    pub(crate) fn has_component(&self, id: &EntityId, type_path: &str) -> bool {
        self.entity_data.get(id).map_or(false, |b| b.contains(type_path))
    }

    /// Returns a display label for either a state or an edge entity, derived from its components.
    /// Order of precedence:
    /// 1) Name text (if present)
    /// 2) EventEdge<T> → T simple name, or "Always" for AlwaysEdge
    /// 3) Fallback: the numeric entity id
    pub(crate) fn get_label_for(&self, id: &EntityId) -> String {
        if let Some(bag) = self.entity_data.get(id) {
            if let Some(name) = extract_name_from_bag(bag) { return name; }
            let edge_label = choose_edge_label_bag(bag);
            return edge_label;
        }
        format!("{}", id.0)
    }

    /// Returns the display name for a state entity (same precedence as get_label_for for now).
    pub(crate) fn get_display_name(&self, id: &EntityId) -> String { self.get_label_for(id) }

    /// Children derived from the per-entity component store (STATE_CHILDREN as array of ids).
    pub(crate) fn get_children(&self, id: &EntityId) -> Vec<EntityId> {
        let mut out: Vec<EntityId> = Vec::new();
        if let Some(bag) = self.entity_data.get(id) {
            if let Some(entry) = bag.get(c::STATE_CHILDREN) {
                if let serde_json::Value::Array(arr) = &entry.value_json {
                    for v in arr.iter() {
                        if let Some(s) = v.as_str() {
                            if let Ok(u) = s.parse::<u64>() { out.push(EntityId(u)); }
                        }
                    }
                }
            }
        }
        out
    }

    /// Parent derived by scanning STATE_CHILDREN of all nodes; O(N) but acceptable for editor.
    pub(crate) fn get_parent(&self, id: &EntityId) -> Option<EntityId> {
        for (candidate, _node) in self.nodes.iter() {
            if self.get_children(candidate).contains(id) { return Some(*candidate); }
        }
        None
    }

    /// True if the entity currently has the Active component (from live component store).
    pub(crate) fn is_active(&self, id: &EntityId) -> bool {
        self.active_nodes.contains(id)
    }

    /// Replace the current active set with the provided entities.
    pub(crate) fn set_active<I: IntoIterator<Item = EntityId>>(&mut self, ids: I) {
        self.active_nodes.clear();
        self.active_nodes.extend(ids);
    }
}

impl fmt::Display for StateMachineGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Collect names for nodes lazily
        let mut names: HashMap<EntityId, String> = HashMap::new();

        // Helper to extract a displayable name from a node's components
        let mut get_node_name = |id: &EntityId| -> String {
            if let Some(n) = names.get(id) { return n.clone(); }
            let name = self.get_display_name(id);
            names.insert(*id, name.clone());
            name
        };

        // Traverse states from root using children relationships, tracking depth
        let mut ordered: Vec<(EntityId, usize)> = Vec::new();
        let mut stack: Vec<(EntityId, usize)> = Vec::new();
        let mut visited: HashSet<EntityId> = HashSet::new();
        if self.nodes.contains_key(&self.root) { stack.push((self.root, 0)); }
        while let Some((id, depth)) = stack.pop() {
            if !visited.insert(id) { continue; }
            ordered.push((id, depth));
            // Push children in reverse to preserve original order on pop
            let mut kids = self.get_children(&id);
            kids.reverse();
            for child in kids.into_iter() { stack.push((child, depth + 1)); }
        }

        // Build edges list in a stable order (by source appearance in states)
        let mut edges_formatted: Vec<String> = Vec::new();
        for (state, _depth) in &ordered {
            // Prefer adjacency if present; otherwise scan all edges
            if let Some(out_ids) = self.adjacency_out.get(state) {
                for e_id in out_ids {
                    if let Some(edge) = self.edges.get(e_id) {
                        let source_name = get_node_name(&edge.source);
                        let mut target_name = get_node_name(&edge.target);
                        if target_name.is_empty() {
                            if let Some(s) = self.component_bag(&edge.id).and_then(|b| extract_name_from_bag(b)) {
                                if let Some(arrow) = s.find("->") {
                                    let rhs = s[arrow+2..].trim();
                                    let rhs = if let Some(paren) = rhs.find('(') { &rhs[..paren] } else { rhs };
                                    let rhs = rhs.trim();
                                    if !rhs.is_empty() { target_name = rhs.to_string(); }
                                }
                            }
                        }
                        let label = self.component_bag(&edge.id).map(choose_edge_label_bag).unwrap_or_else(|| "Edge".to_string());
                        edges_formatted.push(format!("{} - {} -> {}", source_name, label, target_name));
                    }
                }
            } else {
                for edge in self.edges.values() {
                    if &edge.source == state {
                        let source_name = get_node_name(&edge.source);
                        let mut target_name = get_node_name(&edge.target);
                        if target_name.is_empty() {
                            if let Some(s) = self.component_bag(&edge.id).and_then(|b| extract_name_from_bag(b)) {
                                if let Some(arrow) = s.find("->") {
                                    let rhs = s[arrow+2..].trim();
                                    let rhs = if let Some(paren) = rhs.find('(') { &rhs[..paren] } else { rhs };
                                    let rhs = rhs.trim();
                                    if !rhs.is_empty() { target_name = rhs.to_string(); }
                                }
                            }
                        }
                        let label = self.component_bag(&edge.id).map(choose_edge_label_bag).unwrap_or_else(|| "Edge".to_string());
                        edges_formatted.push(format!("{} - {} -> {}", source_name, label, target_name));
                    }
                }
            }
        }

        // Header: root node name
        let header = get_node_name(&self.root);
        writeln!(f, "{}", header)?;
        // Print states excluding the root; indent by depth (3 spaces per level)
        for (id, depth) in ordered.iter().skip(1) {
            let name = get_node_name(id);
            for _ in 0..*depth { write!(f, "   ")?; }
            writeln!(f, "{}", name)?;
        }
        writeln!(f)?;
        for line in edges_formatted {
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

fn extract_name_from_bag(bag: &ComponentBag) -> Option<String> {
    let val = bag.entries.get(c::NAME)?.value_json.clone();
    if let Some(s) = val.as_str() { return Some(s.to_string()); }
    if let JsonValue::Object(obj) = val {
        for v in obj.values() { if let Some(s) = v.as_str() { return Some(s.to_string()); } }
    }
    None
}

pub(crate) fn choose_edge_label_bag(bag: &ComponentBag) -> String {
    // 1) Prefer explicit Name text if present
    if let Some(name_val) = bag.entries.get(c::NAME).map(|e| e.value_json.clone()) {
        if let Some(s) = name_val.as_str() {
            let text = s.trim();
            if !text.is_empty() { return text.to_string(); }
        } else if let JsonValue::Object(obj) = name_val {
            for v in obj.values() {
                if let Some(s) = v.as_str() {
                    let text = s.trim();
                    if !text.is_empty() { return text.to_string(); }
                }
            }
        }
    }

    // 2) Otherwise, prefer EventEdge<T> → use inner T (simple name)
    let keys: HashSet<String> = bag.entries.keys().cloned().collect();
    let mut event_edge_types: Vec<&String> = keys.iter().filter(|s| s.contains(c::EVENT_EDGE_SUBSTR)).collect();
    event_edge_types.sort();
    if let Some(ty) = event_edge_types.first() {
        let s = ty.as_str();
        if let (Some(start), Some(end)) = (s.find('<'), s.rfind('>')) {
            if end > start + 1 {
                let inner = &s[start + 1..end];
                if let Some(simple) = inner.rsplit("::").next() { return simple.to_string(); }
                return inner.to_string();
            }
        }
        if let Some(simple) = s.rsplit("::").next() { return simple.to_string(); }
        return (*ty).clone();
    }

    // 3) Else, if AlwaysEdge present, use "Always"
    if keys.contains(c::ALWAYS_EDGE) { return "Always".to_string(); }

    // Fallback
    "Edge".to_string()
}



