use std::collections::HashMap;
use std::sync::Arc;
use cosmic_text::{CacheKey, Color, FontSystem, PhysicalGlyph, SwashCache, SwashContent};
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuQuad {
    pub rect: [f32; 4],   // x, y, width, height (in pixels)
    pub uv: [f32; 4],     // u1, v1, u2, v2 (0.0 .. 1.0)
    pub color: [f32; 4],  // r, g, b, a (0.0 .. 1.0)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub screen_size: [f32; 2],
    pub _padding: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasEntry {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub uv: [f32; 4],
    pub is_color: bool,
}

pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub current_x: u32,
    pub current_y: u32,
    pub shelf_height: u32,
    pub white_pixel_uv: [f32; 4],
    pub shade_25_uv: [f32; 4],
    pub shade_50_uv: [f32; 4],
    pub shade_75_uv: [f32; 4],
    pub cache: HashMap<CacheKey, AtlasEntry>,
}

impl GlyphAtlas {
    pub fn new(queue: &wgpu::Queue, texture: &wgpu::Texture) -> Self {
        let width = 2048;
        let height = 2048;

        let mut atlas = Self {
            width,
            height,
            current_x: 0,
            current_y: 0,
            shelf_height: 0,
            white_pixel_uv: [0.0; 4],
            shade_25_uv: [0.0; 4],
            shade_50_uv: [0.0; 4],
            shade_75_uv: [0.0; 4],
            cache: HashMap::new(),
        };

        atlas.init_reserved_patterns(queue, texture);
        atlas
    }

    pub fn init_reserved_patterns(&mut self, queue: &wgpu::Queue, texture: &wgpu::Texture) {
        // 1. 4x4 pure white block at (0, 0) for solid quads
        let white_pixels = vec![255u8; 4 * 4 * 4];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &white_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * 4),
                rows_per_image: Some(4),
            },
            wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
        );
        self.white_pixel_uv = [
            1.5 / self.width as f32,
            1.5 / self.height as f32,
            2.5 / self.width as f32,
            2.5 / self.height as f32,
        ];

        // 2. 8x8 shade 25% at (6, 0)
        let mut shade_25 = Vec::with_capacity(8 * 8 * 4);
        for py in 0..8 {
            for px in 0..8 {
                let a = if (px % 2 == 0) && (py % 2 == 0) { 255u8 } else { 0u8 };
                shade_25.extend_from_slice(&[255, 255, 255, a]);
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 6, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &shade_25,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * 4),
                rows_per_image: Some(8),
            },
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        self.shade_25_uv = [
            6.0 / self.width as f32,
            0.0 / self.height as f32,
            14.0 / self.width as f32,
            8.0 / self.height as f32,
        ];

        // 3. 8x8 shade 50% at (16, 0)
        let mut shade_50 = Vec::with_capacity(8 * 8 * 4);
        for py in 0..8 {
            for px in 0..8 {
                let a = if (px + py) % 2 == 0 { 255u8 } else { 0u8 };
                shade_50.extend_from_slice(&[255, 255, 255, a]);
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 16, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &shade_50,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * 4),
                rows_per_image: Some(8),
            },
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        self.shade_50_uv = [
            16.0 / self.width as f32,
            0.0 / self.height as f32,
            24.0 / self.width as f32,
            8.0 / self.height as f32,
        ];

        // 4. 8x8 shade 75% at (26, 0)
        let mut shade_75 = Vec::with_capacity(8 * 8 * 4);
        for py in 0..8 {
            for px in 0..8 {
                let a = if !((px % 2 == 0) && (py % 2 == 0)) { 255u8 } else { 0u8 };
                shade_75.extend_from_slice(&[255, 255, 255, a]);
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 26, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &shade_75,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * 4),
                rows_per_image: Some(8),
            },
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        self.shade_75_uv = [
            26.0 / self.width as f32,
            0.0 / self.height as f32,
            34.0 / self.width as f32,
            8.0 / self.height as f32,
        ];

        self.current_x = 36;
        self.current_y = 0;
        self.shelf_height = 8;
    }

    pub fn get_or_insert(
        &mut self,
        cache_key: CacheKey,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> AtlasEntry {
        if let Some(entry) = self.cache.get(&cache_key) {
            return *entry;
        }

        let image_opt = swash_cache.get_image(font_system, cache_key);
        let image = match image_opt {
            Some(img) => img,
            None => {
                let empty = AtlasEntry {
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 0,
                    uv: [0.0; 4],
                    is_color: false,
                };
                self.cache.insert(cache_key, empty);
                return empty;
            }
        };

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            let empty = AtlasEntry {
                left: image.placement.left,
                top: image.placement.top,
                width: 0,
                height: 0,
                uv: [0.0; 4],
                is_color: false,
            };
            self.cache.insert(cache_key, empty);
            return empty;
        }

        // Check if we need to wrap to the next shelf
        if self.current_x + w + 1 > self.width {
            self.current_x = 0;
            self.current_y += self.shelf_height + 1;
            self.shelf_height = 0;
        }

        // Check if atlas is full
        if self.current_y + h + 1 > self.height {
            self.cache.clear();
            self.init_reserved_patterns(queue, texture);
        }

        let entry_x = self.current_x;
        let entry_y = self.current_y;
        self.current_x += w + 1;
        self.shelf_height = self.shelf_height.max(h);

        let is_color = matches!(image.content, SwashContent::Color);
        let rgba_data: Vec<u8> = match image.content {
            SwashContent::Mask => {
                let mut v = Vec::with_capacity((w * h * 4) as usize);
                for &alpha in &image.data {
                    v.extend_from_slice(&[255, 255, 255, alpha]);
                }
                v
            }
            SwashContent::Color => image.data.clone(),
            SwashContent::SubpixelMask => {
                let mut v = Vec::with_capacity((w * h * 4) as usize);
                for chunk in image.data.chunks_exact(3) {
                    let a = ((chunk[0] as u32 + chunk[1] as u32 + chunk[2] as u32) / 3) as u8;
                    v.extend_from_slice(&[255, 255, 255, a]);
                }
                v
            }
        };

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: entry_x, y: entry_y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let uv = [
            (entry_x as f32) / (self.width as f32),
            (entry_y as f32) / (self.height as f32),
            ((entry_x + w) as f32) / (self.width as f32),
            ((entry_y + h) as f32) / (self.height as f32),
        ];

        let entry = AtlasEntry {
            left: image.placement.left,
            top: image.placement.top,
            width: w,
            height: h,
            uv,
            is_color,
        };

        self.cache.insert(cache_key, entry);
        entry
    }
}

