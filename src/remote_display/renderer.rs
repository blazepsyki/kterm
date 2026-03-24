// SPDX-License-Identifier: MIT OR Apache-2.0
//
// GPU-accelerated RDP display renderer using iced's Shader widget + wgpu.
// Maintains a persistent GPU texture and only uploads dirty regions.

use iced::widget::shader;
use iced::mouse;
use iced::Rectangle;
use std::sync::Arc;

// Re-export wgpu types through iced.
use iced::wgpu;

// ── Uniforms (viewport + texture size) ──────────────────────────────

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    tex_size: [f32; 2],
}

// ── Pipeline — owns GPU resources, created once ─────────────────────

pub struct RdpPipeline {
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    tex_width: u32,
    tex_height: u32,
}

impl RdpPipeline {
    fn create_texture_resources(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        uniform_buf: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rdp_display_texture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rdp_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: uniform_buf.as_entire_binding() },
            ],
        });
        (texture, texture_view, bind_group)
    }
}

impl shader::Pipeline for RdpPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rdp_display_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rdp_display.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rdp_bind_group_layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rdp_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rdp_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rdp_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rdp_uniform_buf"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Start with a 1×1 placeholder; resized on first real frame.
        let (texture, texture_view, bind_group) =
            Self::create_texture_resources(device, &bind_group_layout, &sampler, &uniform_buf, 1, 1);

        Self {
            render_pipeline,
            bind_group_layout,
            sampler,
            texture,
            texture_view,
            bind_group,
            uniform_buf,
            tex_width: 1,
            tex_height: 1,
        }
    }
}

// ── Primitive — per-frame data passed from CPU → GPU ────────────────

#[derive(Debug)]
pub struct RdpFrame {
    pub rgba: Arc<Vec<u8>>,
    pub tex_width: u32,
    pub tex_height: u32,
    pub dirty_rects: Vec<DirtyRect>,
    pub full_upload: bool,
}

#[derive(Debug, Clone)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl shader::Primitive for RdpFrame {
    type Pipeline = RdpPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        let tw = self.tex_width;
        let th = self.tex_height;

        // Recreate texture if the desktop size changed.
        if tw != pipeline.tex_width || th != pipeline.tex_height {
            let (texture, texture_view, bind_group) = RdpPipeline::create_texture_resources(
                device, &pipeline.bind_group_layout, &pipeline.sampler, &pipeline.uniform_buf, tw, th,
            );
            pipeline.texture = texture;
            pipeline.texture_view = texture_view;
            pipeline.bind_group = bind_group;
            pipeline.tex_width = tw;
            pipeline.tex_height = th;
        }

        // Upload pixel data to GPU texture.
        let stride = tw as usize * 4;

        if self.full_upload {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &pipeline.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(th),
                },
                wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
            );
        } else {
            for r in &self.dirty_rects {
                let row_bytes = r.width as usize * 4;
                let mut rect_data = Vec::with_capacity(row_bytes * r.height as usize);
                for row in 0..r.height as usize {
                    let src_y = r.y as usize + row;
                    let src_start = src_y * stride + r.x as usize * 4;
                    let src_end = src_start + row_bytes;
                    if src_end <= self.rgba.len() {
                        rect_data.extend_from_slice(&self.rgba[src_start..src_end]);
                    }
                }
                if !rect_data.is_empty() {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &pipeline.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: r.x, y: r.y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &rect_data,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(r.width * 4),
                            rows_per_image: Some(r.height),
                        },
                        wgpu::Extent3d { width: r.width, height: r.height, depth_or_array_layers: 1 },
                    );
                }
            }
        }

        // Update uniforms (viewport size for aspect-ratio correction in shader).
        let uniforms = Uniforms {
            viewport: [bounds.width, bounds.height],
            tex_size: [tw as f32, th as f32],
        };
        queue.write_buffer(&pipeline.uniform_buf, 0, bytemuck::bytes_of(&uniforms));
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rdp_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_viewport(
            clip_bounds.x as f32,
            clip_bounds.y as f32,
            clip_bounds.width as f32,
            clip_bounds.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, clip_bounds.width, clip_bounds.height);
        pass.set_pipeline(&pipeline.render_pipeline);
        pass.set_bind_group(0, &pipeline.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

// ── Program — iced Shader widget interface ──────────────────────────

pub struct RdpDisplayProgram {
    pub frame: Arc<Vec<u8>>,
    pub tex_width: u32,
    pub tex_height: u32,
    pub dirty_rects: Vec<DirtyRect>,
    pub full_upload: bool,
}

impl<Message> shader::Program<Message> for RdpDisplayProgram {
    type State = ();
    type Primitive = RdpFrame;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        RdpFrame {
            rgba: Arc::clone(&self.frame),
            tex_width: self.tex_width,
            tex_height: self.tex_height,
            dirty_rects: self.dirty_rects.clone(),
            full_upload: self.full_upload,
        }
    }
}
