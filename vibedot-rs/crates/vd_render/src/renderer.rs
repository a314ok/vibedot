//! wgpu renderer: HDR multi-target canvas pass, additive 2D lights, lighting
//! composite, physically-inspired bloom pyramid and a final post-processing
//! pass, followed by an LDR HUD pass.
//!
//! ```text
//!  instances ─► [canvas MRT] ─► albedo ─┐
//!                             └► emissive ┼─► [composite] ─► hdr ─► [bloom ↓↑] ─┐
//!  lights ───► [light accum] ─► light ───┘                    └────────────────┴► [final] ─► target ◄─ [HUD]
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use vd_core::{Affine2, Color, Mat4, StringName, Vec2, vec2};
use wgpu::util::DeviceExt;

use crate::draw::{Blend, DrawCmd, DrawList, Prim, ShapeCode, TonemapMode};
use crate::text::{ATLAS_SIZE, Align, GlyphAtlas};

/// Loads raw file bytes for a texture path (project directory or pack file).
pub type AssetLoader = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const BLOOM_LEVELS: usize = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Instance {
    m: [f32; 4],
    t_size: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct LightInstance {
    pos_r: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view: [[f32; 4]; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, Default)]
struct PostUniform {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    tint: [f32; 4],
    flash: [f32; 4],
    d: [f32; 4],
    texel: [f32; 4],
    ambient: [f32; 4],
}

/// Texture key used for batching: a path, or one of the built-ins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TexKey {
    White,
    Atlas,
    Path(StringName),
}

struct GpuTexture {
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    _texture: wgpu::Texture,
}

#[derive(Clone, Copy, Debug)]
struct Batch {
    tex: TexKey,
    blend: Blend,
    start: u32,
    count: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub instances: u32,
    pub lights: u32,
    pub textures: u32,
}

struct Targets {
    size: (u32, u32),
    internal: (u32, u32),
    albedo: wgpu::TextureView,
    emissive: wgpu::TextureView,
    light: wgpu::TextureView,
    hdr: wgpu::TextureView,
    bloom: Vec<(wgpu::TextureView, (u32, u32))>,
    // Fullscreen pass bind groups and their uniform buffers.
    composite: (wgpu::BindGroup, wgpu::Buffer),
    prefilter: (wgpu::BindGroup, wgpu::Buffer),
    down: Vec<(wgpu::BindGroup, wgpu::Buffer)>,
    up: Vec<(wgpu::BindGroup, wgpu::Buffer)>,
    final_: (wgpu::BindGroup, wgpu::Buffer),
}

struct OutputPipelines {
    final_: wgpu::RenderPipeline,
    hud_alpha: wgpu::RenderPipeline,
    hud_add: wgpu::RenderPipeline,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    loader: AssetLoader,

    globals_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    post_layout: wgpu::BindGroupLayout,
    canvas_module: wgpu::ShaderModule,
    post_module: wgpu::ShaderModule,

    scene_alpha: wgpu::RenderPipeline,
    scene_add: wgpu::RenderPipeline,
    light_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    prefilter_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    outputs: HashMap<wgpu::TextureFormat, OutputPipelines>,

    world_globals: (wgpu::Buffer, wgpu::BindGroup),
    hud_globals: (wgpu::Buffer, wgpu::BindGroup),

    sampler_linear: wgpu::Sampler,
    sampler_pixel: wgpu::Sampler,
    textures: HashMap<TexKey, GpuTexture>,
    atlas: GlyphAtlas,
    atlas_texture: wgpu::Texture,

    instance_buf: wgpu::Buffer,
    instance_cap: u64,
    light_buf: wgpu::Buffer,
    light_cap: u64,

    targets: Option<Targets>,
    /// Use nearest-neighbour filtering for sprites (pixel art).
    pub pixel_art: bool,
    pub stats: RenderStats,

    // Scratch buffers reused every frame (no per-frame allocation).
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    order: Vec<usize>,
}

