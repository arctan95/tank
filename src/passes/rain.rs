use encase::ShaderType;
use glam::{Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

use crate::{
    config::MatrixConfig,
    gpu::{
        additive_blend, buffer_entry, perspective_rh_zo, rain_shader, shader_from_wgsl,
        texture_entry, uniform_bytes, uniform_size, RainStage, RenderTarget, NUM_VERTICES_PER_QUAD,
        RENDER_FORMAT,
    },
    texture::Texture,
};

pub(crate) struct RainPass {
    grid_size: [u32; 2],
    num_quads: u32,
    volumetric: bool,
    isometric: bool,
    _config_buffer: wgpu::Buffer,
    scene_buffer: wgpu::Buffer,
    intro_pipeline: wgpu::ComputePipeline,
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    intro_bind_group: wgpu::BindGroup,
    compute_bind_group: wgpu::BindGroup,
    render_bind_group: wgpu::BindGroup,
    output: RenderTarget,
    high_pass_output: RenderTarget,
}

impl RainPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        time_buffer: &wgpu::Buffer,
        config: MatrixConfig,
    ) -> anyhow::Result<Self> {
        let grid_size = config.grid_size();
        let num_cells = grid_size[0] * grid_size[1];
        let num_quads = if config.volumetric { num_cells } else { 1 };

        let config_uniform = config.to_rain_uniform();
        let config_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Rain Config Uniform Buffer"),
            contents: &uniform_bytes(&config_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let scene_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain Scene Uniform Buffer"),
            size: uniform_size::<SceneUniform>(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let intro_cells_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain Intro Cells Storage Buffer"),
            size: (grid_size[0] as u64) * 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let cells_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain Cells Storage Buffer"),
            size: (num_cells as u64) * 48,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Rain Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let font = config.font.asset();
        let glyph_msdf =
            Texture::from_bytes(device, queue, font.glyph_msdf_bytes, font.glyph_msdf_label)?;
        let glint_msdf = if let Some((bytes, label)) = font.glint_msdf {
            Texture::from_bytes(device, queue, bytes, label)?
        } else {
            Texture::empty(device, queue, "Empty Glint MSDF Texture")
        };
        let base_texture = if let Some(texture) = config.base_texture {
            Texture::from_bytes(device, queue, texture.bytes(), texture.label())?
        } else {
            Texture::empty(device, queue, "Empty Base Texture")
        };
        let glint_texture = if let Some(texture) = config.glint_texture {
            Texture::from_bytes(device, queue, texture.bytes(), texture.label())?
        } else {
            Texture::empty(device, queue, "Empty Glint Texture")
        };

        let intro_shader =
            shader_from_wgsl(device, "Rain Intro Shader", rain_shader(RainStage::Intro));
        let compute_shader = shader_from_wgsl(
            device,
            "Rain Compute Shader",
            rain_shader(RainStage::Compute),
        );
        let render_shader =
            shader_from_wgsl(device, "Rain Render Shader", rain_shader(RainStage::Render));

        let intro_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Rain Intro Pipeline"),
            layout: None,
            module: &intro_shader,
            entry_point: Some("computeIntro"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Rain Compute Pipeline"),
            layout: None,
            module: &compute_shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rain Render Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vertMain"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fragMain"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend: Some(additive_blend()),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend: Some(additive_blend()),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let intro_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rain Intro Bind Group"),
            layout: &intro_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &config_buffer),
                buffer_entry(1, time_buffer),
                buffer_entry(2, &intro_cells_buffer),
            ],
        });
        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rain Compute Bind Group"),
            layout: &compute_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &config_buffer),
                buffer_entry(1, time_buffer),
                buffer_entry(2, &cells_buffer),
                buffer_entry(3, &intro_cells_buffer),
            ],
        });
        let output = RenderTarget::new(device, [1, 1], "Rain Output");
        let high_pass_output = RenderTarget::new(device, [1, 1], "Rain High Pass Output");
        let render_bind_group = Self::create_render_bind_group(
            device,
            &render_pipeline,
            &config_buffer,
            time_buffer,
            &scene_buffer,
            &linear_sampler,
            &glyph_msdf.view,
            &glint_msdf.view,
            &base_texture.view,
            &glint_texture.view,
            &cells_buffer,
        );

        Ok(Self {
            grid_size,
            num_quads,
            volumetric: config.volumetric,
            isometric: config.isometric,
            _config_buffer: config_buffer,
            scene_buffer,
            intro_pipeline,
            compute_pipeline,
            render_pipeline,
            intro_bind_group,
            compute_bind_group,
            render_bind_group,
            output,
            high_pass_output,
        })
    }

    pub(crate) fn build(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
        let aspect_ratio = size[0] as f32 / size[1] as f32;
        let screen_size = if aspect_ratio > 1.0 {
            Vec2::new(1.0, aspect_ratio)
        } else {
            Vec2::new(1.0 / aspect_ratio, 1.0)
        };
        let (camera, transform) = if self.volumetric && self.isometric {
            let camera = if aspect_ratio > 1.0 {
                Mat4::orthographic_rh(
                    -1.5 * aspect_ratio,
                    1.5 * aspect_ratio,
                    -1.5,
                    1.5,
                    -1000.0,
                    1000.0,
                )
            } else {
                Mat4::orthographic_rh(
                    -1.5,
                    1.5,
                    -1.5 / aspect_ratio,
                    1.5 / aspect_ratio,
                    -1000.0,
                    1000.0,
                )
            };
            let transform = Mat4::from_rotation_x(std::f32::consts::PI / 8.0)
                * Mat4::from_rotation_y(std::f32::consts::PI / 4.0)
                * Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0))
                * Mat4::from_scale(Vec3::new(1.0, 1.0, 2.0));
            (camera, transform)
        } else {
            (
                perspective_rh_zo(90.0_f32.to_radians(), aspect_ratio, 0.0001, 1000.0),
                Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0)),
            )
        };
        let scene = SceneUniform {
            screen_size,
            camera,
            transform,
        };
        queue.write_buffer(&self.scene_buffer, 0, &uniform_bytes(&scene));

        self.output = RenderTarget::new(device, size, "Rain Output");
        self.high_pass_output = RenderTarget::new(device, size, "Rain High Pass Output");
    }

    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Rain Intro Compute Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.intro_pipeline);
            pass.set_bind_group(0, &self.intro_bind_group, &[]);
            pass.dispatch_workgroups(self.grid_size[0].div_ceil(32), 1, 1);
        }

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Rain Main Compute Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &self.compute_bind_group, &[]);
            pass.dispatch_workgroups(self.grid_size[0].div_ceil(32), self.grid_size[1], 1);
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Rain Render Pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.output.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.high_pass_output.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, &self.render_bind_group, &[]);
        pass.draw(0..NUM_VERTICES_PER_QUAD * self.num_quads, 0..1);
    }

    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }

    pub(crate) fn high_pass_view(&self) -> &wgpu::TextureView {
        &self.high_pass_output.view
    }

    #[allow(clippy::too_many_arguments)]
    fn create_render_bind_group(
        device: &wgpu::Device,
        pipeline: &wgpu::RenderPipeline,
        config_buffer: &wgpu::Buffer,
        time_buffer: &wgpu::Buffer,
        scene_buffer: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
        glyph_msdf_view: &wgpu::TextureView,
        glint_msdf_view: &wgpu::TextureView,
        base_texture_view: &wgpu::TextureView,
        glint_texture_view: &wgpu::TextureView,
        cells_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rain Render Bind Group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, config_buffer),
                buffer_entry(1, time_buffer),
                buffer_entry(2, scene_buffer),
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                texture_entry(4, glyph_msdf_view),
                texture_entry(5, glint_msdf_view),
                texture_entry(6, base_texture_view),
                texture_entry(7, glint_texture_view),
                buffer_entry(8, cells_buffer),
            ],
        })
    }
}

#[derive(Clone, Copy, ShaderType)]
struct SceneUniform {
    screen_size: Vec2,
    camera: Mat4,
    transform: Mat4,
}
