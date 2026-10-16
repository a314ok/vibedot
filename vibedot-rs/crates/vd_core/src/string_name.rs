//! Interned strings, the Rust counterpart of Godot's `StringName`.
//!
//! A `StringName` is a 4-byte `Copy` id. Comparing two names is an integer
//! compare, hashing is trivial - perfect for property names, group names,
//! input actions and blueprint event names which are compared every frame.
//!
//! Interned strings are leaked on purpose: the set of distinct names in a game
//! is small and bounded, and leaking gives us `&'static str` without lifetimes
//! leaking into every API.

use std::collections::HashMap;
use std::fmt;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StringName(u32);

struct Interner {
    map: HashMap<&'static str, u32>,
    strings: Vec<&'static str>,
}

fn interner() -> &'static RwLock<Interner> {
    static INTERNER: OnceLock<RwLock<Interner>> = OnceLock::new();
    INTERNER.get_or_init(|| {
        let mut i = Interner { map: HashMap::new(), strings: Vec::new() };
        // Index 0 is always the empty name so `StringName::default()` is cheap.
        i.map.insert("", 0);
        i.strings.push("");
        RwLock::new(i)
    })
}

impl StringName {
    pub const EMPTY: StringName = StringName(0);

    pub fn new(s: &str) -> Self {
        // Fast path: shared read lock.
        if let Some(&id) = interner().read().expect("interner poisoned").map.get(s) {
            return StringName(id);
        }
        let mut w = interner().write().expect("interner poisoned");
        // Re-check: another thread may have inserted while we waited.
        if let Some(&id) = w.map.get(s) {
            return StringName(id);
        }
        let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
        let id = u32::try_from(w.strings.len()).expect("too many interned strings");
        w.strings.push(leaked);
        w.map.insert(leaked, id);
        StringName(id)
    }

    pub fn as_str(self) -> &'static str {
        interner().read().expect("interner poisoned").strings[self.0 as usize]
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn id(self) -> u32 {
        self.0
    }
}

impl Default for StringName {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl From<&str> for StringName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<&String> for StringName {
    fn from(s: &String) -> Self {
        Self::new(s)
    }
}

impl fmt::Debug for StringName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "&{:?}", self.as_str())
    }
}

impl fmt::Display for StringName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for StringName {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StringName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(StringName::new(&s))
    }
}

/// Shorthand: `sname!("player")`.
#[macro_export]
macro_rules! sname {
    ($s:expr) => {
        $crate::StringName::new($s)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_stable() {
        let a = StringName::new("player");
        let b = StringName::new(&String::from("player"));
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "player");
        assert_ne!(a, StringName::new("enemy"));
        assert!(StringName::default().is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let a = StringName::new("jump");
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, "\"jump\"");
        let b: StringName = serde_json::from_str(&json).unwrap();
        assert_eq!(a, b);
    }
}
