//! Native operations invoked by the `Call` instruction.

use std::sync::Arc;

use vd_core::{Color, StringName, Variant, Vec2};
use vd_scene::{NodeId, NodeKind, SceneTree};

use crate::vm::ExecCtx;

/// Every native operation. `#[repr(u8)]` keeps `Instr` compact.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Identity,
    // math
    Add, Sub, Mul, Div, Mod, Pow, Min, Max, Clamp, Lerp, MapRange, Abs, Negate, Sqrt, Sin, Cos, Atan2,
    Round, Floor, Ceil, Sign, Wave, RandomRange, RandomInt, Chance, Time,
    // vectors
    MakeVec, BreakVec, Length, Normalize, Distance, DirectionTo, Dot, FromAngle, Angle, RotateVec, MoveToward,
    RandomInCircle,
    // logic
    Gt, Ge, Lt, Le, Eq, Ne, And, Or, Not, Select,
    // text / color / arrays
    Concat, Format, ToText, MakeColor, Hsv, ArrayLen, ArrayGet, ArrayRandom,
    // node queries
    SelfNode, GetNode, GetParent, FirstInGroup, NodesInGroup, IsValid, IsInGroup, GetName, GetPosition,
    GetRotation, GetScale, NodeDistance, GetProperty, GetChild, ChildCount, FindChild, Reparent,
    // signals
    ConnectSignal, DisconnectSignal, EmitSignal, IsConnected,
    // node commands
    SetPosition, Translate, SetRotation, Rotate, LookAt, SetScale, SetVisible, SetProperty, SetText, SetColor,
    AddToGroup, QueueFree, Spawn,
    // physics
    SetVelocity, GetVelocity, ApplyImpulse, IsOnFloor,
    // input
    ActionPressed, ActionJustPressed, Axis, InputVector, MousePosition,
    // game
    Print, EmitEvent, ChangeScene, ReloadScene, CameraShake, HitStop, TimeScale, SetEnv, Flash, Burst, TimerStart,
    TimerStop, Quit, SetPaused, IsPaused,
}

#[inline]
fn n(v: &Variant) -> f64 {
    v.to_number()
}

/// Arithmetic that adapts to operand types (number, vector, color, string).
fn arith(a: &Variant, b: &Variant, f: fn(f64, f64) -> f64, fv: fn(Vec2, Vec2) -> Vec2) -> Variant {
    match (a, b) {
        (Variant::Vec2(x), Variant::Vec2(y)) => Variant::Vec2(fv(*x, *y)),
        (Variant::Vec2(x), other) => Variant::Vec2(fv(*x, Vec2::splat(other.to_f32()))),
        (other, Variant::Vec2(y)) => Variant::Vec2(fv(Vec2::splat(other.to_f32()), *y)),
        (Variant::Color(x), Variant::Color(y)) => {
            let c = |p: f32, q: f32| f(f64::from(p), f64::from(q)) as f32;
            Variant::Color(Color::rgba(c(x.r, y.r), c(x.g, y.g), c(x.b, y.b), c(x.a, y.a)))
        }
        (Variant::Color(x), other) | (other, Variant::Color(x)) => {
            let k = other.to_number();
            let c = |p: f32| f(f64::from(p), k) as f32;
            Variant::Color(Color::rgba(c(x.r), c(x.g), c(x.b), x.a))
        }
        _ => Variant::Number(f(n(a), n(b))),
    }
}

fn safe_div(a: f64, b: f64) -> f64 {
    if b == 0.0 { 0.0 } else { a / b }
}

fn vdiv(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(safe_div(f64::from(a.x), f64::from(b.x)) as f32, safe_div(f64::from(a.y), f64::from(b.y)) as f32)
}

fn lerp_variant(a: &Variant, b: &Variant, t: f64) -> Variant {
    match (a, b) {
        (Variant::Vec2(x), Variant::Vec2(y)) => Variant::Vec2(x.lerp(*y, t as f32)),
        (Variant::Color(x), Variant::Color(y)) => Variant::Color(x.lerp(*y, t as f32)),
        _ => Variant::Number(n(a) + (n(b) - n(a)) * t),
    }
}

