//! Blueprint compiler: graph → register-based bytecode.
//!
//! Semantics follow Unreal blueprints:
//! * Execution flows along exec wires starting at event nodes.
//! * Pure nodes are evaluated lazily, once per *statement* (an impure node
//!   reading its inputs), so loops see fresh values on every iteration.
//! * Outputs of impure nodes keep the value from their last execution.
//!
//! Every data output pin owns a register; literals get constant registers
//! that are loaded when an instance is created.

use std::collections::{HashMap, HashSet};

use vd_core::{StringName, Variant};

use crate::defs::{EventKind, Flavor, FlowKind, Lit, NodeDef, PinDef, PinKind, VarKind};
use crate::graph::{Blueprint, BpNode, BpNodeId, PinRef, VarDef};
use crate::ops::Op;

/// Register index. 16 bits keep instructions at 12 bytes.
pub type Reg = u16;

/// One bytecode instruction (12 bytes, `Copy`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Instr {
    /// `outs[..] = op(args[..])`; arg/out register lists live in `Program::pool`.
    Call {
        op: Op,
        args: u32,
        nargs: u8,
        outs: u32,
        nouts: u8,
    },
    Move {
        dst: Reg,
        src: Reg,
    },
    Jump {
        target: u32,
    },
    JumpIfNot {
        cond: Reg,
        target: u32,
    },
    /// `if idx > last { goto exit }`
    ForCheck {
        idx: Reg,
        last: Reg,
        exit: u32,
    },
    /// `reg += 1`
    Incr {
        reg: Reg,
    },
    /// `if idx >= len(array) { goto exit } else { elem = array[idx] }`
    ArrayIter {
        array: Reg,
        idx: Reg,
        elem: Reg,
        exit: u32,
    },
    GetVar {
        dst: Reg,
        var: u16,
    },
    SetVar {
        var: u16,
        src: Reg,
    },
    AddVar {
        var: u16,
        src: Reg,
        dst: Reg,
    },
    /// Do Once: `if flag { goto skip } else { flag = true }`
    TestSetFlag {
        flag: u16,
        skip: u32,
    },
    /// Flip Flop: toggles `flag`; writes `is_a`; jumps to `b` when on B.
    FlipFlop {
        flag: u16,
        is_a: Reg,
        b: u32,
    },
    /// Schedule `resume` after `seconds` (latent), then continue.
    Delay {
        seconds: Reg,
        resume: u32,
    },
    Return,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstVal {
    Value(Variant),
    /// Resolved to the owning node when the instance is created.
    SelfNode,
}

#[derive(Clone, Debug)]
pub struct CompiledEvent {
    pub kind: EventKind,
    /// Action / key / event / timer name (empty = any).
    pub filter: StringName,
    pub entry: u32,
    /// Registers receiving the event's data outputs, in pin order.
    pub outs: Vec<Reg>,
    pub node: BpNodeId,
}

/// Executable form of a blueprint, shared (`Arc`) by all instances.
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub code: Vec<Instr>,
    pub pool: Vec<Reg>,
    pub consts: Vec<(Reg, ConstVal)>,
    pub num_regs: u32,
    pub num_flags: u16,
    pub variables: Vec<VarDef>,
    pub events: Vec<CompiledEvent>,
    /// Instruction index → graph node, for runtime error reporting.
    pub source_map: Vec<Option<BpNodeId>>,
}

impl Program {
    pub fn has_event(&self, kind: EventKind) -> bool {
        self.events.iter().any(|e| e.kind == kind)
    }

    /// Human-readable disassembly (shown in the editor's "Bytecode" view).
    pub fn disassemble(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        for e in &self.events {
            let _ = writeln!(s, "; event {:?} {:?} -> @{}", e.kind, e.filter.as_str(), e.entry);
        }
        for (i, ins) in self.code.iter().enumerate() {
            let _ = match ins {
                Instr::Call { op, args, nargs, outs, nouts } => writeln!(
                    s,
                    "{i:04}  call {op:?} ({:?}) -> {:?}",
                    &self.pool[*args as usize..*args as usize + *nargs as usize],
                    &self.pool[*outs as usize..*outs as usize + *nouts as usize]
                ),
                other => writeln!(s, "{i:04}  {other:?}"),
            };
        }
        s
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub node: Option<BpNodeId>,
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.node {
            Some(n) => write!(f, "node {}: {}", n.0, self.message),
            None => f.write_str(&self.message),
        }
    }
}

