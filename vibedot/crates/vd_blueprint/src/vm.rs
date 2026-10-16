//! The blueprint virtual machine: a small register VM. Each scene node with a
//! blueprint owns an [`Instance`] (registers, variables, latent delays); the
//! compiled [`Program`] is shared between all instances via `Arc`.

use std::sync::Arc;

use vd_core::{Color, Rng, StringName, Variant, Vec2};
use vd_scene::{NodeId, SceneTree};

use crate::compiler::{ConstVal, Instr, Program};
use crate::defs::EventKind;
use crate::graph::BpNodeId;
use crate::ops;

/// Services the engine provides to blueprints.
pub trait Host {
    fn time(&self) -> f64;
    fn rng(&mut self) -> &mut Rng;
    fn action_pressed(&self, action: StringName) -> bool;
    fn action_just_pressed(&self, action: StringName) -> bool;
    fn mouse_world(&self) -> Vec2;
    fn spawn_scene(&mut self, tree: &mut SceneTree, path: &str, parent: Option<NodeId>, pos: Vec2) -> Option<NodeId>;
    fn print(&mut self, msg: &str);
    fn emit_event(&mut self, name: StringName, payload: Variant, sender: NodeId, target: Option<NodeId>);
    /// `None` = reload current scene.
    fn change_scene(&mut self, path: Option<&str>);
    fn camera_shake(&mut self, amount: f32);
    fn hit_stop(&mut self, seconds: f32);
    fn set_time_scale(&mut self, scale: f32);
    fn flash(&mut self, color: Color, duration: f32);
    fn quit(&mut self);
}

/// Everything a running instruction may touch.
pub struct ExecCtx<'a> {
    pub tree: &'a mut SceneTree,
    pub host: &'a mut dyn Host,
    pub self_id: NodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RuntimeError {
    pub node: Option<BpNodeId>,
    pub message: String,
}

/// Instruction budget per execution - turns an accidental infinite
/// `While` loop into an error instead of a frozen editor.
pub const MAX_STEPS: u32 = 2_000_000;

#[derive(Clone, Debug)]
struct PendingDelay {
    remaining: f32,
    pc: u32,
}

#[derive(Clone, Debug)]
pub struct Instance {
    program: Arc<Program>,
    pub node: NodeId,
    regs: Vec<Variant>,
    pub vars: Vec<Variant>,
    flags: Vec<bool>,
    delays: Vec<PendingDelay>,
    args: Vec<Variant>,
    outs: Vec<Variant>,
    /// Number of instructions executed in total (profiling).
    pub steps: u64,
}

impl Instance {
    /// Creates an instance. `override_var` lets the owning node override
    /// exposed variable defaults (per-instance tweaks in the inspector).
    pub fn new(program: Arc<Program>, node: NodeId, override_var: impl Fn(&str) -> Option<Variant>) -> Self {
        let mut regs = vec![Variant::Nil; program.num_regs as usize];
        let self_obj = Variant::Object(SceneTree::object_id(node));
        for (r, c) in &program.consts {
            regs[*r as usize] = match c {
                ConstVal::Value(v) => v.clone(),
                ConstVal::SelfNode => self_obj.clone(),
            };
        }
        let vars = program
            .variables
            .iter()
            .map(|v| override_var(&v.name).map_or_else(|| v.default.clone(), |o| o.coerce(v.ty)))
            .collect();
        Self {
            flags: vec![false; program.num_flags as usize],
            program,
            node,
            regs,
            vars,
            delays: Vec::new(),
            args: Vec::with_capacity(8),
            outs: Vec::with_capacity(4),
            steps: 0,
        }
    }

    pub fn program(&self) -> &Arc<Program> {
        &self.program
    }

    pub fn var(&self, name: &str) -> Option<&Variant> {
        let i = self.program.variables.iter().position(|v| v.name == name)?;
        self.vars.get(i)
    }

    pub fn handles(&self, kind: EventKind) -> bool {
        self.program.has_event(kind)
    }

    /// Fires every handler of `kind` whose filter matches `key`.
    /// Returns how many handlers ran.
    pub fn trigger(
        &mut self,
        kind: EventKind,
        key: StringName,
        args: &[Variant],
        tree: &mut SceneTree,
        host: &mut dyn Host,
    ) -> Result<usize, RuntimeError> {
        let program = Arc::clone(&self.program);
        let mut ran = 0;
        for ev in program.events.iter().filter(|e| e.kind == kind) {
            if !ev.filter.is_empty() && ev.filter != key {
                continue;
            }
            for (reg, val) in ev.outs.iter().zip(args) {
                self.regs[*reg as usize] = val.clone();
            }
            self.run(ev.entry, tree, host)?;
            ran += 1;
        }
        Ok(ran)
    }

    /// Advances latent `Delay` nodes and resumes the ones that are due.
    pub fn update_delays(&mut self, dt: f32, tree: &mut SceneTree, host: &mut dyn Host) -> Result<(), RuntimeError> {
        if self.delays.is_empty() {
            return Ok(());
        }
        let mut due = Vec::new();
        self.delays.retain_mut(|d| {
            d.remaining -= dt;
            if d.remaining <= 0.0 {
                due.push(d.pc);
                false
            } else {
                true
            }
        });
        for pc in due {
            self.run(pc, tree, host)?;
        }
        Ok(())
    }

    pub fn pending_delays(&self) -> usize {
        self.delays.len()
    }

