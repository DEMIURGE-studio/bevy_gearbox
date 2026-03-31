use bevy::prelude::*;
use std::collections::HashMap;
use crate::types::EntityId;
use super::view_model::GraphDoc;

#[derive(Debug, Default, Resource)]
pub struct Docs {
    pub map: HashMap<EntityId, GraphDoc>,
}


