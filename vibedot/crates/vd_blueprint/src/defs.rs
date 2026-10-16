//! The blueprint node library: static, `const`-constructed descriptions of
//! every node a designer can place. Pins are identified by name so graphs
//! survive library changes that reorder pins.

use std::collections::HashMap;
use std::sync::OnceLock;

use vd_core::{Color, VType, Variant, Vec2};

use crate::ops::Op;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinKind {
    Exec,
    Data(VType),
}

/// Compile-time literal used for pin defaults (Variant holds `Arc`s and
/// cannot be built in a `const` context).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lit {
    Nil,
    Bool(bool),
    Num(f64),
    Vec2(f32, f32),
    Str(&'static str),
    Color(f32, f32, f32, f32),
    /// The node that owns the blueprint instance.
    SelfNode,
}

impl Lit {
    pub fn to_variant(self) -> Variant {
        match self {
            Lit::Nil | Lit::SelfNode => Variant::Nil,
            Lit::Bool(b) => Variant::Bool(b),
            Lit::Num(n) => Variant::Number(n),
            Lit::Vec2(x, y) => Variant::Vec2(Vec2::new(x, y)),
            Lit::Str(s) => Variant::str(s),
            Lit::Color(r, g, b, a) => Variant::Color(Color::rgba(r, g, b, a)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PinDef {
    pub name: &'static str,
    pub kind: PinKind,
    pub default: Lit,
    /// Must be a literal typed into the node (event names, variable names...).
    pub literal_only: bool,
}

impl PinDef {
    pub fn is_exec(&self) -> bool {
        self.kind == PinKind::Exec
    }
    pub fn vtype(&self) -> VType {
        match self.kind {
            PinKind::Exec => VType::Any,
            PinKind::Data(t) => t,
        }
    }
}

const fn pin(name: &'static str, kind: PinKind, default: Lit) -> PinDef {
    PinDef { name, kind, default, literal_only: false }
}
const fn exec(name: &'static str) -> PinDef {
    pin(name, PinKind::Exec, Lit::Nil)
}
const fn num(name: &'static str, d: f64) -> PinDef {
    pin(name, PinKind::Data(VType::Number), Lit::Num(d))
}
/// Polymorphic numeric pin (accepts numbers, vectors, colors).
const fn anynum(name: &'static str, d: f64) -> PinDef {
    pin(name, PinKind::Data(VType::Any), Lit::Num(d))
}
const fn boolean(name: &'static str, d: bool) -> PinDef {
    pin(name, PinKind::Data(VType::Bool), Lit::Bool(d))
}
const fn v2(name: &'static str, x: f32, y: f32) -> PinDef {
    pin(name, PinKind::Data(VType::Vec2), Lit::Vec2(x, y))
}
const fn string(name: &'static str, d: &'static str) -> PinDef {
    pin(name, PinKind::Data(VType::String), Lit::Str(d))
}
const fn color(name: &'static str, r: f32, g: f32, b: f32, a: f32) -> PinDef {
    pin(name, PinKind::Data(VType::Color), Lit::Color(r, g, b, a))
}
const fn node(name: &'static str) -> PinDef {
    pin(name, PinKind::Data(VType::Object), Lit::SelfNode)
}
const fn node_nil(name: &'static str) -> PinDef {
    pin(name, PinKind::Data(VType::Object), Lit::Nil)
}
const fn any(name: &'static str) -> PinDef {
    pin(name, PinKind::Data(VType::Any), Lit::Nil)
}
const fn array(name: &'static str) -> PinDef {
    pin(name, PinKind::Data(VType::Array), Lit::Nil)
}
/// Literal-only string ("config") pin.
const fn cfg(name: &'static str, d: &'static str) -> PinDef {
    PinDef { name, kind: PinKind::Data(VType::String), default: Lit::Str(d), literal_only: true }
}
const fn out(name: &'static str, t: VType) -> PinDef {
    pin(name, PinKind::Data(t), Lit::Nil)
}

const EXEC_IN: PinDef = exec("exec");
const THEN: PinDef = exec("then");

/// Entry points a blueprint can respond to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    Ready,
    Process,
    ActionPressed,
    ActionReleased,
    KeyPressed,
    MousePressed,
    BodyEntered,
    BodyExited,
    Timeout,
    Custom,
}

/// Control-flow nodes handled directly by the compiler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowKind {
    Branch,
    Sequence,
    For,
    ForEach,
    While,
    DoOnce,
    FlipFlop,
    Delay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Get,
    Set,
    Add,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// No exec pins; evaluated on demand when an output is read.
    Pure(Op),
    /// Has `exec` in and `then` out; runs when execution reaches it.
    Exec(Op),
    Event(EventKind),
    Flow(FlowKind),
    Var(VarKind),
}

#[derive(Debug)]
pub struct NodeDef {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub flavor: Flavor,
    pub inputs: &'static [PinDef],
    pub outputs: &'static [PinDef],
    /// Pin whose literal value is shown in the node title (e.g. event name).
    pub title_pin: Option<&'static str>,
}

impl NodeDef {
    pub fn input(&self, name: &str) -> Option<&'static PinDef> {
        self.inputs.iter().find(|p| p.name == name)
    }
    pub fn output(&self, name: &str) -> Option<&'static PinDef> {
        self.outputs.iter().find(|p| p.name == name)
    }
    pub fn is_pure(&self) -> bool {
        matches!(self.flavor, Flavor::Pure(_)) || self.flavor == Flavor::Var(VarKind::Get)
    }
    pub fn is_event(&self) -> bool {
        matches!(self.flavor, Flavor::Event(_))
    }
}

