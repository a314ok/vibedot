//! # vd_blueprint
//!
//! Visual scripting for Vibedot, in the spirit of Unreal Blueprints (Godot
//! dropped VisualScript in 4.0 - we bring it back, compiled):
//!
//! 1. [`graph::Blueprint`] - the editable node graph saved as `.vbp` JSON.
//! 2. [`defs::LIBRARY`] - 130+ nodes: events, flow control, math, vectors,
//!    scene manipulation, physics, input, camera/FX.
//! 3. [`compiler::compile`] - validates types and lowers the graph to compact
//!    register bytecode (16-byte instructions).
//! 4. [`vm::Instance`] - executes the bytecode per node, with latent `Delay`
//!    support and an instruction budget against infinite loops.
#![forbid(unsafe_code)]

pub mod compiler;
pub mod defs;
pub mod graph;
pub mod ops;
pub mod vm;

pub use compiler::{CompileError, Program, compile};
pub use defs::{EventKind, LIBRARY, NodeDef, PinKind};
pub use graph::{BpNode, BpNodeId, Blueprint, Link, LinkError, PinRef, VarDef};
pub use vm::{ExecCtx, Host, Instance, NullHost, RuntimeError};

#[cfg(test)]
mod tests;