const MAX_CODE: usize = 200_000;

struct Compiler<'a> {
    bp: &'a Blueprint,
    nodes: HashMap<BpNodeId, (&'a BpNode, &'static NodeDef)>,
    prog: Program,
    out_regs: HashMap<(BpNodeId, &'static str), Reg>,
    errors: Vec<CompileError>,
    exec_stack: Vec<BpNodeId>,
    evaluating: Vec<BpNodeId>,
    memo: HashSet<BpNodeId>,
    flags: HashMap<BpNodeId, u16>,
    current: Option<BpNodeId>,
}

pub fn compile(bp: &Blueprint) -> Result<Program, Vec<CompileError>> {
    let mut c = Compiler {
        bp,
        nodes: HashMap::new(),
        prog: Program { variables: bp.variables.clone(), ..Default::default() },
        out_regs: HashMap::new(),
        errors: Vec::new(),
        exec_stack: Vec::new(),
        evaluating: Vec::new(),
        memo: HashSet::new(),
        flags: HashMap::new(),
        current: None,
    };
    c.run();
    if c.errors.is_empty() { Ok(c.prog) } else { Err(c.errors) }
}

impl<'a> Compiler<'a> {
    fn err(&mut self, node: Option<BpNodeId>, msg: impl Into<String>) {
        let e = CompileError { node, message: msg.into() };
        if !self.errors.contains(&e) {
            self.errors.push(e);
        }
    }

    fn emit(&mut self, i: Instr) -> u32 {
        let at = self.prog.code.len() as u32;
        self.prog.code.push(i);
        self.prog.source_map.push(self.current);
        at
    }

    fn here(&self) -> u32 {
        self.prog.code.len() as u32
    }

    fn patch(&mut self, at: u32, target: u32) {
        match &mut self.prog.code[at as usize] {
            Instr::Jump { target: t }
            | Instr::JumpIfNot { target: t, .. }
            | Instr::ForCheck { exit: t, .. }
            | Instr::ArrayIter { exit: t, .. }
            | Instr::TestSetFlag { skip: t, .. }
            | Instr::FlipFlop { b: t, .. }
            | Instr::Delay { resume: t, .. } => *t = target,
            other => unreachable!("patching non-jump {other:?}"),
        }
    }

    fn new_reg(&mut self) -> Reg {
        let r = self.prog.num_regs;
        if r >= u32::from(Reg::MAX) {
            self.err(None, "blueprint needs too many registers - split it into smaller blueprints");
            return 0;
        }
        self.prog.num_regs += 1;
        r as Reg
    }

    fn const_reg(&mut self, v: ConstVal) -> Reg {
        let r = self.new_reg();
        self.prog.consts.push((r, v));
        r
    }

    fn out_reg(&mut self, node: BpNodeId, pin: &'static str) -> Reg {
        if let Some(&r) = self.out_regs.get(&(node, pin)) {
            return r;
        }
        let r = self.new_reg();
        self.out_regs.insert((node, pin), r);
        r
    }

    fn var_index(&mut self, node: BpNodeId, n: &BpNode) -> Option<u16> {
        let name = n.literal("var").to_string();
        match self.bp.variables.iter().position(|v| v.name == name) {
            Some(i) => Some(i as u16),
            None => {
                let msg = if name.is_empty() { "choose a variable".to_owned() } else { format!("unknown variable '{name}'") };
                self.err(Some(node), msg);
                None
            }
        }
    }