fn color4(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

fn affine_to_mat4(a: &Affine2) -> Mat4 {
    let m = a.matrix2;
    Mat4::from_cols_array(&[
        m.x_axis.x, m.x_axis.y, 0.0, 0.0, //
        m.y_axis.x, m.y_axis.y, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        a.translation.x, a.translation.y, 0.0, 1.0,
    ])
}

/// Screen pixels (y down) → clip space.
fn screen_to_clip(size: Vec2) -> Affine2 {
    Affine2::from_cols_array(&[2.0 / size.x, 0.0, 0.0, -2.0 / size.y, -1.0, 1.0])
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, loader: AssetLoader) -> Self {
        let device = device.clone();
        let queue = queue.clone();
        let canvas_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vd canvas shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/canvas.wgsl").into()),
        });
        let post_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vd post shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vd globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vd texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vd post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(2),
                tex_entry(3),
                tex_entry(4),
            ],
        });

        let canvas_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vd canvas layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let light_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vd light layout"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vd post layout"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });

        let alpha_blend = wgpu::BlendState::ALPHA_BLENDING;
        let add_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let scene_alpha = Self::canvas_pipeline(
            &device,
            &canvas_layout,
            &canvas_module,
            "fs_scene",
            &[HDR_FORMAT, HDR_FORMAT],
            alpha_blend,
        );
        let scene_add = Self::canvas_pipeline(
            &device,
            &canvas_layout,
            &canvas_module,
            "fs_scene",
            &[HDR_FORMAT, HDR_FORMAT],
            add_blend,
        );

        let light_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let light_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vd lights"),
            layout: Some(&light_layout),
            vertex: wgpu::VertexState {
                module: &canvas_module,
                entry_point: Some("vs_light"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<LightInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &light_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &canvas_module,
                entry_point: Some("fs_light"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(add_blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let fullscreen = |entry: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&post_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &post_module,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &post_module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let composite_pipeline = fullscreen("fs_composite", HDR_FORMAT, None);
        let prefilter_pipeline = fullscreen("fs_bloom_prefilter", HDR_FORMAT, None);
        let down_pipeline = fullscreen("fs_bloom_down", HDR_FORMAT, None);
        let up_pipeline = fullscreen("fs_bloom_up", HDR_FORMAT, Some(add_blend));

        let make_globals = |label| {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &globals_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
            });
            (buf, bg)
        };
        let world_globals = make_globals("vd world globals");
        let hud_globals = make_globals("vd hud globals");

        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vd linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sampler_pixel = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vd nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vd glyph atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let instance_cap = 1024;
        let instance_buf = Self::make_buffer(&device, "vd instances", instance_cap * size_of::<Instance>() as u64);
        let light_cap = 64;
        let light_buf = Self::make_buffer(&device, "vd lights", light_cap * size_of::<LightInstance>() as u64);

        let mut r = Self {
            device,
            queue,
            loader,
            globals_layout,
            texture_layout,
            post_layout,
            canvas_module,
            post_module,
            scene_alpha,
            scene_add,
            light_pipeline,
            composite_pipeline,
            prefilter_pipeline,
            down_pipeline,
            up_pipeline,
            outputs: HashMap::new(),
            world_globals,
            hud_globals,
            sampler_linear,
            sampler_pixel,
            textures: HashMap::new(),
            atlas: GlyphAtlas::new(),
            atlas_texture,
            instance_buf,
            instance_cap,
            light_buf,
            light_cap,
            targets: None,
            pixel_art: false,
            stats: RenderStats::default(),
            instances: Vec::new(),
            batches: Vec::new(),
            order: Vec::new(),
        };
        r.insert_rgba(TexKey::White, &[255, 255, 255, 255], 1, 1, false);
        let atlas_view = r.atlas_texture.create_view(&Default::default());
        let atlas_bg = r.texture_bind_group(&atlas_view, false);
        r.textures.insert(
            TexKey::Atlas,
            GpuTexture { bind_group: atlas_bg, size: (ATLAS_SIZE, ATLAS_SIZE), _texture: r.atlas_texture.clone() },
        );
        r
    }

    fn make_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn canvas_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        module: &wgpu::ShaderModule,
        fs: &str,
        formats: &[wgpu::TextureFormat],
        blend: wgpu::BlendState,
    ) -> wgpu::RenderPipeline {
        let attrs = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4
        ];
        let targets: Vec<Option<wgpu::ColorTargetState>> = formats
            .iter()
            .map(|&format| Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL }))
            .collect();
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vd canvas"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    fn output_pipelines(&mut self, format: wgpu::TextureFormat) {
        if self.outputs.contains_key(&format) {
            return;
        }
        let post_pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vd final layout"),
            bind_group_layouts: &[Some(&self.post_layout)],
            immediate_size: 0,
        });
        let final_ = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vd final"),
            layout: Some(&post_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.post_module,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.post_module,
                entry_point: Some("fs_final"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let canvas_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vd hud layout"),
            bind_group_layouts: &[Some(&self.globals_layout), Some(&self.texture_layout)],
            immediate_size: 0,
        });
        let premul_add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let hud_alpha = Self::canvas_pipeline(
            &self.device,
            &canvas_layout,
            &self.canvas_module,
            "fs_hud",
            &[format],
            wgpu::BlendState::ALPHA_BLENDING,
        );
        let hud_add = Self::canvas_pipeline(&self.device, &canvas_layout, &self.canvas_module, "fs_hud", &[format], premul_add);
        self.outputs.insert(format, OutputPipelines { final_, hud_alpha, hud_add });
    }

    fn texture_bind_group(&self, view: &wgpu::TextureView, pixel: bool) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vd texture bg"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(if pixel { &self.sampler_pixel } else { &self.sampler_linear }),
                },
            ],
        })
    }

    fn insert_rgba(&mut self, key: TexKey, rgba: &[u8], w: u32, h: u32, pixel: bool) {
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("vd texture"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.texture_bind_group(&view, pixel);
        self.textures.insert(key, GpuTexture { bind_group, size: (w, h), _texture: texture });
    }

    /// Forces a texture to be reloaded next time it is drawn (hot reload).
    pub fn invalidate_texture(&mut self, path: &str) {
        self.textures.remove(&TexKey::Path(StringName::new(path)));
    }

    pub fn invalidate_all_textures(&mut self) {
        self.textures.retain(|k, _| !matches!(k, TexKey::Path(_)));
    }

    pub fn texture_size(&mut self, path: StringName) -> Option<(u32, u32)> {
        self.ensure_texture(TexKey::Path(path));
        self.textures.get(&TexKey::Path(path)).map(|t| t.size)
    }

    fn ensure_texture(&mut self, key: TexKey) {
        let TexKey::Path(path) = key else { return };
        if self.textures.contains_key(&key) {
            return;
        }
        let decoded = (self.loader)(path.as_str()).and_then(|bytes| match image::load_from_memory(&bytes) {
            Ok(img) => Some(img.to_rgba8()),
            Err(e) => {
                log::warn!("cannot decode texture '{path}': {e}");
                None
            }
        });
        match decoded {
            Some(img) => {
                let (w, h) = img.dimensions();
                self.insert_rgba(key, &img, w, h, self.pixel_art);
            }
            None => {
                log::warn!("missing texture '{path}' - using checkerboard");
                let mut px = Vec::with_capacity(8 * 8 * 4);
                for y in 0..8 {
                    for x in 0..8 {
                        let on = (x / 2 + y / 2) % 2 == 0;
                        px.extend_from_slice(if on { &[255, 0, 255, 255] } else { &[20, 20, 20, 255] });
                    }
                }
                self.insert_rgba(key, &px, 8, 8, true);
            }
        }
    }

    fn create_targets(&self, size: (u32, u32), pixelate: f32) -> Targets {
        let device = &self.device;
        let k = pixelate.max(1.0);
        let internal = (((size.0 as f32 / k).round() as u32).max(1), ((size.1 as f32 / k).round() as u32).max(1));
        let tex = |label: &str, (w, h): (u32, u32)| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let albedo = tex("vd albedo", internal);
        let emissive = tex("vd emissive", internal);
        let light = tex("vd light", internal);
        let hdr = tex("vd hdr", internal);
        let mut bloom = Vec::new();
        let mut bs = ((internal.0 / 2).max(1), (internal.1 / 2).max(1));
        for i in 0..BLOOM_LEVELS {
            bloom.push((tex(&format!("vd bloom {i}"), bs), bs));
            if bs.0 <= 4 || bs.1 <= 4 {
                break;
            }
            bs = ((bs.0 / 2).max(1), (bs.1 / 2).max(1));
        }
        let pass = |t0: &wgpu::TextureView, t1: &wgpu::TextureView, t2: &wgpu::TextureView| {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("vd post uniform"),
                size: size_of::<PostUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vd post bg"),
                layout: &self.post_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler_linear) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(t0) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(t1) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(t2) },
                ],
            });
            (bg, buf)
        };
        let composite = pass(&albedo, &emissive, &light);
        let prefilter = pass(&hdr, &hdr, &hdr);
        let down = (1..bloom.len()).map(|i| pass(&bloom[i - 1].0, &hdr, &hdr)).collect();
        let up = (1..bloom.len()).map(|i| pass(&bloom[i].0, &hdr, &hdr)).collect();
        let final_ = pass(&hdr, &bloom[0].0, &hdr);
        Targets { size, internal, albedo, emissive, light, hdr, bloom, composite, prefilter, down, up, final_ }
    }

    fn write_buffer_grow<T: Pod>(&mut self, which: u8, data: &[T]) {
        let bytes = bytemuck::cast_slice::<T, u8>(data);
        let elem = size_of::<T>() as u64;
        let (buf, cap) = if which == 0 {
            (&mut self.instance_buf, &mut self.instance_cap)
        } else {
            (&mut self.light_buf, &mut self.light_cap)
        };
        let needed = data.len() as u64;
        if needed > *cap {
            *cap = needed.next_power_of_two();
            *buf = Self::make_buffer(&self.device, "vd instances", *cap * elem);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(buf, 0, bytes);
        }
    }

    /// Converts draw commands into GPU instances and batches.
    fn build_instances(&mut self, cmds: &[DrawCmd], hud: bool) {
        self.order.clear();
        self.order.extend(0..cmds.len());
        // Stable sort by z keeps submission order inside a layer.
        self.order.sort_by_key(|&i| cmds[i].z);
        let base = self.instances.len();
        let _ = base;
        for oi in 0..self.order.len() {
            let cmd = &cmds[self.order[oi]];
            match &cmd.prim {
                Prim::Quad(q) => {
                    let key = if q.texture.is_empty() || q.shape != ShapeCode::Sprite {
                        TexKey::White
                    } else {
                        TexKey::Path(q.texture)
                    };
                    self.ensure_texture(key);
                    let m = q.xform.matrix2;
                    let inst = Instance {
                        m: [m.x_axis.x, m.x_axis.y, m.y_axis.x, m.y_axis.y],
                        t_size: [q.xform.translation.x, q.xform.translation.y, q.size.x, q.size.y],
                        uv: q.uv,
                        color: color4(q.color),
                        params: [q.shape as u32 as f32, q.corner_radius, q.thickness, q.softness],
                        extra: [q.emission, if q.blend == Blend::Additive { 1.0 } else { 0.0 }, 0.0, 0.0],
                    };
                    self.push_instance(key, q.blend, inst);
                }
                Prim::Text(t) => {
                    let (quads, block) = self.atlas.layout(&t.text, t.size, t.face, t.align);
                    let anchor_x = match t.align {
                        Align::Left => 0.0,
                        Align::Center => block.x * 0.5,
                        Align::Right => block.x,
                    };
                    let origin = vec2(-anchor_x, -block.y * 0.5);
                    for g in quads {
                        let center = origin + g.min + g.size * 0.5;
                        let xf = t.xform * Affine2::from_translation(center) * Affine2::from_scale(g.size);
                        let m = xf.matrix2;
                        let inst = Instance {
                            m: [m.x_axis.x, m.x_axis.y, m.y_axis.x, m.y_axis.y],
                            t_size: [xf.translation.x, xf.translation.y, g.size.x, g.size.y],
                            uv: g.uv,
                            color: color4(t.color),
                            params: [5.0, 0.0, 0.0, 0.0],
                            extra: [t.emission, 0.0, 0.0, 0.0],
                        };
                        self.push_instance(TexKey::Atlas, Blend::Alpha, inst);
                    }
                }
            }
        }
        let _ = hud;
    }

    fn push_instance(&mut self, tex: TexKey, blend: Blend, inst: Instance) {
        let idx = self.instances.len() as u32;
        self.instances.push(inst);
        if let Some(last) = self.batches.last_mut() {
            if last.tex == tex && last.blend == blend && last.start + last.count == idx {
                last.count += 1;
                return;
            }
        }
        self.batches.push(Batch { tex, blend, start: idx, count: 1 });
    }

    fn post_uniform(list: &DrawList, src: (u32, u32), dst: (u32, u32), srgb_encode: bool) -> PostUniform {
        let e = &list.env;
        PostUniform {
            a: [
                e.exposure,
                match e.tonemap {
                    TonemapMode::Linear => 0.0,
                    TonemapMode::Reinhard => 1.0,
                    TonemapMode::Aces => 2.0,
                    TonemapMode::AgX => 3.0,
                },
                e.bloom_intensity,
                e.bloom_threshold,
            ],
            b: [e.bloom_radius, e.vignette, e.chromatic_aberration, e.film_grain],
            c: [e.scanlines, e.crt_curvature, e.saturation, e.contrast],
            tint: color4(e.tint),
            flash: color4(list.flash),
            d: [list.time, e.pixelate, if srgb_encode { 1.0 } else { 0.0 }, if e.bloom { 1.0 } else { 0.0 }],
            texel: [1.0 / src.0 as f32, 1.0 / src.1 as f32, dst.0 as f32, dst.1 as f32],
            ambient: color4(e.ambient),
        }
    }

    /// Renders `list` into `target` (of `format`, `size` pixels).
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        list: &DrawList,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> RenderStats {
        let size = (size.0.max(1), size.1.max(1));
        let pixelate = list.env.pixelate.round().max(1.0);
        let needs_new = self.targets.as_ref().is_none_or(|t| {
            t.size != size || t.internal != (((size.0 as f32 / pixelate).round() as u32).max(1), ((size.1 as f32 / pixelate).round() as u32).max(1))
        });
        if needs_new {
            self.targets = Some(self.create_targets(size, pixelate));
        }
        self.output_pipelines(format);

        // ---- CPU: instances, batches, lights, uniforms
        self.instances.clear();
        self.batches.clear();
        self.build_instances(&list.world, false);
        let world_batches = self.batches.len();
        self.build_instances(&list.screen, true);
        if self.atlas.dirty {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.atlas.pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ATLAS_SIZE * 4), rows_per_image: None },
                wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            );
            self.atlas.dirty = false;
        }
        let instances = std::mem::take(&mut self.instances);
        self.write_buffer_grow(0, &instances);
        self.instances = instances;

        let lights: Vec<LightInstance> = list
            .lights
            .iter()
            .map(|l| LightInstance {
                pos_r: [l.position.x, l.position.y, l.radius, l.falloff],
                color: color4(l.color),
            })
            .collect();
        self.write_buffer_grow(1, &lights);

        let t = self.targets.as_ref().expect("targets created above");
        let internal = t.internal;
        let internal_v = vec2(internal.0 as f32, internal.1 as f32);
        let out_v = vec2(size.0 as f32, size.1 as f32);
        // World camera: computed against the *output* size so pixelation
        // only lowers resolution, never changes framing.
        let world_to_screen = list.view.world_to_screen(out_v);
        let view = screen_to_clip(out_v) * world_to_screen;
        let _ = internal_v;
        let bold = 0.06;
        let globals = Globals { view: affine_to_mat4(&view).to_cols_array_2d(), params: [list.time, bold, 0.0, 0.0] };
        self.queue.write_buffer(&self.world_globals.0, 0, bytemuck::bytes_of(&globals));
        let hud = Globals {
            view: affine_to_mat4(&screen_to_clip(out_v)).to_cols_array_2d(),
            params: [list.time, bold, 0.0, 0.0],
        };
        self.queue.write_buffer(&self.hud_globals.0, 0, bytemuck::bytes_of(&hud));

        let srgb_encode = !format.is_srgb();
        self.queue.write_buffer(&t.composite.1, 0, bytemuck::bytes_of(&Self::post_uniform(list, internal, internal, false)));
        self.queue.write_buffer(&t.prefilter.1, 0, bytemuck::bytes_of(&Self::post_uniform(list, internal, t.bloom[0].1, false)));
        for (i, (_, buf)) in t.down.iter().enumerate() {
            let (src, dst) = (t.bloom[i].1, t.bloom[i + 1].1);
            self.queue.write_buffer(buf, 0, bytemuck::bytes_of(&Self::post_uniform(list, src, dst, false)));
        }
        for (i, (_, buf)) in t.up.iter().enumerate() {
            let (src, dst) = (t.bloom[i + 1].1, t.bloom[i].1);
            self.queue.write_buffer(buf, 0, bytemuck::bytes_of(&Self::post_uniform(list, src, dst, false)));
        }
        self.queue.write_buffer(&t.final_.1, 0, bytemuck::bytes_of(&Self::post_uniform(list, internal, size, srgb_encode)));

        // ---- GPU passes
        let clear = list.env.clear_color;
        let mut stats = RenderStats {
            instances: self.instances.len() as u32,
            lights: lights.len() as u32,
            textures: self.textures.len() as u32,
            ..Default::default()
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vd scene"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &t.albedo,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: f64::from(clear.r),
                                g: f64::from(clear.g),
                                b: f64::from(clear.b),
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &t.emissive,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    }),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.world_globals.1, &[]);
            pass.set_vertex_buffer(0, self.instance_buf.slice(..));
            for b in &self.batches[..world_batches] {
                let Some(tex) = self.textures.get(&b.tex) else { continue };
                pass.set_pipeline(if b.blend == Blend::Additive { &self.scene_add } else { &self.scene_alpha });
                pass.set_bind_group(1, &tex.bind_group, &[]);
                pass.draw(0..6, b.start..b.start + b.count);
                stats.draw_calls += 1;
            }
        }
        {
            let amb = list.env.ambient;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vd lights"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.light,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(amb.r),
                            g: f64::from(amb.g),
                            b: f64::from(amb.b),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !lights.is_empty() {
                pass.set_pipeline(&self.light_pipeline);
                pass.set_bind_group(0, &self.world_globals.1, &[]);
                pass.set_vertex_buffer(0, self.light_buf.slice(..));
                pass.draw(0..6, 0..lights.len() as u32);
                stats.draw_calls += 1;
            }
        }
        let fullscreen = |encoder: &mut wgpu::CommandEncoder,
                          label: &str,
                          pipeline: &wgpu::RenderPipeline,
                          bg: &wgpu::BindGroup,
                          view: &wgpu::TextureView,
                          load: wgpu::LoadOp<wgpu::Color>| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
        };
        let clear_op = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        fullscreen(encoder, "vd composite", &self.composite_pipeline, &t.composite.0, &t.hdr, clear_op);
        stats.draw_calls += 1;
        if list.env.bloom {
            fullscreen(encoder, "vd bloom prefilter", &self.prefilter_pipeline, &t.prefilter.0, &t.bloom[0].0, clear_op);
            for (i, (bg, _)) in t.down.iter().enumerate() {
                fullscreen(encoder, "vd bloom down", &self.down_pipeline, bg, &t.bloom[i + 1].0, clear_op);
            }
            for (i, (bg, _)) in t.up.iter().enumerate().rev() {
                fullscreen(encoder, "vd bloom up", &self.up_pipeline, bg, &t.bloom[i].0, wgpu::LoadOp::Load);
            }
            stats.draw_calls += 1 + 2 * t.down.len() as u32;
        }
        let out = &self.outputs[&format];
        fullscreen(encoder, "vd final", &out.final_, &t.final_.0, target, clear_op);
        stats.draw_calls += 1;

        if self.batches.len() > world_batches {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vd hud"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.hud_globals.1, &[]);
            pass.set_vertex_buffer(0, self.instance_buf.slice(..));
            for b in &self.batches[world_batches..] {
                let Some(tex) = self.textures.get(&b.tex) else { continue };
                pass.set_pipeline(if b.blend == Blend::Additive { &out.hud_add } else { &out.hud_alpha });
                pass.set_bind_group(1, &tex.bind_group, &[]);
                pass.draw(0..6, b.start..b.start + b.count);
                stats.draw_calls += 1;
            }
        }
        self.stats = stats;
        stats
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// Creates a standalone device (no window) - used by tests, screenshots and
/// headless exports. Returns `None` if no adapter is available.
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("vd headless"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .ok()
}

/// Renders a draw list off-screen and reads it back as RGBA8 (sRGB) pixels.
pub fn render_to_rgba(renderer: &mut Renderer, list: &DrawList, size: (u32, u32)) -> Vec<u8> {
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vd readback"),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vd readback") });
    renderer.render(&mut encoder, list, &view, format, size);
    let row = (size.0 * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vd readback buf"),
        size: u64::from(row * size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("buffer mapped");
    let mut out = Vec::with_capacity((size.0 * size.1 * 4) as usize);
    for y in 0..size.1 {
        let start = (y * row) as usize;
        out.extend_from_slice(&data[start..start + (size.0 * 4) as usize]);
    }
    drop(data);
    buf.unmap();
    out
}