macro_rules! def {
    ($id:literal, $title:literal, $cat:literal, $desc:literal, $flavor:expr,
     in [$($i:expr),* $(,)?], out [$($o:expr),* $(,)?] $(, title_pin = $tp:literal)?) => {
        NodeDef {
            id: $id, title: $title, category: $cat, description: $desc, flavor: $flavor,
            inputs: &[$($i),*], outputs: &[$($o),*],
            title_pin: { #[allow(unused_mut, unused_assignments)] let mut t = None; $(t = Some($tp);)? t },
        }
    };
}

use EventKind as E;
use Flavor::{Event, Exec, Flow, Pure, Var};
use FlowKind as F;
use VType as T;

/// The full node library.
pub static LIBRARY: &[NodeDef] = &[
    // ------------------------------------------------------------------ events
    def!("event.ready", "On Ready", "Events", "Runs once when the node enters the scene.",
        Event(E::Ready), in [], out [THEN]),
    def!("event.process", "On Process", "Events", "Runs every frame. `delta` is the frame time in seconds.",
        Event(E::Process), in [], out [THEN, out("delta", T::Number)]),
    def!("event.action_pressed", "On Action Pressed", "Events", "Runs when an input action is pressed (see Project -> Input Map).",
        Event(E::ActionPressed), in [cfg("action", "fire")], out [THEN], title_pin = "action"),
    def!("event.action_released", "On Action Released", "Events", "Runs when an input action is released.",
        Event(E::ActionReleased), in [cfg("action", "fire")], out [THEN], title_pin = "action"),
    def!("event.key_pressed", "On Key Pressed", "Events", "Runs when a keyboard key is pressed (e.g. Space, A, F1, Enter).",
        Event(E::KeyPressed), in [cfg("key", "Space")], out [THEN], title_pin = "key"),
    def!("event.mouse_pressed", "On Mouse Click", "Events", "Runs on a mouse button press (Left / Right / Middle).",
        Event(E::MousePressed), in [cfg("button", "Left")], out [THEN, out("position", T::Vec2)], title_pin = "button"),
    def!("event.body_entered", "On Body Entered", "Events", "A physics body / area started touching this one.",
        Event(E::BodyEntered), in [], out [THEN, out("other", T::Object)]),
    def!("event.body_exited", "On Body Exited", "Events", "A physics body / area stopped touching this one.",
        Event(E::BodyExited), in [], out [THEN, out("other", T::Object)]),
    def!("event.timeout", "On Timer Timeout", "Events", "A Timer fired. `timer` = name of a child Timer (empty: this node or any child).",
        Event(E::Timeout), in [cfg("timer", "")], out [THEN, out("timer", T::Object)], title_pin = "timer"),
    def!("event.custom", "On Event / Method", "Events", "Runs when a custom event (`Emit Event`) or a connected signal calls this method name.",
        Event(E::Custom), in [cfg("name", "my_event")], out [THEN, out("payload", T::Any), out("sender", T::Object), out("args", T::Array)], title_pin = "name"),
    // ------------------------------------------------------------------ flow
    def!("flow.branch", "Branch", "Flow", "If / else.",
        Flow(F::Branch), in [EXEC_IN, boolean("condition", true)], out [exec("true"), exec("false")]),
    def!("flow.sequence", "Sequence", "Flow", "Runs the outputs one after another.",
        Flow(F::Sequence), in [EXEC_IN], out [exec("then 0"), exec("then 1"), exec("then 2"), exec("then 3")]),
    def!("flow.for", "For Loop", "Flow", "Runs `body` for each index from `first` to `last` (inclusive).",
        Flow(F::For), in [EXEC_IN, num("first", 0.0), num("last", 9.0)],
        out [exec("body"), out("index", T::Number), exec("completed")]),
    def!("flow.foreach", "For Each", "Flow", "Runs `body` for each element of an array.",
        Flow(F::ForEach), in [EXEC_IN, array("array")],
        out [exec("body"), out("element", T::Any), out("index", T::Number), exec("completed")]),
    def!("flow.while", "While Loop", "Flow", "Runs `body` while the condition holds (guarded against infinite loops).",
        Flow(F::While), in [EXEC_IN, boolean("condition", false)], out [exec("body"), exec("completed")]),
    def!("flow.do_once", "Do Once", "Flow", "Lets execution through only the first time.",
        Flow(F::DoOnce), in [EXEC_IN], out [THEN]),
    def!("flow.flip_flop", "Flip Flop", "Flow", "Alternates between A and B on each run.",
        Flow(F::FlipFlop), in [EXEC_IN], out [exec("a"), exec("b"), out("is_a", T::Bool)]),
    def!("flow.delay", "Delay", "Flow", "Waits `seconds` then continues from `completed` (latent).",
        Flow(F::Delay), in [EXEC_IN, num("seconds", 1.0)], out [exec("completed")]),
    // ------------------------------------------------------------------ variables
    def!("var.get", "Get Variable", "Variables", "Reads a blueprint variable.",
        Var(VarKind::Get), in [cfg("var", "")], out [out("value", T::Any)], title_pin = "var"),
    def!("var.set", "Set Variable", "Variables", "Writes a blueprint variable.",
        Var(VarKind::Set), in [EXEC_IN, cfg("var", ""), any("value")], out [THEN, out("value", T::Any)], title_pin = "var"),
    def!("var.add", "Add To Variable", "Variables", "variable += amount (numbers, vectors) or appends (strings).",
        Var(VarKind::Add), in [EXEC_IN, cfg("var", ""), num("amount", 1.0)], out [THEN, out("value", T::Any)], title_pin = "var"),
    // ------------------------------------------------------------------ literals
    def!("lit.number", "Number", "Literals", "A constant number.", Pure(Op::Identity), in [num("value", 0.0)], out [out("out", T::Number)]),
    def!("lit.bool", "Boolean", "Literals", "A constant boolean.", Pure(Op::Identity), in [boolean("value", true)], out [out("out", T::Bool)]),
    def!("lit.string", "Text", "Literals", "A constant string.", Pure(Op::Identity), in [string("value", "")], out [out("out", T::String)]),
    def!("lit.vec2", "Vector2", "Literals", "A constant 2D vector.", Pure(Op::Identity), in [v2("value", 0.0, 0.0)], out [out("out", T::Vec2)]),
    def!("lit.color", "Color", "Literals", "A constant color.", Pure(Op::Identity), in [color("value", 1.0, 1.0, 1.0, 1.0)], out [out("out", T::Color)]),
    // ------------------------------------------------------------------ math
    def!("math.add", "Add", "Math", "a + b (numbers, vectors, colors; joins strings).", Pure(Op::Add), in [anynum("a", 0.0), anynum("b", 0.0)], out [out("result", T::Any)]),
    def!("math.sub", "Subtract", "Math", "a - b", Pure(Op::Sub), in [anynum("a", 0.0), anynum("b", 0.0)], out [out("result", T::Any)]),
    def!("math.mul", "Multiply", "Math", "a × b (vector × number scales).", Pure(Op::Mul), in [anynum("a", 1.0), anynum("b", 1.0)], out [out("result", T::Any)]),
    def!("math.div", "Divide", "Math", "a ÷ b (division by zero gives 0).", Pure(Op::Div), in [anynum("a", 1.0), anynum("b", 1.0)], out [out("result", T::Any)]),
    def!("math.mod", "Modulo", "Math", "Positive remainder of a ÷ b.", Pure(Op::Mod), in [num("a", 0.0), num("b", 1.0)], out [out("result", T::Number)]),
    def!("math.pow", "Power", "Math", "base ^ exponent", Pure(Op::Pow), in [num("base", 2.0), num("exponent", 2.0)], out [out("result", T::Number)]),
    def!("math.min", "Min", "Math", "Smaller of a and b.", Pure(Op::Min), in [num("a", 0.0), num("b", 0.0)], out [out("result", T::Number)]),
    def!("math.max", "Max", "Math", "Larger of a and b.", Pure(Op::Max), in [num("a", 0.0), num("b", 0.0)], out [out("result", T::Number)]),
    def!("math.clamp", "Clamp", "Math", "Limits value to [min, max].", Pure(Op::Clamp), in [num("value", 0.0), num("min", 0.0), num("max", 1.0)], out [out("result", T::Number)]),
    def!("math.lerp", "Lerp", "Math", "Linear interpolation a -> b by t (numbers, vectors, colors).", Pure(Op::Lerp), in [anynum("a", 0.0), anynum("b", 1.0), num("t", 0.5)], out [out("result", T::Any)]),
    def!("math.map", "Map Range", "Math", "Remaps value from [in_min,in_max] to [out_min,out_max].", Pure(Op::MapRange),
        in [num("value", 0.0), num("in_min", 0.0), num("in_max", 1.0), num("out_min", 0.0), num("out_max", 100.0)], out [out("result", T::Number)]),
    def!("math.abs", "Abs", "Math", "Absolute value.", Pure(Op::Abs), in [num("value", 0.0)], out [out("result", T::Number)]),
    def!("math.negate", "Negate", "Math", "-value", Pure(Op::Negate), in [anynum("value", 0.0)], out [out("result", T::Any)]),
    def!("math.sqrt", "Square Root", "Math", "√value", Pure(Op::Sqrt), in [num("value", 0.0)], out [out("result", T::Number)]),
    def!("math.sin", "Sin", "Math", "Sine of an angle in degrees.", Pure(Op::Sin), in [num("degrees", 0.0)], out [out("result", T::Number)]),
    def!("math.cos", "Cos", "Math", "Cosine of an angle in degrees.", Pure(Op::Cos), in [num("degrees", 0.0)], out [out("result", T::Number)]),
    def!("math.atan2", "Atan2", "Math", "Angle in degrees of the vector (x, y).", Pure(Op::Atan2), in [num("y", 0.0), num("x", 1.0)], out [out("degrees", T::Number)]),
    def!("math.round", "Round", "Math", "Rounds to the nearest integer.", Pure(Op::Round), in [num("value", 0.0)], out [out("result", T::Number)]),
    def!("math.floor", "Floor", "Math", "Rounds down.", Pure(Op::Floor), in [num("value", 0.0)], out [out("result", T::Number)]),
    def!("math.ceil", "Ceil", "Math", "Rounds up.", Pure(Op::Ceil), in [num("value", 0.0)], out [out("result", T::Number)]),
    def!("math.sign", "Sign", "Math", "-1, 0 or 1.", Pure(Op::Sign), in [num("value", 0.0)], out [out("result", T::Number)]),
    def!("math.wave", "Wave", "Math", "Smooth oscillation between -amplitude and +amplitude over time.", Pure(Op::Wave),
        in [num("frequency", 1.0), num("amplitude", 1.0), num("phase", 0.0)], out [out("result", T::Number)]),
    def!("math.random", "Random Range", "Math", "Random number in [min, max).", Pure(Op::RandomRange), in [num("min", 0.0), num("max", 1.0)], out [out("result", T::Number)]),
    def!("math.random_int", "Random Integer", "Math", "Random integer in [min, max] inclusive.", Pure(Op::RandomInt), in [num("min", 0.0), num("max", 10.0)], out [out("result", T::Number)]),
    def!("math.chance", "Chance", "Math", "True with the given probability (0..1).", Pure(Op::Chance), in [num("probability", 0.5)], out [out("result", T::Bool)]),
    def!("math.time", "Get Time", "Math", "Seconds since the game started.", Pure(Op::Time), in [], out [out("seconds", T::Number)]),
    // ------------------------------------------------------------------ vectors
    def!("vec.make", "Make Vector2", "Vector", "Builds a vector from x and y.", Pure(Op::MakeVec), in [num("x", 0.0), num("y", 0.0)], out [out("vector", T::Vec2)]),
    def!("vec.break", "Break Vector2", "Vector", "Splits a vector into x and y.", Pure(Op::BreakVec), in [v2("vector", 0.0, 0.0)], out [out("x", T::Number), out("y", T::Number)]),
    def!("vec.length", "Length", "Vector", "Length of the vector.", Pure(Op::Length), in [v2("vector", 0.0, 0.0)], out [out("length", T::Number)]),
    def!("vec.normalize", "Normalize", "Vector", "Unit-length vector (zero stays zero).", Pure(Op::Normalize), in [v2("vector", 1.0, 0.0)], out [out("result", T::Vec2)]),
    def!("vec.distance", "Distance", "Vector", "Distance between two points.", Pure(Op::Distance), in [v2("a", 0.0, 0.0), v2("b", 0.0, 0.0)], out [out("distance", T::Number)]),
    def!("vec.direction", "Direction To", "Vector", "Unit vector pointing from `from` to `to`.", Pure(Op::DirectionTo), in [v2("from", 0.0, 0.0), v2("to", 1.0, 0.0)], out [out("direction", T::Vec2)]),
    def!("vec.dot", "Dot", "Vector", "Dot product.", Pure(Op::Dot), in [v2("a", 1.0, 0.0), v2("b", 1.0, 0.0)], out [out("result", T::Number)]),
    def!("vec.from_angle", "Vector From Angle", "Vector", "Vector of `length` pointing at `degrees`.", Pure(Op::FromAngle), in [num("degrees", 0.0), num("length", 1.0)], out [out("vector", T::Vec2)]),
    def!("vec.angle", "Vector Angle", "Vector", "Angle of the vector in degrees.", Pure(Op::Angle), in [v2("vector", 1.0, 0.0)], out [out("degrees", T::Number)]),
    def!("vec.rotate", "Rotate Vector", "Vector", "Rotates a vector by degrees.", Pure(Op::RotateVec), in [v2("vector", 1.0, 0.0), num("degrees", 90.0)], out [out("result", T::Vec2)]),
    def!("vec.move_toward", "Move Toward", "Vector", "Moves `from` toward `to` by at most `delta`.", Pure(Op::MoveToward), in [v2("from", 0.0, 0.0), v2("to", 0.0, 0.0), num("delta", 1.0)], out [out("result", T::Vec2)]),
    def!("vec.clamp", "Clamp Vector", "Vector", "Clamps each component between min and max (Godot `Vector2.clamp`).", Pure(Op::ClampVec),
        in [v2("vector", 0.0, 0.0), v2("min", -100.0, -100.0), v2("max", 100.0, 100.0)], out [out("result", T::Vec2)]),
    def!("vec.random_in_circle", "Random In Circle", "Vector", "Random point inside a circle of `radius`.", Pure(Op::RandomInCircle), in [num("radius", 1.0)], out [out("point", T::Vec2)]),
    // ------------------------------------------------------------------ logic
    def!("cmp.gt", "Greater (>)", "Logic", "a > b", Pure(Op::Gt), in [num("a", 0.0), num("b", 0.0)], out [out("result", T::Bool)]),
    def!("cmp.ge", "Greater Or Equal (≥)", "Logic", "a ≥ b", Pure(Op::Ge), in [num("a", 0.0), num("b", 0.0)], out [out("result", T::Bool)]),
    def!("cmp.lt", "Less (<)", "Logic", "a < b", Pure(Op::Lt), in [num("a", 0.0), num("b", 0.0)], out [out("result", T::Bool)]),
    def!("cmp.le", "Less Or Equal (≤)", "Logic", "a ≤ b", Pure(Op::Le), in [num("a", 0.0), num("b", 0.0)], out [out("result", T::Bool)]),
    def!("cmp.eq", "Equal (==)", "Logic", "a == b (any types).", Pure(Op::Eq), in [any("a"), any("b")], out [out("result", T::Bool)]),
    def!("cmp.ne", "Not Equal (!=)", "Logic", "a != b (any types).", Pure(Op::Ne), in [any("a"), any("b")], out [out("result", T::Bool)]),
    def!("logic.and", "And", "Logic", "a AND b", Pure(Op::And), in [boolean("a", true), boolean("b", true)], out [out("result", T::Bool)]),
    def!("logic.or", "Or", "Logic", "a OR b", Pure(Op::Or), in [boolean("a", false), boolean("b", false)], out [out("result", T::Bool)]),
    def!("logic.not", "Not", "Logic", "NOT value", Pure(Op::Not), in [boolean("value", false)], out [out("result", T::Bool)]),
    def!("logic.select", "Select", "Logic", "Returns `a` if condition is true, otherwise `b`.", Pure(Op::Select), in [boolean("condition", true), any("a"), any("b")], out [out("result", T::Any)]),
    // ------------------------------------------------------------------ text
    def!("str.concat", "Join Text", "Text", "a + b as text.", Pure(Op::Concat), in [string("a", ""), string("b", "")], out [out("result", T::String)]),
    def!("str.format", "Format Text", "Text", "Replaces {0}, {1}, {2} in the template.", Pure(Op::Format),
        in [string("template", "Score: {0}"), any("0"), any("1"), any("2")], out [out("result", T::String)]),
    def!("str.to_string", "To Text", "Text", "Converts any value to text.", Pure(Op::ToText), in [any("value")], out [out("text", T::String)]),
    // ------------------------------------------------------------------ color
    def!("color.make", "Make Color", "Color", "Color from linear RGBA (values > 1 glow).", Pure(Op::MakeColor),
        in [num("r", 1.0), num("g", 1.0), num("b", 1.0), num("a", 1.0)], out [out("color", T::Color)]),
    def!("color.hsv", "Color From HSV", "Color", "Hue, saturation, value in 0..1.", Pure(Op::Hsv),
        in [num("hue", 0.0), num("saturation", 1.0), num("value", 1.0), num("alpha", 1.0)], out [out("color", T::Color)]),
    // ------------------------------------------------------------------ arrays
    def!("array.length", "Array Length", "Array", "Number of elements.", Pure(Op::ArrayLen), in [array("array")], out [out("length", T::Number)]),
    def!("array.get", "Array Get", "Array", "Element at index (null if out of range).", Pure(Op::ArrayGet), in [array("array"), num("index", 0.0)], out [out("element", T::Any)]),
    def!("array.random", "Random Element", "Array", "A random element (null if empty).", Pure(Op::ArrayRandom), in [array("array")], out [out("element", T::Any)]),
    // ------------------------------------------------------------------ nodes (pure)
    def!("node.self", "Self", "Node", "The node this blueprint is attached to.", Pure(Op::SelfNode), in [], out [out("self", T::Object)]),
    def!("node.get_node", "Get Node", "Node", "Finds a node by path relative to `from` (\"../Player\", \"%Score\").", Pure(Op::GetNode),
        in [node("from"), string("path", "")], out [out("node", T::Object)]),
    def!("node.parent", "Get Parent", "Node", "Parent of a node.", Pure(Op::GetParent), in [node("node")], out [out("parent", T::Object)]),
    def!("node.first_in_group", "First In Group", "Node", "First node in a group (e.g. \"player\").", Pure(Op::FirstInGroup), in [string("group", "player")], out [out("node", T::Object)]),
    def!("node.group", "Get Nodes In Group", "Node", "All nodes in a group.", Pure(Op::NodesInGroup), in [string("group", "enemies")], out [out("nodes", T::Array)]),
    def!("node.is_valid", "Is Valid", "Node", "True if the node still exists.", Pure(Op::IsValid), in [node_nil("node")], out [out("valid", T::Bool)]),
    def!("node.in_group", "Is In Group", "Node", "True if the node belongs to the group.", Pure(Op::IsInGroup), in [node("node"), string("group", "enemies")], out [out("result", T::Bool)]),
    def!("node.name", "Get Name", "Node", "Node name.", Pure(Op::GetName), in [node("node")], out [out("name", T::String)]),
    def!("node.get_position", "Get Position", "Transform", "Global position of a node.", Pure(Op::GetPosition), in [node("node")], out [out("position", T::Vec2)]),
    def!("node.get_rotation", "Get Rotation", "Transform", "Rotation in degrees.", Pure(Op::GetRotation), in [node("node")], out [out("degrees", T::Number)]),
    def!("node.get_scale", "Get Scale", "Transform", "Local scale.", Pure(Op::GetScale), in [node("node")], out [out("scale", T::Vec2)]),
    def!("node.distance_to", "Distance Between Nodes", "Transform", "Distance between two nodes' global positions.", Pure(Op::NodeDistance),
        in [node("a"), node_nil("b")], out [out("distance", T::Number)]),
    def!("node.get_property", "Get Property", "Node", "Reads any property by name (e.g. \"text\", \"energy\").", Pure(Op::GetProperty),
        in [node("node"), string("property", "position")], out [out("value", T::Any)]),
    // ------------------------------------------------------------------ nodes (exec)
    def!("node.set_position", "Set Position", "Transform", "Sets the global position.", Exec(Op::SetPosition), in [EXEC_IN, node("node"), v2("position", 0.0, 0.0)], out [THEN]),
    def!("node.translate", "Move By", "Transform", "Moves a node by an offset.", Exec(Op::Translate), in [EXEC_IN, node("node"), v2("offset", 0.0, 0.0)], out [THEN]),
    def!("node.set_rotation", "Set Rotation", "Transform", "Sets rotation in degrees.", Exec(Op::SetRotation), in [EXEC_IN, node("node"), num("degrees", 0.0)], out [THEN]),
    def!("node.rotate", "Rotate By", "Transform", "Adds degrees to the rotation.", Exec(Op::Rotate), in [EXEC_IN, node("node"), num("degrees", 0.0)], out [THEN]),
    def!("node.look_at", "Look At", "Transform", "Rotates the node to face a point.", Exec(Op::LookAt), in [EXEC_IN, node("node"), v2("target", 0.0, 0.0)], out [THEN]),
    def!("node.set_scale", "Set Scale", "Transform", "Sets the local scale.", Exec(Op::SetScale), in [EXEC_IN, node("node"), v2("scale", 1.0, 1.0)], out [THEN]),
    def!("node.set_visible", "Set Visible", "Node", "Shows or hides a node.", Exec(Op::SetVisible), in [EXEC_IN, node("node"), boolean("visible", true)], out [THEN]),
    def!("node.set_property", "Set Property", "Node", "Writes any property by name.", Exec(Op::SetProperty),
        in [EXEC_IN, node("node"), string("property", "position"), any("value")], out [THEN]),
    def!("node.set_text", "Set Text", "Node", "Sets a Label's text.", Exec(Op::SetText), in [EXEC_IN, node("node"), string("text", "")], out [THEN]),
    def!("node.set_color", "Set Color", "Node", "Sets the node's color (or modulate).", Exec(Op::SetColor), in [EXEC_IN, node("node"), color("color", 1.0, 1.0, 1.0, 1.0)], out [THEN]),
    def!("node.add_group", "Add To Group", "Node", "Adds a node to a group.", Exec(Op::AddToGroup), in [EXEC_IN, node("node"), string("group", "")], out [THEN]),
    def!("node.destroy", "Destroy", "Node", "Removes a node (and its children) at the end of the frame.", Exec(Op::QueueFree), in [EXEC_IN, node("node")], out [THEN]),
    def!("node.spawn", "Spawn Scene", "Node", "Instantiates a scene file at a position.", Exec(Op::Spawn),
        in [EXEC_IN, string("scene", "scenes/bullet.vscn"), v2("position", 0.0, 0.0), node_nil("parent")], out [THEN, out("node", T::Object)]),
    def!("node.get_child", "Get Child", "Node", "Child at index (negative counts from the end).", Pure(Op::GetChild),
        in [node("node"), num("index", 0.0)], out [out("child", T::Object)]),
    def!("node.child_count", "Get Child Count", "Node", "Number of children.", Pure(Op::ChildCount), in [node("node")], out [out("count", T::Number)]),
    def!("node.find_child", "Find Child", "Node", "First descendant whose name matches a pattern (* and ? wildcards).", Pure(Op::FindChild),
        in [node("node"), string("pattern", "Enemy*"), boolean("recursive", true)], out [out("child", T::Object)]),
    def!("node.reparent", "Reparent", "Node", "Moves a node under a new parent, keeping its global transform.", Exec(Op::Reparent),
        in [EXEC_IN, node("node"), node_nil("new_parent")], out [THEN]),
    // ------------------------------------------------------------------ signals
    def!("signal.connect", "Connect Signal", "Signals", "Connects `source`'s signal to a method (an `On Event / Method` handler) on `target`.", Exec(Op::ConnectSignal),
        in [EXEC_IN, node("source"), string("signal", "timeout"), node("target"), string("method", "_on_timeout"), boolean("one_shot", false)], out [THEN]),
    def!("signal.disconnect", "Disconnect Signal", "Signals", "Removes a signal connection.", Exec(Op::DisconnectSignal),
        in [EXEC_IN, node("source"), string("signal", "timeout"), node("target"), string("method", "_on_timeout")], out [THEN]),
    def!("signal.emit", "Emit Signal", "Signals", "Emits a signal from a node with up to two arguments.", Exec(Op::EmitSignal),
        in [EXEC_IN, node("node"), string("signal", "hit"), any("arg0"), any("arg1")], out [THEN]),
    def!("signal.is_connected", "Is Connected", "Signals", "True if the connection exists.", Pure(Op::IsConnected),
        in [node("source"), string("signal", "timeout"), node("target"), string("method", "_on_timeout")], out [out("connected", T::Bool)]),
    // ------------------------------------------------------------------ physics
    def!("physics.set_velocity", "Set Velocity", "Physics", "Sets a body's velocity (pixels / second).", Exec(Op::SetVelocity), in [EXEC_IN, node("node"), v2("velocity", 0.0, 0.0)], out [THEN]),
    def!("physics.get_velocity", "Get Velocity", "Physics", "A body's velocity.", Pure(Op::GetVelocity), in [node("node")], out [out("velocity", T::Vec2)]),
    def!("physics.impulse", "Apply Impulse", "Physics", "Instant push, scaled by 1/mass.", Exec(Op::ApplyImpulse), in [EXEC_IN, node("node"), v2("impulse", 0.0, -300.0)], out [THEN]),
    def!("physics.on_floor", "Is On Floor", "Physics", "True if the body is standing on something.", Pure(Op::IsOnFloor), in [node("node")], out [out("result", T::Bool)]),
    // ------------------------------------------------------------------ input
    def!("input.pressed", "Is Action Pressed", "Input", "True while an action is held.", Pure(Op::ActionPressed), in [string("action", "ui_right")], out [out("pressed", T::Bool)]),
    def!("input.just_pressed", "Is Action Just Pressed", "Input", "True only in the frame the action was pressed.", Pure(Op::ActionJustPressed), in [string("action", "jump")], out [out("pressed", T::Bool)]),
    def!("input.axis", "Get Axis", "Input", "-1..1 from two actions.", Pure(Op::Axis), in [string("negative", "ui_left"), string("positive", "ui_right")], out [out("value", T::Number)]),
    def!("input.vector", "Get Input Vector", "Input", "Normalized direction from four actions.", Pure(Op::InputVector),
        in [string("left", "ui_left"), string("right", "ui_right"), string("up", "ui_up"), string("down", "ui_down")], out [out("vector", T::Vec2)]),
    def!("input.mouse", "Mouse Position", "Input", "Mouse position in world coordinates.", Pure(Op::MousePosition), in [], out [out("position", T::Vec2)]),
    // ------------------------------------------------------------------ game / fx
    def!("debug.print", "Print", "Game", "Writes a message to the Output panel.", Exec(Op::Print), in [EXEC_IN, any("value")], out [THEN]),
    def!("game.emit", "Emit Event", "Game", "Sends a custom event to `target` (empty = every blueprint).", Exec(Op::EmitEvent),
        in [EXEC_IN, string("name", "my_event"), any("payload"), node_nil("target")], out [THEN]),
    def!("game.change_scene", "Change Scene", "Game", "Loads another scene.", Exec(Op::ChangeScene), in [EXEC_IN, string("scene", "scenes/main.vscn")], out [THEN]),
    def!("game.reload", "Restart Scene", "Game", "Reloads the current scene.", Exec(Op::ReloadScene), in [EXEC_IN], out [THEN]),
    def!("game.shake", "Camera Shake", "Game", "Adds screen-shake trauma (0..1).", Exec(Op::CameraShake), in [EXEC_IN, num("amount", 0.4)], out [THEN]),
    def!("game.hitstop", "Hit Stop", "Game", "Briefly freezes the game for impact (seconds).", Exec(Op::HitStop), in [EXEC_IN, num("seconds", 0.05)], out [THEN]),
    def!("game.time_scale", "Set Time Scale", "Game", "Slow motion (<1) or fast forward (>1).", Exec(Op::TimeScale), in [EXEC_IN, num("scale", 1.0)], out [THEN]),
    def!("game.set_env", "Set Environment", "Rendering", "Changes a WorldEnvironment property (bloom_intensity, exposure...).", Exec(Op::SetEnv),
        in [EXEC_IN, string("property", "bloom_intensity"), any("value")], out [THEN]),
    def!("game.flash", "Screen Flash", "Rendering", "Flashes the screen with a color that fades out.", Exec(Op::Flash),
        in [EXEC_IN, color("color", 1.0, 1.0, 1.0, 0.6), num("duration", 0.15)], out [THEN]),
    def!("fx.burst", "Particles Burst", "Rendering", "Emits a burst from a Particles2D node.", Exec(Op::Burst), in [EXEC_IN, node("node"), num("count", 32.0)], out [THEN]),
    def!("timer.start", "Start Timer", "Game", "Starts (or restarts) a Timer node.", Exec(Op::TimerStart), in [EXEC_IN, node("timer"), num("wait_time", 0.0)], out [THEN]),
    def!("timer.stop", "Stop Timer", "Game", "Stops a Timer node.", Exec(Op::TimerStop), in [EXEC_IN, node("timer")], out [THEN]),
    def!("game.pause", "Set Paused", "Game", "Pauses the SceneTree (nodes with process_mode Always / WhenPaused keep running).", Exec(Op::SetPaused),
        in [EXEC_IN, boolean("paused", true)], out [THEN]),
    def!("game.is_paused", "Is Paused", "Game", "True while the SceneTree is paused.", Pure(Op::IsPaused), in [], out [out("paused", T::Bool)]),
    def!("game.quit", "Quit Game", "Game", "Closes the game.", Exec(Op::Quit), in [EXEC_IN], out [THEN]),
];

/// Looks a definition up by id (O(1) after first call).
pub fn find(id: &str) -> Option<&'static NodeDef> {
    static INDEX: OnceLock<HashMap<&'static str, &'static NodeDef>> = OnceLock::new();
    INDEX.get_or_init(|| LIBRARY.iter().map(|d| (d.id, d)).collect()).get(id).copied()
}