impl ExecCtx<'_> {
    pub(crate) fn node_arg(&self, v: &Variant) -> Option<NodeId> {
        self.tree.from_object(v.to_object()?)
    }

    fn obj(id: Option<NodeId>) -> Variant {
        id.map_or(Variant::Nil, |id| Variant::Object(SceneTree::object_id(id)))
    }

    fn with_node(&mut self, v: &Variant, f: impl FnOnce(&mut vd_scene::Node)) {
        if let Some(id) = self.node_arg(v) {
            if let Some(node) = self.tree.get_mut(id) {
                f(node);
            }
        }
    }
}

/// Executes `op`. `outs` has exactly as many slots as the node's outputs.
pub(crate) fn call(op: Op, a: &[Variant], outs: &mut [Variant], cx: &mut ExecCtx<'_>) {
    use Op::*;
    let out0 = |outs: &mut [Variant], v: Variant| {
        if let Some(slot) = outs.first_mut() {
            *slot = v;
        }
    };
    match op {
        Identity => out0(outs, a[0].clone()),
        Add => out0(outs, match (&a[0], &a[1]) {
            (Variant::String(x), y) => Variant::str(format!("{x}{y}")),
            (x, Variant::String(y)) => Variant::str(format!("{x}{y}")),
            (x, y) => arith(x, y, |p, q| p + q, |p, q| p + q),
        }),
        Sub => out0(outs, arith(&a[0], &a[1], |p, q| p - q, |p, q| p - q)),
        Mul => out0(outs, arith(&a[0], &a[1], |p, q| p * q, |p, q| p * q)),
        Div => out0(outs, arith(&a[0], &a[1], safe_div, vdiv)),
        Mod => out0(outs, Variant::Number(if n(&a[1]) == 0.0 { 0.0 } else { n(&a[0]).rem_euclid(n(&a[1])) })),
        Pow => out0(outs, Variant::Number(n(&a[0]).powf(n(&a[1])))),
        Min => out0(outs, Variant::Number(n(&a[0]).min(n(&a[1])))),
        Max => out0(outs, Variant::Number(n(&a[0]).max(n(&a[1])))),
        Clamp => {
            let (lo, hi) = (n(&a[1]), n(&a[2]));
            out0(outs, Variant::Number(if lo <= hi { n(&a[0]).clamp(lo, hi) } else { lo }));
        }
        Lerp => out0(outs, lerp_variant(&a[0], &a[1], n(&a[2]))),
        MapRange => {
            let t = safe_div(n(&a[0]) - n(&a[1]), n(&a[2]) - n(&a[1]));
            out0(outs, Variant::Number(n(&a[3]) + (n(&a[4]) - n(&a[3])) * t));
        }
        Abs => out0(outs, Variant::Number(n(&a[0]).abs())),
        Negate => out0(outs, match &a[0] {
            Variant::Vec2(v) => Variant::Vec2(-*v),
            other => Variant::Number(-n(other)),
        }),
        Sqrt => out0(outs, Variant::Number(n(&a[0]).max(0.0).sqrt())),
        Sin => out0(outs, Variant::Number(n(&a[0]).to_radians().sin())),
        Cos => out0(outs, Variant::Number(n(&a[0]).to_radians().cos())),
        Atan2 => out0(outs, Variant::Number(n(&a[0]).atan2(n(&a[1])).to_degrees())),
        Round => out0(outs, Variant::Number(n(&a[0]).round())),
        Floor => out0(outs, Variant::Number(n(&a[0]).floor())),
        Ceil => out0(outs, Variant::Number(n(&a[0]).ceil())),
        Sign => out0(outs, Variant::Number(if n(&a[0]) == 0.0 { 0.0 } else { n(&a[0]).signum() })),
        Wave => {
            let t = cx.host.time();
            out0(outs, Variant::Number((t * n(&a[0]) * std::f64::consts::TAU + n(&a[2]).to_radians()).sin() * n(&a[1])));
        }
        RandomRange => {
            let (lo, hi) = (a[0].to_f32(), a[1].to_f32());
            let v = cx.host.rng().range(lo, hi);
            out0(outs, Variant::Number(f64::from(v)));
        }
        RandomInt => {
            let v = cx.host.rng().range_i(n(&a[0]).round() as i64, n(&a[1]).round() as i64);
            out0(outs, Variant::Number(v as f64));
        }
        Chance => {
            let p = a[0].to_f32();
            let v = cx.host.rng().chance(p);
            out0(outs, Variant::Bool(v));
        }
        Time => out0(outs, Variant::Number(cx.host.time())),

        MakeVec => out0(outs, Variant::Vec2(Vec2::new(a[0].to_f32(), a[1].to_f32()))),
        BreakVec => {
            let v = a[0].to_vec2();
            outs[0] = Variant::Number(f64::from(v.x));
            outs[1] = Variant::Number(f64::from(v.y));
        }
        Length => out0(outs, Variant::Number(f64::from(a[0].to_vec2().length()))),
        Normalize => out0(outs, Variant::Vec2(a[0].to_vec2().normalize_or_zero())),
        Distance => out0(outs, Variant::Number(f64::from(a[0].to_vec2().distance(a[1].to_vec2())))),
        DirectionTo => out0(outs, Variant::Vec2((a[1].to_vec2() - a[0].to_vec2()).normalize_or_zero())),
        Dot => out0(outs, Variant::Number(f64::from(a[0].to_vec2().dot(a[1].to_vec2())))),
        FromAngle => {
            let r = a[0].to_f32().to_radians();
            out0(outs, Variant::Vec2(Vec2::new(r.cos(), r.sin()) * a[1].to_f32()));
        }
        Angle => {
            let v = a[0].to_vec2();
            out0(outs, Variant::Number(f64::from(v.y.atan2(v.x).to_degrees())));
        }
        RotateVec => out0(outs, Variant::Vec2(Vec2::from_angle(a[1].to_f32().to_radians()).rotate(a[0].to_vec2()))),
        MoveToward => {
            let (from, to, d) = (a[0].to_vec2(), a[1].to_vec2(), a[2].to_f32());
            let delta = to - from;
            let len = delta.length();
            out0(outs, Variant::Vec2(if len <= d || len < 1e-6 { to } else { from + delta / len * d }));
        }
        RandomInCircle => {
            let r = a[0].to_f32();
            let p = cx.host.rng().in_circle(r);
            out0(outs, Variant::Vec2(p));
        }

        Gt => out0(outs, Variant::Bool(n(&a[0]) > n(&a[1]))),
        Ge => out0(outs, Variant::Bool(n(&a[0]) >= n(&a[1]))),
        Lt => out0(outs, Variant::Bool(n(&a[0]) < n(&a[1]))),
        Le => out0(outs, Variant::Bool(n(&a[0]) <= n(&a[1]))),
        Eq => out0(outs, Variant::Bool(a[0].loose_eq(&a[1]))),
        Ne => out0(outs, Variant::Bool(!a[0].loose_eq(&a[1]))),
        And => out0(outs, Variant::Bool(a[0].to_bool() && a[1].to_bool())),
        Or => out0(outs, Variant::Bool(a[0].to_bool() || a[1].to_bool())),
        Not => out0(outs, Variant::Bool(!a[0].to_bool())),
        Select => out0(outs, if a[0].to_bool() { a[1].clone() } else { a[2].clone() }),

        Concat => out0(outs, Variant::str(format!("{}{}", a[0], a[1]))),
        Format => {
            let mut s = a[0].to_string();
            for (i, v) in a[1..].iter().enumerate() {
                s = s.replace(&format!("{{{i}}}"), &v.to_string());
            }
            out0(outs, Variant::str(s));
        }
        ToText => out0(outs, Variant::str(a[0].to_string())),
        MakeColor => out0(outs, Variant::Color(Color::rgba(a[0].to_f32(), a[1].to_f32(), a[2].to_f32(), a[3].to_f32()))),
        Hsv => out0(outs, Variant::Color(Color::from_hsv(a[0].to_f32(), a[1].to_f32(), a[2].to_f32(), a[3].to_f32()))),
        ArrayLen => out0(outs, Variant::Number(a[0].as_array().len() as f64)),
        ArrayGet => {
            let i = n(&a[1]);
            let arr = a[0].as_array();
            out0(outs, if i >= 0.0 { arr.get(i as usize).cloned().unwrap_or_default() } else { Variant::Nil });
        }
        ArrayRandom => {
            let arr = a[0].as_array();
            let v = if arr.is_empty() {
                Variant::Nil
            } else {
                let i = cx.host.rng().range_i(0, arr.len() as i64 - 1) as usize;
                arr[i].clone()
            };
            out0(outs, v);
        }

        SelfNode => out0(outs, ExecCtx::obj(Some(cx.self_id))),
        GetNode => {
            let from = cx.node_arg(&a[0]).unwrap_or(cx.self_id);
            let path = a[1].to_string();
            out0(outs, ExecCtx::obj(cx.tree.get_node(from, &path)));
        }
        GetParent => {
            let p = cx.node_arg(&a[0]).and_then(|id| cx.tree.parent(id));
            out0(outs, ExecCtx::obj(p));
        }
        FirstInGroup => out0(outs, ExecCtx::obj(cx.tree.first_in_group(StringName::new(&a[0].to_string())))),
        NodesInGroup => {
            let g = StringName::new(&a[0].to_string());
            let list: Vec<Variant> = cx
                .tree
                .nodes_in_group(g)
                .into_iter()
                .filter(|&id| cx.tree.get(id).is_some_and(|n| !n.is_queued_for_deletion()))
                .map(|id| ExecCtx::obj(Some(id)))
                .collect();
            out0(outs, Variant::Array(Arc::new(list)));
        }
        IsValid => {
            let ok = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).is_some_and(|n| !n.is_queued_for_deletion());
            out0(outs, Variant::Bool(ok));
        }
        IsInGroup => {
            let g = StringName::new(&a[1].to_string());
            let r = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).is_some_and(|n| n.in_group(g));
            out0(outs, Variant::Bool(r));
        }
        GetName => {
            let name = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).map(|n| n.name.clone()).unwrap_or_default();
            out0(outs, Variant::str(name));
        }
        GetPosition => {
            let p = cx.node_arg(&a[0]).map(|id| cx.tree.global_position(id)).unwrap_or_default();
            out0(outs, Variant::Vec2(p));
        }
        GetRotation => {
            let r = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).map_or(0.0, |n| n.transform.rotation);
            out0(outs, Variant::Number(f64::from(r.to_degrees())));
        }
        GetScale => {
            let s = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).map_or(Vec2::ONE, |n| n.transform.scale);
            out0(outs, Variant::Vec2(s));
        }
        NodeDistance => {
            let pa = cx.node_arg(&a[0]).map(|id| cx.tree.global_position(id));
            let pb = cx.node_arg(&a[1]).map(|id| cx.tree.global_position(id));
            let d = match (pa, pb) {
                (Some(x), Some(y)) => f64::from(x.distance(y)),
                _ => f64::INFINITY,
            };
            out0(outs, Variant::Number(d));
        }
        GetProperty => {
            let name = a[1].to_string();
            let v = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).and_then(|n| n.get(&name)).unwrap_or_default();
            out0(outs, v);
        }
        GetChild => {
            let c = cx.node_arg(&a[0]).and_then(|id| {
                let len = cx.tree.get_child_count(id) as i32;
                let i = a[1].to_number() as i32;
                let i = if i < 0 { i + len } else { i };
                (0..len).contains(&i).then(|| cx.tree.get_children(id)[i as usize])
            });
            out0(outs, ExecCtx::obj(c));
        }
        ChildCount => {
            let n = cx.node_arg(&a[0]).map_or(0, |id| cx.tree.get_child_count(id));
            out0(outs, Variant::Number(n as f64));
        }
        FindChild => {
            let c = cx.node_arg(&a[0]).and_then(|id| cx.tree.find_child(id, &a[1].to_string(), a[2].to_bool()));
            out0(outs, ExecCtx::obj(c));
        }
        Reparent => {
            if let (Some(n), Some(p)) = (cx.node_arg(&a[0]), cx.node_arg(&a[1]))
                && let Err(e) = cx.tree.reparent(n, p, true)
            {
                cx.host.print(&format!("Reparent failed: {e}"));
            }
        }

        ConnectSignal => {
            if let (Some(src), Some(dst)) = (cx.node_arg(&a[0]), cx.node_arg(&a[2])) {
                let flags = if a[4].to_bool() { vd_scene::connect_flags::ONE_SHOT } else { 0 };
                // Connecting twice is a no-op for designers (Godot would error).
                if !cx.tree.is_connected(src, &a[1].to_string(), dst, &a[3].to_string()) {
                    let _ = cx.tree.connect(src, &a[1].to_string(), dst, &a[3].to_string(), flags);
                }
            }
        }
        DisconnectSignal => {
            if let (Some(src), Some(dst)) = (cx.node_arg(&a[0]), cx.node_arg(&a[2])) {
                let _ = cx.tree.disconnect(src, &a[1].to_string(), dst, &a[3].to_string());
            }
        }
        EmitSignal => {
            if let Some(src) = cx.node_arg(&a[0]) {
                let args: Vec<Variant> = a[2..].iter().filter(|v| !v.is_nil()).cloned().collect();
                cx.tree.emit_signal(src, &a[1].to_string(), &args);
            }
        }
        IsConnected => {
            let r = match (cx.node_arg(&a[0]), cx.node_arg(&a[2])) {
                (Some(src), Some(dst)) => cx.tree.is_connected(src, &a[1].to_string(), dst, &a[3].to_string()),
                _ => false,
            };
            out0(outs, Variant::Bool(r));
        }

        SetPosition => {
            if let Some(id) = cx.node_arg(&a[0]) {
                cx.tree.set_global_position(id, a[1].to_vec2());
            }
        }
        Translate => cx.with_node(&a[0], |n| n.transform.position += a[1].to_vec2()),
        SetRotation => cx.with_node(&a[0], |n| n.transform.rotation = a[1].to_f32().to_radians()),
        Rotate => cx.with_node(&a[0], |n| n.transform.rotation += a[1].to_f32().to_radians()),
        LookAt => {
            if let Some(id) = cx.node_arg(&a[0]) {
                let from = cx.tree.global_position(id);
                let d = a[1].to_vec2() - from;
                if d.length_squared() > 1e-8 {
                    let parent_rot = cx
                        .tree
                        .parent(id)
                        .map_or(0.0, |p| vd_core::Transform2D::from_affine(&cx.tree.compute_global(p)).rotation);
                    if let Some(node) = cx.tree.get_mut(id) {
                        node.transform.rotation = d.y.atan2(d.x) - parent_rot;
                    }
                }
            }
        }
        SetScale => cx.with_node(&a[0], |n| n.transform.scale = a[1].to_vec2()),
        SetVisible => cx.with_node(&a[0], |n| n.visible = a[1].to_bool()),
        SetProperty => {
            let name = a[1].to_string();
            cx.with_node(&a[0], |n| {
                n.set(&name, &a[2]);
            });
        }
        SetText => cx.with_node(&a[0], |n| {
            n.set("text", &a[1]);
        }),
        SetColor => cx.with_node(&a[0], |n| {
            let has_color = n.property_list().iter().any(|p| p.name == "color");
            n.set(if has_color { "color" } else { "modulate" }, &a[1]);
        }),
        AddToGroup => {
            let g = StringName::new(&a[1].to_string());
            cx.with_node(&a[0], |n| {
                if !n.in_group(g) {
                    n.groups.push(g);
                }
            });
        }
        QueueFree => {
            if let Some(id) = cx.node_arg(&a[0]) {
                cx.tree.queue_free(id);
            }
        }
        Spawn => {
            let parent = cx.node_arg(&a[2]);
            let id = cx.host.spawn_scene(cx.tree, &a[0].to_string(), parent, a[1].to_vec2());
            out0(outs, ExecCtx::obj(id));
        }

        SetVelocity => cx.with_node(&a[0], |n| {
            if let NodeKind::Body2D(b) = &mut n.kind {
                b.velocity = a[1].to_vec2();
            }
        }),
        GetVelocity => {
            let v = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).map_or(Vec2::ZERO, |n| match &n.kind {
                NodeKind::Body2D(b) => b.velocity,
                _ => Vec2::ZERO,
            });
            out0(outs, Variant::Vec2(v));
        }
        ApplyImpulse => cx.with_node(&a[0], |n| {
            if let NodeKind::Body2D(b) = &mut n.kind {
                b.velocity += a[1].to_vec2() / b.mass.max(0.001);
            }
        }),
        IsOnFloor => {
            let r = cx.node_arg(&a[0]).and_then(|id| cx.tree.get(id)).is_some_and(|n| match &n.kind {
                NodeKind::Body2D(b) => b.on_floor,
                _ => false,
            });
            out0(outs, Variant::Bool(r));
        }

        ActionPressed => out0(outs, Variant::Bool(cx.host.action_pressed(StringName::new(&a[0].to_string())))),
        ActionJustPressed => {
            out0(outs, Variant::Bool(cx.host.action_just_pressed(StringName::new(&a[0].to_string()))));
        }
        Axis => {
            let neg = cx.host.action_pressed(StringName::new(&a[0].to_string()));
            let pos = cx.host.action_pressed(StringName::new(&a[1].to_string()));
            out0(outs, Variant::Number(f64::from(i8::from(pos) - i8::from(neg))));
        }
        InputVector => {
            let p = |cx: &ExecCtx<'_>, v: &Variant| f32::from(u8::from(cx.host.action_pressed(StringName::new(&v.to_string()))));
            let v = Vec2::new(p(cx, &a[1]) - p(cx, &a[0]), p(cx, &a[3]) - p(cx, &a[2]));
            out0(outs, Variant::Vec2(v.normalize_or_zero()));
        }
        MousePosition => out0(outs, Variant::Vec2(cx.host.mouse_world())),

        Print => {
            let msg = a[0].to_string();
            let who = cx.tree.get(cx.self_id).map(|n| n.name.clone()).unwrap_or_default();
            cx.host.print(&format!("[{who}] {msg}"));
        }
        EmitEvent => {
            let target = cx.node_arg(&a[2]);
            cx.host.emit_event(StringName::new(&a[0].to_string()), a[1].clone(), cx.self_id, target);
        }
        ChangeScene => cx.host.change_scene(Some(&a[0].to_string())),
        ReloadScene => cx.host.change_scene(None),
        CameraShake => cx.host.camera_shake(a[0].to_f32()),
        HitStop => cx.host.hit_stop(a[0].to_f32()),
        TimeScale => cx.host.set_time_scale(a[0].to_f32()),
        SetEnv => {
            let prop = a[0].to_string();
            let env = cx.tree.all_nodes().into_iter().find(|&id| {
                matches!(cx.tree.get(id).map(|n| &n.kind), Some(NodeKind::WorldEnvironment(_)))
            });
            if let Some(n) = env.and_then(|id| cx.tree.get_mut(id)) {
                n.set(&prop, &a[1]);
            }
        }
        Flash => cx.host.flash(a[0].to_color(), a[1].to_f32()),
        Burst => cx.with_node(&a[0], |n| {
            if let NodeKind::Particles2D(p) = &mut n.kind {
                p.burst = a[1].to_number().max(0.0) as u32;
                p.burst_pending = true;
            }
        }),
        TimerStart => cx.with_node(&a[0], |n| {
            if let NodeKind::Timer(t) = &mut n.kind {
                let w = a[1].to_f32();
                if w > 0.0 {
                    t.wait_time = w;
                }
                t.time_left = t.wait_time;
                t.running = true;
                t.initialized = true;
            }
        }),
        TimerStop => cx.with_node(&a[0], |n| {
            if let NodeKind::Timer(t) = &mut n.kind {
                t.running = false;
                t.initialized = true;
            }
        }),
        Quit => cx.host.quit(),
        SetPaused => cx.tree.set_pause(a[0].to_bool()),
        IsPaused => out0(outs, Variant::Bool(cx.tree.is_paused())),
    }
}