    fn run(&mut self) {
        for n in &self.bp.nodes {
            match n.def() {
                Some(d) => {
                    self.nodes.insert(n.id, (n, d));
                }
                None => self.errors.push(CompileError { node: Some(n.id), message: format!("unknown node type '{}'", n.op) }),
            }
        }
        // Validate links up-front so the editor can flag every broken wire.
        for l in &self.bp.links {
            if !self.nodes.contains_key(&l.from.node) || !self.nodes.contains_key(&l.to.node) {
                self.err(None, "link references a missing node");
                continue;
            }
            let fk = self.bp.pin_kind(l.from.node, &l.from.pin, true);
            let tk = self.bp.pin_kind(l.to.node, &l.to.pin, false);
            match (fk, tk) {
                (Some(PinKind::Exec), Some(PinKind::Exec)) => {}
                (Some(PinKind::Data(a)), Some(PinKind::Data(b))) if b.accepts(a) => {}
                (Some(PinKind::Data(a)), Some(PinKind::Data(b))) => {
                    self.err(Some(l.to.node), format!("cannot feed {} into {} pin '{}'", a.name(), b.name(), l.to.pin));
                }
                _ => self.err(Some(l.to.node), format!("invalid link to pin '{}'", l.to.pin)),
            }
        }
        if !self.errors.is_empty() {
            return;
        }

        let mut events: Vec<(&BpNode, &NodeDef)> = self.nodes.values().filter(|(_, d)| d.is_event()).map(|&(n, d)| (n, d)).collect();
        events.sort_by_key(|(n, _)| n.id);
        for (n, d) in events {
            let Flavor::Event(kind) = d.flavor else { unreachable!() };
            self.current = Some(n.id);
            let filter = d
                .inputs
                .iter()
                .find(|p| p.literal_only)
                .map_or(StringName::EMPTY, |p| StringName::new(n.literal(p.name).to_string().trim()));
            let outs: Vec<Reg> = d.outputs.iter().filter(|p| !p.is_exec()).map(|p| self.out_reg(n.id, p.name)).collect();
            let entry = self.here();
            self.exec_stack.push(n.id);
            self.chain(n.id, "then");
            self.exec_stack.pop();
            self.emit(Instr::Return);
            self.prog.events.push(CompiledEvent { kind, filter, entry, outs, node: n.id });
        }
        if self.prog.code.len() > MAX_CODE {
            self.err(None, "blueprint is too large (too many merged execution paths)");
        }
    }

    /// Follows the exec wire leaving `node.pin`.
    fn chain(&mut self, node: BpNodeId, pin: &str) {
        if self.prog.code.len() > MAX_CODE {
            return;
        }
        if let Some(target) = self.bp.exec_target(&PinRef::new(node, pin)) {
            let t = target.node;
            self.exec_node(t);
        }
    }

    fn exec_node(&mut self, id: BpNodeId) {
        let Some(&(node, def)) = self.nodes.get(&id) else {
            return;
        };
        if self.exec_stack.contains(&id) {
            self.err(Some(id), "execution wires form a loop - use a For/While loop node instead");
            return;
        }
        self.exec_stack.push(id);
        let saved = self.current.replace(id);
        self.memo.clear();
        match def.flavor {
            Flavor::Exec(op) => {
                self.emit_call(node, def, op);
                self.chain(id, "then");
            }
            Flavor::Flow(flow) => self.flow(node, def, flow),
            Flavor::Var(VarKind::Set) => {
                if let Some(var) = self.var_index(id, node) {
                    let src = self.input(node, def.input("value").expect("pin"));
                    self.emit(Instr::SetVar { var, src });
                    let out = self.out_reg(id, "value");
                    self.emit(Instr::Move { dst: out, src });
                }
                self.chain(id, "then");
            }
            Flavor::Var(VarKind::Add) => {
                if let Some(var) = self.var_index(id, node) {
                    let src = self.input(node, def.input("amount").expect("pin"));
                    let dst = self.out_reg(id, "value");
                    self.emit(Instr::AddVar { var, src, dst });
                }
                self.chain(id, "then");
            }
            Flavor::Var(VarKind::Get) | Flavor::Pure(_) | Flavor::Event(_) => {
                self.err(Some(id), "this node has no execution input");
            }
        }
        self.current = saved;
        self.exec_stack.pop();
    }

