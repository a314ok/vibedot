//! The editable blueprint document (`.vbp`, JSON).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use vd_core::{Color, VType, Variant, Vec2};

use crate::defs::{self, NodeDef, PinKind};

pub const BLUEPRINT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BpNodeId(pub u32);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BpNode {
    pub id: BpNodeId,
    /// Library id, e.g. `"math.add"`.
    pub op: String,
    pub pos: Vec2,
    /// Literal values for unconnected data inputs, keyed by pin name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Variant>,
}

impl BpNode {
    pub fn def(&self) -> Option<&'static NodeDef> {
        defs::find(&self.op)
    }

    /// Literal value of an input: explicit value, else the pin default.
    pub fn literal(&self, pin: &str) -> Variant {
        if let Some(v) = self.values.get(pin) {
            return v.clone();
        }
        self.def().and_then(|d| d.input(pin)).map(|p| p.default.to_variant()).unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub node: BpNodeId,
    pub pin: String,
}

impl PinRef {
    pub fn new(node: BpNodeId, pin: &str) -> Self {
        Self { node, pin: pin.to_owned() }
    }
}

/// Directed connection: `from` is always an output pin, `to` an input pin.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Link {
    pub from: PinRef,
    pub to: PinRef,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VarDef {
    pub name: String,
    pub ty: VType,
    pub default: Variant,
    /// Shown in the node inspector so level designers can tweak per instance.
    #[serde(default)]
    pub exposed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub pos: Vec2,
    pub size: Vec2,
    pub text: String,
    pub color: Color,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Blueprint {
    pub format: u32,
    #[serde(default)]
    pub variables: Vec<VarDef>,
    pub nodes: Vec<BpNode>,
    pub links: Vec<Link>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    next_id: u32,
}

impl Default for Blueprint {
    fn default() -> Self {
        Self {
            format: BLUEPRINT_FORMAT_VERSION,
            variables: Vec::new(),
            nodes: Vec::new(),
            links: Vec::new(),
            comments: Vec::new(),
            next_id: 1,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("unknown node or pin")]
    NoSuchPin,
    #[error("connect an output to an input")]
    Direction,
    #[error("execution pins can only connect to execution pins")]
    ExecMismatch,
    #[error("type mismatch: {from} → {to}")]
    Type { from: &'static str, to: &'static str },
    #[error("this input only accepts a typed-in value")]
    LiteralOnly,
    #[error("a node cannot connect to itself")]
    SelfLink,
}

#[derive(Debug, thiserror::Error)]
pub enum BlueprintIoError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("blueprint parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

impl Blueprint {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node(&self, id: BpNodeId) -> Option<&BpNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: BpNodeId) -> Option<&mut BpNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn add_node(&mut self, op: &str, pos: Vec2) -> BpNodeId {
        let id = BpNodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(BpNode { id, op: op.to_owned(), pos, values: BTreeMap::new() });
        id
    }

    /// Adds a node and sets literal values in one go (handy for code-built graphs).
    pub fn add(&mut self, op: &str, pos: (f32, f32), values: &[(&str, Variant)]) -> BpNodeId {
        let id = self.add_node(op, Vec2::new(pos.0, pos.1));
        let n = self.node_mut(id).expect("just added");
        for (k, v) in values {
            n.values.insert((*k).to_owned(), v.clone());
        }
        id
    }

    pub fn remove_node(&mut self, id: BpNodeId) {
        self.nodes.retain(|n| n.id != id);
        self.links.retain(|l| l.from.node != id && l.to.node != id);
    }

    /// Copies nodes (and the links between them) with fresh ids, offset by `offset`.
    pub fn duplicate_nodes(&mut self, ids: &[BpNodeId], offset: Vec2) -> Vec<BpNodeId> {
        let mut map = BTreeMap::new();
        for &id in ids {
            if let Some(n) = self.node(id).cloned() {
                let new_id = self.add_node(&n.op, n.pos + offset);
                self.node_mut(new_id).expect("added").values = n.values;
                map.insert(id, new_id);
            }
        }
        let new_links: Vec<Link> = self
            .links
            .iter()
            .filter_map(|l| {
                Some(Link {
                    from: PinRef { node: *map.get(&l.from.node)?, pin: l.from.pin.clone() },
                    to: PinRef { node: *map.get(&l.to.node)?, pin: l.to.pin.clone() },
                })
            })
            .collect();
        self.links.extend(new_links);
        map.into_values().collect()
    }

    pub fn var(&self, name: &str) -> Option<&VarDef> {
        self.variables.iter().find(|v| v.name == name)
    }

    pub fn add_variable(&mut self, name: &str, ty: VType, default: Variant) {
        if self.var(name).is_none() {
            self.variables.push(VarDef { name: name.to_owned(), ty, default, exposed: false });
        }
    }

    /// Kind of a pin (with variable-aware typing for Get/Set Variable).
    pub fn pin_kind(&self, node: BpNodeId, pin: &str, output: bool) -> Option<PinKind> {
        let n = self.node(node)?;
        let def = n.def()?;
        let p = if output { def.output(pin)? } else { def.input(pin)? };
        if let (PinKind::Data(VType::Any), true) = (p.kind, n.op.starts_with("var.") && pin == "value") {
            let var_name = n.literal("var").to_string();
            if let Some(v) = self.var(&var_name) {
                return Some(PinKind::Data(v.ty));
            }
        }
        Some(p.kind)
    }

    /// Validates and adds a link. Single-link pins (data inputs, exec
    /// outputs) replace their previous connection, like Unreal.
    pub fn connect(&mut self, a: PinRef, b: PinRef) -> Result<(), LinkError> {
        if a.node == b.node {
            return Err(LinkError::SelfLink);
        }
        // Accept either drag direction: normalize to (output → input).
        let (from, to) = if self.pin_kind(a.node, &a.pin, true).is_some() && self.pin_kind(b.node, &b.pin, false).is_some()
        {
            (a, b)
        } else if self.pin_kind(b.node, &b.pin, true).is_some() && self.pin_kind(a.node, &a.pin, false).is_some() {
            (b, a)
        } else if self.node(a.node).is_none() || self.node(b.node).is_none() {
            return Err(LinkError::NoSuchPin);
        } else {
            return Err(LinkError::Direction);
        };
        let fk = self.pin_kind(from.node, &from.pin, true).ok_or(LinkError::NoSuchPin)?;
        let tk = self.pin_kind(to.node, &to.pin, false).ok_or(LinkError::NoSuchPin)?;
        let to_def = self.node(to.node).and_then(|n| n.def()).and_then(|d| d.input(&to.pin));
        if to_def.is_some_and(|p| p.literal_only) {
            return Err(LinkError::LiteralOnly);
        }
        match (fk, tk) {
            (PinKind::Exec, PinKind::Exec) => {
                self.links.retain(|l| l.from != from);
            }
            (PinKind::Data(ft), PinKind::Data(tt)) => {
                if !tt.accepts(ft) {
                    return Err(LinkError::Type { from: ft.name(), to: tt.name() });
                }
                self.links.retain(|l| l.to != to);
            }
            _ => return Err(LinkError::ExecMismatch),
        }
        self.links.push(Link { from, to });
        Ok(())
    }

    pub fn disconnect_pin(&mut self, pin: &PinRef) {
        self.links.retain(|l| l.from != *pin && l.to != *pin);
    }

    /// The output feeding a data input, if connected.
    pub fn source_of(&self, to: &PinRef) -> Option<&PinRef> {
        self.links.iter().find(|l| l.to == *to).map(|l| &l.from)
    }

    /// The node an exec output leads to.
    pub fn exec_target(&self, from: &PinRef) -> Option<&PinRef> {
        self.links.iter().find(|l| l.from == *from).map(|l| &l.to)
    }

    pub fn is_connected(&self, pin: &PinRef) -> bool {
        self.links.iter().any(|l| l.from == *pin || l.to == *pin)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("blueprint serialization cannot fail")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let mut bp: Blueprint = serde_json::from_str(s)?;
        // Repair next_id if the file was hand-edited.
        let max = bp.nodes.iter().map(|n| n.id.0).max().unwrap_or(0);
        bp.next_id = bp.next_id.max(max + 1);
        Ok(bp)
    }

    pub fn load(path: &Path) -> Result<Self, BlueprintIoError> {
        Ok(Self::from_json(&std::fs::read_to_string(path)?)?)
    }

    pub fn save(&self, path: &Path) -> Result<(), BlueprintIoError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("vbp.tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
