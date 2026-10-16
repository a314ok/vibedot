//! # vd_render
//!
//! Vibedot's rendering server. The engine produces a backend-agnostic
//! [`DrawList`]; the [`Renderer`] turns it into an HDR image with 2D lighting,
//! bloom, tonemapping and stylised post effects using `wgpu` (Vulkan, Metal,
//! DX12 or OpenGL ES depending on the platform).
//!
//! The only `unsafe` in the rendering path lives inside `wgpu`; GPU data is
//! uploaded through `bytemuck::Pod` types, so there are no manual transmutes.
#![forbid(unsafe_code)]

pub mod draw;
pub mod renderer;
pub mod text;

pub use draw::*;
pub use renderer::{AssetLoader, RenderStats, Renderer, headless_device, render_to_rgba};
pub use text::{Align, FontFace};
pub use wgpu;
