// Save / load — in-memory slot store; serializes essential world state.

use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Resource, Debug, Default)]
pub struct SaveSlots(pub HashMap<String, Value>);

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveSlots>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_slots_default_empty() {
        let s = SaveSlots::default();
        assert!(s.0.is_empty());
    }

    #[test]
    fn save_slots_insert_lookup() {
        let mut s = SaveSlots::default();
        s.0.insert("slot1".into(), serde_json::json!({"hello": "world"}));
        assert_eq!(s.0["slot1"]["hello"].as_str(), Some("world"));
    }
}
