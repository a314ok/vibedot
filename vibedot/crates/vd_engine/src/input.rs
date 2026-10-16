//! Input state and the action map (Godot's `Input` + `InputMap`).
//!
//! Keys are identified by name (`"A"`, `"Space"`, `"ArrowLeft"`, `"F1"`,
//! `"MouseLeft"`...) so the editor, the standalone player and project files
//! all agree without depending on a windowing library here.

use std::collections::{BTreeMap, HashMap, HashSet};

use vd_core::{StringName, Vec2};

#[derive(Clone, Debug, Default)]
pub struct InputState {
    down: HashSet<StringName>,
    pressed: HashSet<StringName>,
    released: HashSet<StringName>,
    /// Pointer position in viewport pixels.
    pub mouse_screen: Vec2,
    /// Ordered key/button presses this frame (for `On Key Pressed` events).
    pub press_order: Vec<StringName>,
    pub text: String,
}

impl InputState {
    /// Call at the start of each frame, before feeding new events.
    pub fn begin_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.press_order.clear();
        self.text.clear();
    }

    pub fn key_down(&mut self, key: &str) {
        let k = StringName::new(key);
        if self.down.insert(k) {
            self.pressed.insert(k);
            self.press_order.push(k);
        }
    }

    pub fn key_up(&mut self, key: &str) {
        let k = StringName::new(key);
        if self.down.remove(&k) {
            self.released.insert(k);
        }
    }

    /// Releases everything (e.g. when the window loses focus).
    pub fn release_all(&mut self) {
        for k in self.down.drain() {
            self.released.insert(k);
        }
    }

    pub fn is_key_down(&self, key: StringName) -> bool {
        self.down.contains(&key)
    }

    pub fn is_key_pressed(&self, key: StringName) -> bool {
        self.pressed.contains(&key)
    }

    pub fn is_key_released(&self, key: StringName) -> bool {
        self.released.contains(&key)
    }
}

/// Resolves actions to keys.
#[derive(Clone, Debug, Default)]
pub struct InputMap {
    actions: HashMap<StringName, Vec<StringName>>,
    /// Reverse map: key → actions it triggers.
    by_key: HashMap<StringName, Vec<StringName>>,
}

impl InputMap {
    pub fn new(map: &BTreeMap<String, Vec<String>>) -> Self {
        let mut s = Self::default();
        for (action, keys) in map {
            let a = StringName::new(action);
            let keys: Vec<StringName> = keys.iter().map(|k| StringName::new(k)).collect();
            for &k in &keys {
                s.by_key.entry(k).or_default().push(a);
            }
            s.actions.insert(a, keys);
        }
        s
    }

    pub fn has_action(&self, action: StringName) -> bool {
        self.actions.contains_key(&action)
    }

    /// `Input.is_action_pressed()`. Unknown actions fall back to a key of the
    /// same name, so blueprints can test `"Space"` directly.
    pub fn is_action_pressed(&self, input: &InputState, action: StringName) -> bool {
        match self.actions.get(&action) {
            Some(keys) => keys.iter().any(|&k| input.is_key_down(k)),
            None => input.is_key_down(action),
        }
    }

    pub fn is_action_just_pressed(&self, input: &InputState, action: StringName) -> bool {
        match self.actions.get(&action) {
            Some(keys) => {
                keys.iter().any(|&k| input.is_key_pressed(k)) && !keys.iter().any(|&k| input.is_key_down(k) && !input.is_key_pressed(k))
            }
            None => input.is_key_pressed(action),
        }
    }

    pub fn is_action_just_released(&self, input: &InputState, action: StringName) -> bool {
        match self.actions.get(&action) {
            Some(keys) => keys.iter().any(|&k| input.is_key_released(k)) && !keys.iter().any(|&k| input.is_key_down(k)),
            None => input.is_key_released(action),
        }
    }

    /// Actions triggered by `key`.
    pub fn actions_for_key(&self, key: StringName) -> &[StringName] {
        self.by_key.get(&key).map_or(&[], Vec::as_slice)
    }

    /// `Input.get_axis()`.
    pub fn get_axis(&self, input: &InputState, negative: StringName, positive: StringName) -> f32 {
        f32::from(u8::from(self.is_action_pressed(input, positive))) - f32::from(u8::from(self.is_action_pressed(input, negative)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::default_input_map;

    #[test]
    fn actions_map_to_keys() {
        let map = InputMap::new(&default_input_map());
        let mut input = InputState::default();
        let left = StringName::new("ui_left");
        let right = StringName::new("ui_right");
        input.key_down("A");
        assert!(map.is_action_pressed(&input, left));
        assert!(map.is_action_just_pressed(&input, left));
        assert_eq!(map.get_axis(&input, left, right), -1.0);
        input.begin_frame();
        assert!(map.is_action_pressed(&input, left));
        assert!(!map.is_action_just_pressed(&input, left));
        // Second key for the same action does not re-trigger "just pressed".
        input.key_down("ArrowLeft");
        assert!(!map.is_action_just_pressed(&input, left));
        input.begin_frame();
        input.key_up("A");
        assert!(!map.is_action_just_released(&input, left), "ArrowLeft still held");
        input.key_up("ArrowLeft");
        assert!(map.is_action_just_released(&input, left));
        // Raw key names work as actions.
        input.key_down("Q");
        assert!(map.is_action_pressed(&input, StringName::new("Q")));
        assert_eq!(map.actions_for_key(StringName::new("Space")).len(), 3);
    }
}