/// Category names in display order.
pub fn categories() -> Vec<&'static str> {
    let mut cats: Vec<&'static str> = Vec::new();
    for d in LIBRARY {
        if !cats.contains(&d.category) {
            cats.push(d.category);
        }
    }
    cats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_pins_sane() {
        let mut seen = std::collections::HashSet::new();
        for d in LIBRARY {
            assert!(seen.insert(d.id), "duplicate id {}", d.id);
            let mut names = std::collections::HashSet::new();
            for p in d.inputs {
                assert!(names.insert(("in", p.name)), "{}: dup input {}", d.id, p.name);
            }
            for p in d.outputs {
                assert!(names.insert(("out", p.name)), "{}: dup output {}", d.id, p.name);
            }
            match d.flavor {
                Flavor::Exec(_) => {
                    assert!(d.input("exec").is_some() && d.output("then").is_some(), "{}", d.id);
                }
                Flavor::Pure(_) => {
                    assert!(d.inputs.iter().chain(d.outputs).all(|p| !p.is_exec()), "{}", d.id);
                }
                Flavor::Event(_) => assert!(d.input("exec").is_none(), "{}", d.id),
                _ => {}
            }
        }
        assert!(LIBRARY.len() > 100, "library has {} nodes", LIBRARY.len());
        assert!(find("math.add").is_some());
    }
}