    fn run(&mut self, mut pc: u32, tree: &mut SceneTree, host: &mut dyn Host) -> Result<(), RuntimeError> {
        let program = Arc::clone(&self.program);
        let code = &program.code;
        let mut cx = ExecCtx { tree, host, self_id: self.node };
        let mut budget = MAX_STEPS;
        loop {
            budget -= 1;
            if budget == 0 {
                return Err(RuntimeError {
                    node: program.source_map.get(pc as usize).copied().flatten(),
                    message: "execution took too long (infinite loop?)".into(),
                });
            }
            self.steps += 1;
            let Some(&ins) = code.get(pc as usize) else {
                return Err(RuntimeError { node: None, message: format!("program counter {pc} out of range") });
            };
            pc += 1;
            match ins {
                Instr::Call { op, args, nargs, outs, nouts } => {
                    let (a0, o0) = (args as usize, outs as usize);
                    self.args.clear();
                    self.args.extend(program.pool[a0..a0 + nargs as usize].iter().map(|&r| self.regs[r as usize].clone()));
                    self.outs.clear();
                    self.outs.resize(nouts as usize, Variant::Nil);
                    ops::call(op, &self.args, &mut self.outs, &mut cx);
                    for (i, &r) in program.pool[o0..o0 + nouts as usize].iter().enumerate() {
                        self.regs[r as usize] = std::mem::take(&mut self.outs[i]);
                    }
                }
                Instr::Move { dst, src } => self.regs[dst as usize] = self.regs[src as usize].clone(),
                Instr::Jump { target } => pc = target,
                Instr::JumpIfNot { cond, target } => {
                    if !self.regs[cond as usize].to_bool() {
                        pc = target;
                    }
                }
                Instr::ForCheck { idx, last, exit } => {
                    if self.regs[idx as usize].to_number() > self.regs[last as usize].to_number() {
                        pc = exit;
                    }
                }
                Instr::Incr { reg } => {
                    let v = self.regs[reg as usize].to_number() + 1.0;
                    self.regs[reg as usize] = Variant::Number(v);
                }
                Instr::ArrayIter { array, idx, elem, exit } => {
                    let i = self.regs[idx as usize].to_number() as usize;
                    match self.regs[array as usize].as_array().get(i).cloned() {
                        Some(v) => self.regs[elem as usize] = v,
                        None => pc = exit,
                    }
                }
                Instr::GetVar { dst, var } => self.regs[dst as usize] = self.vars[var as usize].clone(),
                Instr::SetVar { var, src } => {
                    let ty = program.variables[var as usize].ty;
                    self.vars[var as usize] = self.regs[src as usize].coerce(ty);
                }
                Instr::AddVar { var, src, dst } => {
                    let cur = &self.vars[var as usize];
                    let amt = &self.regs[src as usize];
                    let new = match cur {
                        Variant::Vec2(v) => Variant::Vec2(*v + amt.to_vec2()),
                        Variant::String(s) => Variant::str(format!("{s}{amt}")),
                        Variant::Array(a) => {
                            let mut v = (**a).clone();
                            v.push(amt.clone());
                            Variant::Array(Arc::new(v))
                        }
                        other => Variant::Number(other.to_number() + amt.to_number()),
                    };
                    self.regs[dst as usize] = new.clone();
                    self.vars[var as usize] = new;
                }
                Instr::TestSetFlag { flag, skip } => {
                    let f = &mut self.flags[flag as usize];
                    if *f {
                        pc = skip;
                    } else {
                        *f = true;
                    }
                }
                Instr::FlipFlop { flag, is_a, b } => {
                    // First run goes to A (flag false -> A, then set true).
                    let f = &mut self.flags[flag as usize];
                    let go_a = !*f;
                    *f = !*f;
                    self.regs[is_a as usize] = Variant::Bool(go_a);
                    if !go_a {
                        pc = b;
                    }
                }
                Instr::Delay { seconds, resume } => {
                    let s = self.regs[seconds as usize].to_f32().max(0.0);
                    self.delays.push(PendingDelay { remaining: s, pc: resume });
                }
                Instr::Return => return Ok(()),
            }
        }
    }
}

/// A host with no engine behind it - for tests, tools and headless runs.
#[derive(Debug, Default)]
pub struct NullHost {
    pub log: Vec<String>,
    pub time: f64,
    pub rng: Rng,
    pub pressed: Vec<StringName>,
    pub events: Vec<(StringName, Variant)>,
}

impl Host for NullHost {
    fn time(&self) -> f64 {
        self.time
    }
    fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }
    fn action_pressed(&self, a: StringName) -> bool {
        self.pressed.contains(&a)
    }
    fn action_just_pressed(&self, a: StringName) -> bool {
        self.pressed.contains(&a)
    }
    fn mouse_world(&self) -> Vec2 {
        Vec2::ZERO
    }
    fn spawn_scene(&mut self, _: &mut SceneTree, _: &str, _: Option<NodeId>, _: Vec2) -> Option<NodeId> {
        None
    }
    fn print(&mut self, msg: &str) {
        self.log.push(msg.to_owned());
    }
    fn emit_event(&mut self, name: StringName, payload: Variant, _: NodeId, _: Option<NodeId>) {
        self.events.push((name, payload));
    }
    fn change_scene(&mut self, _: Option<&str>) {}
    fn camera_shake(&mut self, _: f32) {}
    fn hit_stop(&mut self, _: f32) {}
    fn set_time_scale(&mut self, _: f32) {}
    fn flash(&mut self, _: Color, _: f32) {}
    fn quit(&mut self) {}
}