    fn flow(&mut self, node: &BpNode, def: &'static NodeDef, flow: FlowKind) {
        let id = node.id;
        match flow {
            FlowKind::Branch => {
                let cond = self.input(node, def.input("condition").expect("pin"));
                let jf = self.emit(Instr::JumpIfNot { cond, target: 0 });
                self.chain(id, "true");
                let jend = self.emit(Instr::Jump { target: 0 });
                let else_at = self.here();
                self.patch(jf, else_at);
                self.chain(id, "false");
                let end = self.here();
                self.patch(jend, end);
            }
            FlowKind::Sequence => {
                for p in def.outputs {
                    self.memo.clear();
                    self.chain(id, p.name);
                }
            }
            FlowKind::For => {
                let first = self.input(node, def.input("first").expect("pin"));
                let last_src = self.input(node, def.input("last").expect("pin"));
                let last = self.new_reg();
                self.emit(Instr::Move { dst: last, src: last_src });
                let idx = self.out_reg(id, "index");
                self.emit(Instr::Move { dst: idx, src: first });
                let head = self.here();
                let check = self.emit(Instr::ForCheck { idx, last, exit: 0 });
                self.chain(id, "body");
                self.emit(Instr::Incr { reg: idx });
                self.emit(Instr::Jump { target: head });
                let exit = self.here();
                self.patch(check, exit);
                self.memo.clear();
                self.chain(id, "completed");
            }
            FlowKind::ForEach => {
                let src = self.input(node, def.input("array").expect("pin"));
                let array = self.new_reg();
                self.emit(Instr::Move { dst: array, src });
                let idx = self.out_reg(id, "index");
                let zero = self.const_reg(ConstVal::Value(Variant::Number(0.0)));
                self.emit(Instr::Move { dst: idx, src: zero });
                let elem = self.out_reg(id, "element");
                let head = self.here();
                let it = self.emit(Instr::ArrayIter { array, idx, elem, exit: 0 });
                self.chain(id, "body");
                self.emit(Instr::Incr { reg: idx });
                self.emit(Instr::Jump { target: head });
                let exit = self.here();
                self.patch(it, exit);
                self.memo.clear();
                self.chain(id, "completed");
            }
            FlowKind::While => {
                let head = self.here();
                self.memo.clear();
                let cond = self.input(node, def.input("condition").expect("pin"));
                let jf = self.emit(Instr::JumpIfNot { cond, target: 0 });
                self.chain(id, "body");
                self.emit(Instr::Jump { target: head });
                let exit = self.here();
                self.patch(jf, exit);
                self.memo.clear();
                self.chain(id, "completed");
            }
            FlowKind::DoOnce => {
                let flag = self.flag(id);
                let t = self.emit(Instr::TestSetFlag { flag, skip: 0 });
                self.chain(id, "then");
                let end = self.here();
                self.patch(t, end);
            }
            FlowKind::FlipFlop => {
                let flag = self.flag(id);
                let is_a = self.out_reg(id, "is_a");
                let ff = self.emit(Instr::FlipFlop { flag, is_a, b: 0 });
                self.chain(id, "a");
                let jend = self.emit(Instr::Jump { target: 0 });
                let b_at = self.here();
                self.patch(ff, b_at);
                self.chain(id, "b");
                let end = self.here();
                self.patch(jend, end);
            }
            FlowKind::Delay => {
                let seconds = self.input(node, def.input("seconds").expect("pin"));
                let d = self.emit(Instr::Delay { seconds, resume: 0 });
                let skip = self.emit(Instr::Jump { target: 0 });
                let resume = self.here();
                self.patch(d, resume);
                // The continuation runs later as its own execution: fresh
                // statement context, and it must not inherit the loop stack.
                let saved_stack = std::mem::take(&mut self.exec_stack);
                self.exec_stack.push(id);
                self.memo.clear();
                self.chain(id, "completed");
                self.emit(Instr::Return);
                self.exec_stack = saved_stack;
                let after = self.here();
                self.patch(skip, after);
            }
        }
    }

    fn flag(&mut self, id: BpNodeId) -> u16 {
        if let Some(&f) = self.flags.get(&id) {
            return f;
        }
        let f = self.prog.num_flags;
        self.prog.num_flags += 1;
        self.flags.insert(id, f);
        f
    }