pub struct GpuRenderer {
    #[allow(dead_code)]
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub pipeline: wgpu::RenderPipeline,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub atlas_texture: wgpu::Texture,
    pub atlas: GlyphAtlas,
    pub instance_buffer: wgpu::Buffer,
    pub instance_buffer_capacity: usize,
    pub quads: Vec<GpuQuad>,
}

impl GpuRenderer {
    pub fn new(window: Arc<Window>, width: u32, height: u32) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
        });

        let surface = instance
            .create_surface(window)
            .expect("Failed to create Vulkan surface");

        let adapter = if let Ok(target) = std::env::var("ESTEDAD_GPU") {
            let target_lower = target.to_lowercase();
            instance
                .enumerate_adapters(wgpu::Backends::VULKAN)
                .into_iter()
                .find(|a| {
                    let name = a.get_info().name.to_lowercase();
                    name.contains(&target_lower)
                })
                .unwrap_or_else(|| {
                    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: false,
                    }))
                    .expect("Failed to find supported Vulkan GPU adapter")
                })
        } else {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .expect("Failed to find supported Vulkan GPU adapter")
        };

        let adapter_info = adapter.get_info();
        println!(
            "🚀 [Vulkan Engine Active] GPU: \"{}\" | Driver: {:?} | Backend: {:?}",
            adapter_info.name, adapter_info.driver_info, adapter_info.backend
        );

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Estedad-Term Vulkan Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .expect("Failed to create Vulkan device");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let initial_w = width.max(1);
        let initial_h = height.max(1);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: initial_w,
            height: initial_h,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Atlas texture (2048 x 2048 RGBA8)
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas Texture"),
            size: wgpu::Extent3d {
                width: 2048,
                height: 2048,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let atlas = GlyphAtlas::new(&queue, &atlas_texture);

        // Uniform buffer for screen size
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: std::mem::size_of::<Uniforms>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // WGSL Shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Terminal Quad Shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct Uniforms {
    screen_size: vec2<f32>,
    padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct QuadInstance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: QuadInstance,
) -> VertexOutput {
    var out: VertexOutput;

    var pos_norm = vec2<f32>(0.0, 0.0);
    switch vertex_index {
        case 0u: { pos_norm = vec2<f32>(0.0, 0.0); }
        case 1u: { pos_norm = vec2<f32>(1.0, 0.0); }
        case 2u: { pos_norm = vec2<f32>(0.0, 1.0); }
        case 3u: { pos_norm = vec2<f32>(0.0, 1.0); }
        case 4u: { pos_norm = vec2<f32>(1.0, 0.0); }
        case 5u: { pos_norm = vec2<f32>(1.0, 1.0); }
        default: {}
    }

    let pixel_pos = vec2<f32>(
        instance.rect.x + pos_norm.x * instance.rect.z,
        instance.rect.y + pos_norm.y * instance.rect.w,
    );

    let ndc_x = (pixel_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_pos.y / uniforms.screen_size.y) * 2.0;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);

    out.uv = vec2<f32>(
        mix(instance.uv.x, instance.uv.z, pos_norm.x),
        mix(instance.uv.y, instance.uv.w, pos_norm.y),
    );

    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(atlas_tex, atlas_sampler, in.uv);
    return vec4<f32>(in.color.rgb * sampled.rgb, in.color.a * sampled.a);
}
"#
                .into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Terminal Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Terminal Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Terminal Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Terminal Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuQuad>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 2,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let initial_capacity = 2048;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (initial_capacity * std::mem::size_of::<GpuQuad>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            instance,
            surface,
            device,
            queue,
            config,
            pipeline,
            uniform_buffer,
            bind_group,
            atlas_texture,
            atlas,
            instance_buffer,
            instance_buffer_capacity: initial_capacity,
            quads: Vec::with_capacity(initial_capacity),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn begin_frame(&mut self) {
        self.quads.clear();
        let uniforms = Uniforms {
            screen_size: [self.config.width as f32, self.config.height as f32],
            _padding: [0.0, 0.0],
        };
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );
    }

    #[inline]
    pub fn push_solid_quad(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.quads.push(GpuQuad {
            rect: [x, y, w, h],
            uv: self.atlas.white_pixel_uv,
            color,
        });
    }

    #[inline]
    pub fn push_rect_color(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let rgba = [
            color.r() as f32 / 255.0,
            color.g() as f32 / 255.0,
            color.b() as f32 / 255.0,
            color.a() as f32 / 255.0,
        ];
        self.push_solid_quad(x, y, w, h, rgba);
    }

    #[inline]
    pub fn push_rect_hex_alpha(&mut self, x: f32, y: f32, w: f32, h: f32, rgb_hex: u32, alpha: u8) {
        let r = ((rgb_hex >> 16) & 0xFF) as f32 / 255.0;
        let g = ((rgb_hex >> 8) & 0xFF) as f32 / 255.0;
        let b = (rgb_hex & 0xFF) as f32 / 255.0;
        let a = (alpha as f32) / 255.0;
        self.push_solid_quad(x, y, w, h, [r, g, b, a]);
    }

    pub fn draw_glyph(
        &mut self,
        physical: PhysicalGlyph,
        color: Color,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
    ) {
        let entry = self.atlas.get_or_insert(
            physical.cache_key,
            font_system,
            swash_cache,
            &self.queue,
            &self.atlas_texture,
        );

        if entry.width > 0 && entry.height > 0 {
            let gx = (physical.x + entry.left) as f32;
            let gy = (physical.y - entry.top) as f32;
            let quad_color = if entry.is_color {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [
                    color.r() as f32 / 255.0,
                    color.g() as f32 / 255.0,
                    color.b() as f32 / 255.0,
                    color.a() as f32 / 255.0,
                ]
            };

            self.quads.push(GpuQuad {
                rect: [gx, gy, entry.width as f32, entry.height as f32],
                uv: entry.uv,
                color: quad_color,
            });
        }
    }

    pub fn draw_buffer(
        &mut self,
        buffer: &mut cosmic_text::Buffer,
        base_x: f32,
        base_y: f32,
        default_color: Color,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
    ) {
        buffer.shape_until_scroll(font_system, false);
        for run in buffer.layout_runs() {
            let line_y = run.line_y;
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((base_x, line_y + base_y), 1.0);
                let color = glyph.color_opt.unwrap_or(default_color);
                self.draw_glyph(physical, color, font_system, swash_cache);
            }
        }
    }

    pub fn draw_block_element(
        &mut self,
        x_start: f32,
        x_end: f32,
        y_start: f32,
        y_end: f32,
        ch: char,
        fg: Color,
        bg: Option<Color>,
    ) {
        if x_start >= x_end || y_start >= y_end {
            return;
        }

        let w = x_end - x_start;
        let h = y_end - y_start;

        let fg_rgba = [
            fg.r() as f32 / 255.0,
            fg.g() as f32 / 255.0,
            fg.b() as f32 / 255.0,
            fg.a() as f32 / 255.0,
        ];

        let bg_rgba = bg.map(|c| [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            c.a() as f32 / 255.0,
        ]);

        let push_bg = |gpu: &mut GpuRenderer, bx, by, bw, bh| {
            if let Some(b) = bg_rgba {
                gpu.push_solid_quad(bx, by, bw, bh, b);
            }
        };

        match ch {
            // Full block
            '\u{2588}' => {
                self.push_solid_quad(x_start, y_start, w, h, fg_rgba);
            }

            // Half blocks
            '\u{2580}' => {
                let mid_h = (h / 2.0).round();
                self.push_solid_quad(x_start, y_start, w, mid_h, fg_rgba);
                push_bg(self, x_start, y_start + mid_h, w, h - mid_h);
            }
            '\u{2584}' => {
                let mid_h = (h / 2.0).round();
                push_bg(self, x_start, y_start, w, mid_h);
                self.push_solid_quad(x_start, y_start + mid_h, w, h - mid_h, fg_rgba);
            }
            '\u{258C}' => {
                let mid_w = (w / 2.0).round();
                self.push_solid_quad(x_start, y_start, mid_w, h, fg_rgba);
                push_bg(self, x_start + mid_w, y_start, w - mid_w, h);
            }
            '\u{2590}' => {
                let mid_w = (w / 2.0).round();
                push_bg(self, x_start, y_start, mid_w, h);
                self.push_solid_quad(x_start + mid_w, y_start, w - mid_w, h, fg_rgba);
            }

            // Lower vertical fractions (1/8 to 7/8)
            '\u{2581}'..='\u{2587}' => {
                let k = (ch as u32 - 0x2580) as f32;
                let fh = ((h * k + 4.0) / 8.0).floor();
                push_bg(self, x_start, y_start, w, h - fh);
                self.push_solid_quad(x_start, y_start + h - fh, w, fh, fg_rgba);
            }

            // Upper fraction (1/8)
            '\u{2594}' => {
                let fh = ((h * 1.0 + 4.0) / 8.0).floor();
                self.push_solid_quad(x_start, y_start, w, fh, fg_rgba);
                push_bg(self, x_start, y_start + fh, w, h - fh);
            }

            // Left horizontal fractions (1/8 to 7/8)
            '\u{258F}' | '\u{258E}' | '\u{258D}' | '\u{258B}' | '\u{258A}' | '\u{2589}' => {
                let k = match ch {
                    '\u{258F}' => 1.0,
                    '\u{258E}' => 2.0,
                    '\u{258D}' => 3.0,
                    '\u{258B}' => 5.0,
                    '\u{258A}' => 6.0,
                    '\u{2589}' => 7.0,
                    _ => 0.0,
                };
                let fw = ((w * k + 4.0) / 8.0).floor();
                self.push_solid_quad(x_start, y_start, fw, h, fg_rgba);
                push_bg(self, x_start + fw, y_start, w - fw, h);
            }

            // Right fraction (1/8)
            '\u{2595}' => {
                let fw = ((w * 1.0 + 4.0) / 8.0).floor();
                push_bg(self, x_start, y_start, w - fw, h);
                self.push_solid_quad(x_start + w - fw, y_start, fw, h, fg_rgba);
            }

            // Shades
            '\u{2591}' => {
                push_bg(self, x_start, y_start, w, h);
                self.quads.push(GpuQuad {
                    rect: [x_start, y_start, w, h],
                    uv: self.atlas.shade_25_uv,
                    color: fg_rgba,
                });
            }
            '\u{2592}' => {
                push_bg(self, x_start, y_start, w, h);
                self.quads.push(GpuQuad {
                    rect: [x_start, y_start, w, h],
                    uv: self.atlas.shade_50_uv,
                    color: fg_rgba,
                });
            }
            '\u{2593}' => {
                push_bg(self, x_start, y_start, w, h);
                self.quads.push(GpuQuad {
                    rect: [x_start, y_start, w, h],
                    uv: self.atlas.shade_75_uv,
                    color: fg_rgba,
                });
            }

            // Quadrants
            '\u{2596}'..='\u{259F}' => {
                let w1 = (w / 2.0).round();
                let w2 = w - w1;
                let h1 = (h / 2.0).round();
                let h2 = h - h1;

                let is_tl = match ch {
                    '\u{2598}' | '\u{2599}' | '\u{259A}' | '\u{259B}' | '\u{259C}' => true,
                    _ => false,
                };
                let is_tr = match ch {
                    '\u{259B}' | '\u{259C}' | '\u{259D}' | '\u{259E}' | '\u{259F}' => true,
                    _ => false,
                };
                let is_bl = match ch {
                    '\u{2596}' | '\u{2599}' | '\u{259B}' | '\u{259E}' | '\u{259F}' => true,
                    _ => false,
                };
                let is_br = match ch {
                    '\u{2597}' | '\u{2599}' | '\u{259A}' | '\u{259C}' | '\u{259F}' => true,
                    _ => false,
                };

                // TL
                if is_tl {
                    self.push_solid_quad(x_start, y_start, w1, h1, fg_rgba);
                } else {
                    push_bg(self, x_start, y_start, w1, h1);
                }
                // TR
                if is_tr {
                    self.push_solid_quad(x_start + w1, y_start, w2, h1, fg_rgba);
                } else {
                    push_bg(self, x_start + w1, y_start, w2, h1);
                }
                // BL
                if is_bl {
                    self.push_solid_quad(x_start, y_start + h1, w1, h2, fg_rgba);
                } else {
                    push_bg(self, x_start, y_start + h1, w1, h2);
                }
                // BR
                if is_br {
                    self.push_solid_quad(x_start + w1, y_start + h1, w2, h2, fg_rgba);
                } else {
                    push_bg(self, x_start + w1, y_start + h1, w2, h2);
                }
            }

            _ => {
                self.push_solid_quad(x_start, y_start, w, h, fg_rgba);
            }
        }
    }

    pub fn render(&mut self, view: &wgpu::TextureView) {
        let quad_count = self.quads.len();

        if quad_count > self.instance_buffer_capacity {
            let new_capacity = (quad_count * 3 / 2).max(self.instance_buffer_capacity * 2);
            self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Instance Buffer"),
                size: (new_capacity * std::mem::size_of::<GpuQuad>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_buffer_capacity = new_capacity;
        }

        if quad_count > 0 {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.quads),
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Terminal Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Terminal Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 15.0 / 255.0,
                            g: 17.0 / 255.0,
                            b: 26.0 / 255.0,
                            a: 1.0,
                        }), // Obsidian 0xFF0F111A
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if quad_count > 0 {
                render_pass.set_pipeline(&self.pipeline);
                render_pass.set_bind_group(0, &self.bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                render_pass.draw(0..6, 0..quad_count as u32);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