    fn emit_call(&mut self, node: &BpNode, def: &'static NodeDef, op: Op) {
        let args: Vec<Reg> = def.inputs.iter().filter(|p| !p.is_exec()).map(|p| self.input(node, p)).collect();
        let outs: Vec<Reg> = def.outputs.iter().filter(|p| !p.is_exec()).map(|p| self.out_reg(node.id, p.name)).collect();
        let args_at = self.prog.pool.len() as u32;
        self.prog.pool.extend(&args);
        let outs_at = self.prog.pool.len() as u32;
        self.prog.pool.extend(&outs);
        self.emit(Instr::Call { op, args: args_at, nargs: args.len() as u8, outs: outs_at, nouts: outs.len() as u8 });
    }

    /// Returns the register holding the value of a data input pin.
    fn input(&mut self, node: &BpNode, pin: &PinDef) -> Reg {
        let pref = PinRef::new(node.id, pin.name);
        if let Some(src) = self.bp.source_of(&pref).cloned() {
            let Some(&(src_node, src_def)) = self.nodes.get(&src.node) else {
                return self.const_reg(ConstVal::Value(Variant::Nil));
            };
            let Some(src_pin) = src_def.output(&src.pin) else {
                self.err(Some(node.id), format!("pin '{}' links to a missing output '{}'", pin.name, src.pin));
                return self.const_reg(ConstVal::Value(Variant::Nil));
            };
            if src_def.is_pure() {
                self.eval_pure(src_node, src_def);
            }
            return self.out_reg(src.node, src_pin.name);
        }
        // Unconnected: literal value.
        let explicit = node.values.get(pin.name);
        let cv = match (explicit, pin.default) {
            (None, Lit::SelfNode) => ConstVal::SelfNode,
            (Some(v), _) => ConstVal::Value(v.coerce(pin.vtype())),
            (None, lit) => ConstVal::Value(lit.to_variant()),
        };
        self.const_reg(cv)
    }

    fn eval_pure(&mut self, node: &BpNode, def: &'static NodeDef) {
        if self.memo.contains(&node.id) {
            return;
        }
        if self.evaluating.contains(&node.id) {
            self.err(Some(node.id), "data wires form a cycle");
            return;
        }
        self.evaluating.push(node.id);
        let saved = self.current.replace(node.id);
        match def.flavor {
            Flavor::Pure(op) => self.emit_call(node, def, op),
            Flavor::Var(VarKind::Get) => {
                if let Some(var) = self.var_index(node.id, node) {
                    let dst = self.out_reg(node.id, "value");
                    self.emit(Instr::GetVar { dst, var });
                }
            }
            _ => unreachable!("eval_pure on impure node"),
        }
        self.current = saved;
        self.evaluating.pop();
        self.memo.insert(node.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instr_is_compact() {
        assert!(size_of::<Instr>() <= 12, "Instr is {} bytes", size_of::<Instr>());
    }

    #[test]
    fn reports_exec_loops_and_unknown_vars() {
        let mut bp = Blueprint::new();
        let ev = bp.add("event.ready", (0.0, 0.0), &[]);
        let a = bp.add("debug.print", (200.0, 0.0), &[]);
        let b = bp.add("var.set", (400.0, 0.0), &[("var", "missing".into())]);
        bp.connect(PinRef::new(ev, "then"), PinRef::new(a, "exec")).unwrap();
        bp.connect(PinRef::new(a, "then"), PinRef::new(b, "exec")).unwrap();
        bp.connect(PinRef::new(b, "then"), PinRef::new(a, "exec")).unwrap();
        let errs = compile(&bp).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("loop")), "{errs:?}");
        assert!(errs.iter().any(|e| e.message.contains("unknown variable")), "{errs:?}");
    }

    #[test]
    fn type_errors_are_rejected_at_connect_time() {
        let mut bp = Blueprint::new();
        let v = bp.add("vec.make", (0.0, 0.0), &[]);
        let c = bp.add("color.make", (0.0, 0.0), &[]);
        let r = bp.connect(PinRef::new(v, "vector"), PinRef::new(c, "r"));
        assert!(matches!(r, Err(crate::graph::LinkError::Type { .. })));
    }
}
