use std::{borrow::Cow, sync::Arc, time::Instant};

use encase::{ShaderType, UniformBuffer};
use glam::{IVec2, Mat2, Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

mod texture;

use texture::Texture;

const RENDER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const NUM_VERTICES_PER_QUAD: u32 = 6;

pub struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    time_buffer: wgpu::Buffer,
    start_time: Instant,
    frames: i32,
    config: MatrixConfig,
    rain: RainPass,
    bloom: BloomPass,
    palette: PalettePass,
    end: EndPass,
}

impl State {
    pub async fn new(window: Arc<Window>) -> State {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|format| {
                matches!(
                    format,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        let time_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Time Uniform Buffer"),
            size: uniform_size::<TimeUniform>(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let config = MatrixConfig::for_version("classic");
        let rain = RainPass::new(&device, &queue, &time_buffer, config)
            .expect("failed to create rain pass");
        let bloom = BloomPass::new(&device, config);
        let palette = PalettePass::new(&device, &time_buffer, config);
        let end = EndPass::new(&device, surface_config.format);

        let mut state = State {
            window,
            device,
            queue,
            surface,
            surface_config,
            time_buffer,
            start_time: Instant::now(),
            frames: 0,
            config,
            rain,
            bloom,
            palette,
            end,
        };
        state.rebuild_targets();
        state
    }

    pub fn get_window(&self) -> &Window {
        &self.window
    }

    pub fn configure_surface(&self) {
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
            self.rebuild_targets();
        }
    }

    pub fn set_version(&mut self, version: &str) {
        let skip_intro = self.config.skip_intro;
        let mut config = MatrixConfig::for_version(version);
        config.skip_intro = skip_intro;
        self.apply_config(config)
            .expect("failed to rebuild renderer for matrix version");
    }

    pub fn toggle_skip_intro(&mut self) {
        let mut config = self.config;
        config.skip_intro = !config.skip_intro;
        self.apply_config(config)
            .expect("failed to rebuild renderer for intro toggle");
    }

    fn reset_time(&mut self) {
        self.start_time = Instant::now();
        self.frames = 0;
    }

    pub fn render(&mut self) {
        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.configure_surface();
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(wgpu::SurfaceError::OutOfMemory) => panic!("surface is out of memory"),
            Err(wgpu::SurfaceError::Other) => return,
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.write_time();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Matrix Frame Encoder"),
            });

        self.rain.run(&mut encoder);
        self.bloom.run(&mut encoder);
        self.palette.run(&mut encoder);
        self.end.run(&mut encoder, &surface_view);

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
        self.frames += 1;
    }

    fn rebuild_targets(&mut self) {
        let size = [self.surface_config.width, self.surface_config.height];
        self.rain.build(&self.device, &self.queue, size);
        self.bloom
            .build(&self.device, size, self.rain.high_pass_view());
        self.palette.build(
            &self.device,
            size,
            self.rain.output_view(),
            self.bloom.output_view(),
        );
        self.end.build(&self.device, self.palette.output_view());
    }

    fn apply_config(&mut self, config: MatrixConfig) -> anyhow::Result<()> {
        self.rain = RainPass::new(&self.device, &self.queue, &self.time_buffer, config)?;
        self.bloom = BloomPass::new(&self.device, config);
        self.palette = PalettePass::new(&self.device, &self.time_buffer, config);
        self.config = config;
        self.reset_time();
        self.rebuild_targets();
        Ok(())
    }

    fn write_time(&self) {
        let time = TimeUniform {
            seconds: self.start_time.elapsed().as_secs_f32(),
            frames: self.frames,
        };
        self.queue
            .write_buffer(&self.time_buffer, 0, &uniform_bytes(&time));
    }
}

struct RainPass {
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
    fn new(
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

    fn build(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
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

    fn run(&self, encoder: &mut wgpu::CommandEncoder) {
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

    fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }

    fn high_pass_view(&self) -> &wgpu::TextureView {
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

struct BloomPass {
    enabled: bool,
    bloom_size: f32,
    blur_pipeline: wgpu::ComputePipeline,
    combine_pipeline: wgpu::ComputePipeline,
    h_blur_buffer: wgpu::Buffer,
    v_blur_buffer: wgpu::Buffer,
    combine_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    h_blur_pyramid: Vec<RenderTarget>,
    v_blur_pyramid: Vec<RenderTarget>,
    h_blur_bind_groups: Vec<wgpu::BindGroup>,
    v_blur_bind_groups: Vec<wgpu::BindGroup>,
    combine_bind_group: Option<wgpu::BindGroup>,
    output: RenderTarget,
    scaled_size: [u32; 2],
}

impl BloomPass {
    const PYRAMID_HEIGHT: usize = 4;

    fn new(device: &wgpu::Device, config: MatrixConfig) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blur_shader = device
            .create_shader_module(wgpu::include_wgsl!("../matrix/shaders/wgsl/bloomBlur.wgsl"));
        let combine_shader = device.create_shader_module(wgpu::include_wgsl!(
            "../matrix/shaders/wgsl/bloomCombine.wgsl"
        ));
        let blur_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Bloom Blur Pipeline"),
            layout: None,
            module: &blur_shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let combine_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Bloom Combine Pipeline"),
            layout: None,
            module: &combine_shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let h_blur_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Horizontal Blur Uniform Buffer"),
            contents: &uniform_bytes(&BloomBlurConfigUniform {
                bloom_radius: 2.0,
                direction: Vec2::new(1.0, 0.0),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let v_blur_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Vertical Blur Uniform Buffer"),
            contents: &uniform_bytes(&BloomBlurConfigUniform {
                bloom_radius: 2.0,
                direction: Vec2::new(0.0, 1.0),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let combine_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Combine Uniform Buffer"),
            contents: &uniform_bytes(&BloomCombineConfigUniform {
                pyramid_height: Self::PYRAMID_HEIGHT as f32,
                bloom_strength: config.bloom_strength,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            enabled: config.bloom_size > 0.0 && config.bloom_strength > 0.0,
            bloom_size: config.bloom_size,
            blur_pipeline,
            combine_pipeline,
            h_blur_buffer,
            v_blur_buffer,
            combine_buffer,
            sampler,
            h_blur_pyramid: Vec::new(),
            v_blur_pyramid: Vec::new(),
            h_blur_bind_groups: Vec::new(),
            v_blur_bind_groups: Vec::new(),
            combine_bind_group: None,
            output: RenderTarget::new_compute(device, [1, 1], "Bloom Output"),
            scaled_size: [1, 1],
        }
    }

    fn build(
        &mut self,
        device: &wgpu::Device,
        screen_size: [u32; 2],
        high_pass: &wgpu::TextureView,
    ) {
        if !self.enabled {
            self.output = RenderTarget::new_compute(device, [1, 1], "Bloom Disabled Output");
            self.combine_bind_group = None;
            return;
        }

        self.scaled_size = [
            ((screen_size[0] as f32) * self.bloom_size).floor().max(1.0) as u32,
            ((screen_size[1] as f32) * self.bloom_size).floor().max(1.0) as u32,
        ];
        self.h_blur_pyramid =
            make_bloom_pyramid(device, self.scaled_size, "Bloom Horizontal Pyramid");
        self.v_blur_pyramid =
            make_bloom_pyramid(device, self.scaled_size, "Bloom Vertical Pyramid");
        self.output = RenderTarget::new_compute(device, self.scaled_size, "Bloom Output");

        self.h_blur_bind_groups.clear();
        self.v_blur_bind_groups.clear();
        for i in 0..Self::PYRAMID_HEIGHT {
            let src_view = if i == 0 {
                high_pass
            } else {
                &self.h_blur_pyramid[i - 1].view
            };
            self.h_blur_bind_groups.push(self.create_blur_bind_group(
                device,
                &self.h_blur_buffer,
                src_view,
                &self.h_blur_pyramid[i].view,
            ));
            self.v_blur_bind_groups.push(self.create_blur_bind_group(
                device,
                &self.v_blur_buffer,
                &self.h_blur_pyramid[i].view,
                &self.v_blur_pyramid[i].view,
            ));
        }

        self.combine_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Combine Bind Group"),
            layout: &self.combine_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &self.combine_buffer),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(2, &self.v_blur_pyramid[0].view),
                texture_entry(3, &self.v_blur_pyramid[1].view),
                texture_entry(4, &self.v_blur_pyramid[2].view),
                texture_entry(5, &self.v_blur_pyramid[3].view),
                texture_entry(6, &self.output.view),
            ],
        }));
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.enabled {
            return;
        }

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Bloom Compute Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.blur_pipeline);
        for i in 0..Self::PYRAMID_HEIGHT {
            let level_size = bloom_level_size(self.scaled_size, i);
            let dispatch = [level_size[0].div_ceil(32), level_size[1], 1];
            pass.set_bind_group(0, &self.h_blur_bind_groups[i], &[]);
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
            pass.set_bind_group(0, &self.v_blur_bind_groups[i], &[]);
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }

        pass.set_pipeline(&self.combine_pipeline);
        pass.set_bind_group(0, self.combine_bind_group.as_ref().unwrap(), &[]);
        pass.dispatch_workgroups(self.scaled_size[0].div_ceil(32), self.scaled_size[1], 1);
    }

    fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }

    fn create_blur_bind_group(
        &self,
        device: &wgpu::Device,
        config_buffer: &wgpu::Buffer,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Blur Bind Group"),
            layout: &self.blur_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, config_buffer),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(2, input),
                texture_entry(3, output),
            ],
        })
    }
}

struct PalettePass {
    pipeline: wgpu::ComputePipeline,
    config_buffer: wgpu::Buffer,
    palette_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    time_buffer: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    output: RenderTarget,
    screen_size: [u32; 2],
}

impl PalettePass {
    fn new(device: &wgpu::Device, time_buffer: &wgpu::Buffer, config: MatrixConfig) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Palette Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!(
            "../matrix/shaders/wgsl/palettePass.wgsl"
        ));
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Palette Pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let config_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Palette Config Uniform Buffer"),
            contents: &uniform_bytes(&config.to_palette_uniform()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let palette_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Palette Uniform Buffer"),
            contents: bytemuck::cast_slice(&make_palette(config.palette.entries())),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            pipeline,
            config_buffer,
            palette_buffer,
            sampler,
            time_buffer: time_buffer.clone(),
            bind_group: None,
            output: RenderTarget::new_compute(device, [1, 1], "Palette Output"),
            screen_size: [1, 1],
        }
    }

    fn build(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        primary: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
    ) {
        self.screen_size = size;
        self.output = RenderTarget::new_compute(device, size, "Palette Output");
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Palette Bind Group"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &self.config_buffer),
                buffer_entry(1, &self.palette_buffer),
                buffer_entry(2, &self.time_buffer),
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(4, primary),
                texture_entry(5, bloom),
                texture_entry(6, &self.output.view),
            ],
        }));
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Palette Compute Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
        pass.dispatch_workgroups(self.screen_size[0].div_ceil(32), self.screen_size[1], 1);
    }

    fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }
}

struct EndPass {
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bind_group: Option<wgpu::BindGroup>,
}

impl EndPass {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("End Pass Nearest Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let shader =
            device.create_shader_module(wgpu::include_wgsl!("../matrix/shaders/wgsl/endPass.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("End Pass Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertMain"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragMain"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            sampler,
            pipeline,
            bind_group: None,
        }
    }

    fn build(&mut self, device: &wgpu::Device, input: &wgpu::TextureView) {
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("End Pass Bind Group"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(1, input),
            ],
        }));
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("End Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
        pass.draw(0..NUM_VERTICES_PER_QUAD, 0..1);
    }
}

struct RenderTarget {
    view: wgpu::TextureView,
}

impl RenderTarget {
    fn new(device: &wgpu::Device, size: [u32; 2], label: &str) -> Self {
        Self::with_usage(
            device,
            size,
            label,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
    }

    fn new_compute(device: &wgpu::Device, size: [u32; 2], label: &str) -> Self {
        Self::with_usage(
            device,
            size,
            label,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::STORAGE_BINDING,
        )
    }

    fn with_usage(
        device: &wgpu::Device,
        size: [u32; 2],
        label: &str,
        usage: wgpu::TextureUsages,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RENDER_FORMAT,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum FontKind {
    Coptic,
    Gothic,
    MatrixCode,
    Megacity,
    Resurrections,
    HuberfishA,
    HuberfishD,
    GtargTenretniolleh,
    GtargAlientext,
    Neomatrixology,
}

#[derive(Clone, Copy)]
struct FontAsset {
    glyph_msdf_bytes: &'static [u8],
    glyph_msdf_label: &'static str,
    glint_msdf: Option<(&'static [u8], &'static str)>,
    glyph_sequence_length: f32,
    glyph_texture_grid_size: IVec2,
}

impl FontKind {
    fn asset(self) -> FontAsset {
        match self {
            Self::Coptic => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/coptic_msdf.png"),
                glyph_msdf_label: "Coptic MSDF",
                glint_msdf: None,
                glyph_sequence_length: 32.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::Gothic => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/gothic_msdf.png"),
                glyph_msdf_label: "Gothic MSDF",
                glint_msdf: None,
                glyph_sequence_length: 27.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::MatrixCode => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/matrixcode_msdf.png"),
                glyph_msdf_label: "Matrix Code MSDF",
                glint_msdf: None,
                glyph_sequence_length: 57.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::Megacity => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/megacity_msdf.png"),
                glyph_msdf_label: "Megacity MSDF",
                glint_msdf: None,
                glyph_sequence_length: 64.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::Resurrections => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/resurrections_msdf.png"),
                glyph_msdf_label: "Resurrections MSDF",
                glint_msdf: Some((
                    include_bytes!("../matrix/assets/resurrections_glint_msdf.png"),
                    "Resurrections Glint MSDF",
                )),
                glyph_sequence_length: 135.0,
                glyph_texture_grid_size: IVec2::new(13, 12),
            },
            Self::HuberfishA => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/huberfish_a_msdf.png"),
                glyph_msdf_label: "Huberfish A MSDF",
                glint_msdf: None,
                glyph_sequence_length: 34.0,
                glyph_texture_grid_size: IVec2::new(6, 6),
            },
            Self::HuberfishD => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/huberfish_d_msdf.png"),
                glyph_msdf_label: "Huberfish D MSDF",
                glint_msdf: None,
                glyph_sequence_length: 34.0,
                glyph_texture_grid_size: IVec2::new(6, 6),
            },
            Self::GtargTenretniolleh => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/gtarg_tenretniolleh_msdf.png"),
                glyph_msdf_label: "GTArg Tenretniolleh MSDF",
                glint_msdf: None,
                glyph_sequence_length: 36.0,
                glyph_texture_grid_size: IVec2::new(6, 6),
            },
            Self::GtargAlientext => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/gtarg_alientext_msdf.png"),
                glyph_msdf_label: "GTArg Alientext MSDF",
                glint_msdf: None,
                glyph_sequence_length: 38.0,
                glyph_texture_grid_size: IVec2::new(8, 5),
            },
            Self::Neomatrixology => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/neomatrixology_msdf.png"),
                glyph_msdf_label: "Neomatrixology MSDF",
                glint_msdf: None,
                glyph_sequence_length: 12.0,
                glyph_texture_grid_size: IVec2::new(4, 4),
            },
        }
    }
}

#[derive(Clone, Copy)]
enum TextureKind {
    Sand,
    Pixels,
    Mesh,
    Metal,
}

impl TextureKind {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Sand => include_bytes!("../matrix/assets/sand.png"),
            Self::Pixels => include_bytes!("../matrix/assets/pixel_grid.png"),
            Self::Mesh => include_bytes!("../matrix/assets/mesh.png"),
            Self::Metal => include_bytes!("../matrix/assets/metal.png"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sand => "Sand Texture",
            Self::Pixels => "Pixel Grid Texture",
            Self::Mesh => "Mesh Texture",
            Self::Metal => "Metal Texture",
        }
    }
}

#[derive(Clone, Copy)]
enum RippleType {
    Box,
    Circle,
}

impl RippleType {
    fn shader_value(self) -> i32 {
        match self {
            Self::Box => 0,
            Self::Circle => 1,
        }
    }
}

#[derive(Clone, Copy)]
struct PaletteEntry {
    color: Vec3,
    at: f32,
}

#[derive(Clone, Copy)]
struct PaletteSpec {
    entries: [PaletteEntry; 5],
    len: usize,
}

impl PaletteSpec {
    fn new(entries: &[PaletteEntry]) -> Self {
        let mut stored = [PaletteEntry {
            color: Vec3::ZERO,
            at: 0.0,
        }; 5];
        for (index, entry) in entries.iter().enumerate() {
            stored[index] = *entry;
        }
        Self {
            entries: stored,
            len: entries.len(),
        }
    }

    fn entries(&self) -> &[PaletteEntry] {
        &self.entries[..self.len]
    }
}

fn palette_entry(color: Vec3, at: f32) -> PaletteEntry {
    PaletteEntry { color, at }
}

fn hsl_palette_entry(hue: f32, saturation: f32, lightness: f32, at: f32) -> PaletteEntry {
    palette_entry(hsl_to_rgb(hue, saturation, lightness), at)
}

fn default_palette() -> PaletteSpec {
    PaletteSpec::new(&[
        hsl_palette_entry(0.3, 0.9, 0.0, 0.0),
        hsl_palette_entry(0.3, 0.9, 0.2, 0.2),
        hsl_palette_entry(0.3, 0.9, 0.7, 0.7),
        hsl_palette_entry(0.3, 0.9, 0.8, 0.8),
    ])
}

#[derive(Clone, Copy)]
struct MatrixConfig {
    font: FontKind,
    base_texture: Option<TextureKind>,
    glint_texture: Option<TextureKind>,
    glyph_flip: bool,
    glyph_rotation_degrees: f32,
    animation_speed: f32,
    glyph_sequence_length: f32,
    glyph_texture_grid_size: IVec2,
    glyph_height_to_width: f32,
    brightness_threshold: f32,
    brightness_override: f32,
    brightness_decay: f32,
    cycle_speed: f32,
    cycle_frame_skip: i32,
    fall_speed: f32,
    raindrop_length: f32,
    num_columns: u32,
    base_brightness: f32,
    base_contrast: f32,
    glint_brightness: f32,
    glint_contrast: f32,
    glyph_vertical_spacing: f32,
    glyph_edge_crop: f32,
    volumetric: bool,
    isometric: bool,
    density: f32,
    slant: f32,
    has_thunder: bool,
    ripple_type: Option<RippleType>,
    ripple_scale: f32,
    ripple_speed: f32,
    ripple_thickness: f32,
    forward_speed: f32,
    is_polar: bool,
    isolate_cursor: bool,
    isolate_glint: bool,
    loops: bool,
    skip_intro: bool,
    high_pass_threshold: f32,
    bloom_size: f32,
    bloom_strength: f32,
    dither_magnitude: f32,
    background_color: Vec3,
    cursor_color: Vec3,
    glint_color: Vec3,
    cursor_intensity: f32,
    glint_intensity: f32,
    palette: PaletteSpec,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        let font = FontKind::MatrixCode.asset();
        Self {
            font: FontKind::MatrixCode,
            base_texture: None,
            glint_texture: None,
            glyph_flip: false,
            glyph_rotation_degrees: 0.0,
            animation_speed: 1.0,
            glyph_sequence_length: font.glyph_sequence_length,
            glyph_texture_grid_size: font.glyph_texture_grid_size,
            glyph_height_to_width: 1.0,
            brightness_threshold: 0.0,
            brightness_override: 0.0,
            brightness_decay: 1.0,
            cycle_speed: 0.03,
            cycle_frame_skip: 1,
            fall_speed: 0.3,
            raindrop_length: 0.75,
            num_columns: 80,
            base_brightness: -0.5,
            base_contrast: 1.1,
            glint_brightness: -1.5,
            glint_contrast: 2.5,
            glyph_vertical_spacing: 1.0,
            glyph_edge_crop: 0.0,
            volumetric: false,
            isometric: false,
            density: 1.0,
            slant: 0.0,
            has_thunder: false,
            ripple_type: None,
            ripple_scale: 30.0,
            ripple_speed: 0.2,
            ripple_thickness: 0.2,
            forward_speed: 0.25,
            is_polar: false,
            isolate_cursor: true,
            isolate_glint: false,
            loops: false,
            skip_intro: false,
            high_pass_threshold: 0.1,
            bloom_size: 0.4,
            bloom_strength: 0.7,
            dither_magnitude: 0.05,
            background_color: hsl_to_rgb(0.0, 0.0, 0.0),
            cursor_color: hsl_to_rgb(0.242, 1.0, 0.73),
            glint_color: hsl_to_rgb(0.0, 0.0, 1.0),
            cursor_intensity: 2.0,
            glint_intensity: 1.0,
            palette: default_palette(),
        }
    }
}

impl MatrixConfig {
    fn for_version(version: &str) -> Self {
        let mut config = Self::default();
        match version {
            "classic" | "2003" => {}
            "megacity" => {
                config.set_font(FontKind::Megacity);
                config.animation_speed = 0.5;
                config.num_columns = 40;
            }
            "neomatrixology" => {
                config.set_font(FontKind::Neomatrixology);
                config.animation_speed = 0.8;
                config.num_columns = 40;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.15, 0.9, 0.0, 0.0),
                    hsl_palette_entry(0.15, 0.9, 0.2, 0.2),
                    hsl_palette_entry(0.15, 0.9, 0.7, 0.7),
                    hsl_palette_entry(0.15, 0.9, 0.8, 0.8),
                ]);
                config.cursor_color = hsl_to_rgb(0.167, 1.0, 0.75);
                config.cursor_intensity = 2.0;
            }
            "operator" | "throwback" | "1999" => {
                config.cursor_color = hsl_to_rgb(0.375, 1.0, 0.66);
                config.cursor_intensity = 3.0;
                config.bloom_size = 0.6;
                config.bloom_strength = 0.75;
                config.high_pass_threshold = 0.0;
                config.cycle_speed = 0.01;
                config.cycle_frame_skip = 8;
                config.brightness_override = 0.22;
                config.brightness_threshold = 0.0;
                config.fall_speed = 0.6;
                config.glyph_edge_crop = 0.15;
                config.glyph_height_to_width = 1.35;
                config.ripple_type = Some(RippleType::Box);
                config.num_columns = 108;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.4, 0.8, 0.0, 0.0),
                    hsl_palette_entry(0.4, 0.8, 0.5, 0.5),
                    hsl_palette_entry(0.4, 0.8, 1.0, 1.0),
                ]);
                config.raindrop_length = 1.5;
            }
            "nightmare" => {
                config.set_font(FontKind::Gothic);
                config.isolate_cursor = false;
                config.high_pass_threshold = 0.7;
                config.base_brightness = -0.8;
                config.brightness_decay = 0.75;
                config.fall_speed = 1.2;
                config.has_thunder = true;
                config.num_columns = 60;
                config.cycle_speed = 0.35;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.0, 1.0, 0.0, 0.0),
                    hsl_palette_entry(0.0, 1.0, 0.2, 0.2),
                    hsl_palette_entry(0.0, 1.0, 0.4, 0.4),
                    hsl_palette_entry(0.1, 1.0, 0.7, 0.7),
                    hsl_palette_entry(0.2, 1.0, 1.0, 1.0),
                ]);
                config.raindrop_length = 0.5;
                config.slant = 22.5_f32.to_radians();
            }
            "paradise" => {
                config.set_font(FontKind::Coptic);
                config.isolate_cursor = false;
                config.bloom_strength = 1.0;
                config.high_pass_threshold = 0.0;
                config.cycle_speed = 0.005;
                config.base_brightness = -1.3;
                config.base_contrast = 2.0;
                config.brightness_decay = 0.05;
                config.fall_speed = 0.02;
                config.is_polar = true;
                config.ripple_type = Some(RippleType::Circle);
                config.ripple_speed = 0.1;
                config.num_columns = 40;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.0, 0.0, 0.0, 0.0),
                    hsl_palette_entry(0.0, 0.8, 0.3, 0.3),
                    hsl_palette_entry(0.1, 0.8, 0.5, 0.5),
                    hsl_palette_entry(0.1, 1.0, 0.6, 0.6),
                    hsl_palette_entry(0.1, 1.0, 0.9, 0.9),
                ]);
                config.raindrop_length = 0.4;
            }
            "resurrections" | "updated" | "2021" => {
                config.apply_resurrections();
            }
            "trinity" => {
                config.apply_resurrections();
                config.glint_texture = Some(TextureKind::Metal);
                config.base_texture = Some(TextureKind::Pixels);
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.131, 1.0, 0.6);
                config.glint_intensity = 3.0;
                config.glint_brightness = -0.5;
                config.glint_contrast = 1.5;
                config.base_brightness = -0.4;
                config.base_contrast = 1.5;
                config.num_columns = 60;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.37, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.37, 0.6, 0.5, 1.0),
                ]);
                config.cycle_speed = 0.01;
                config.volumetric = true;
                config.forward_speed = 0.2;
                config.raindrop_length = 0.3;
                config.density = 0.75;
            }
            "morpheus" => {
                config.set_font(FontKind::Resurrections);
                config.glint_texture = Some(TextureKind::Mesh);
                config.base_texture = Some(TextureKind::Metal);
                config.glyph_edge_crop = 0.1;
                config.cursor_color = hsl_to_rgb(0.333, 1.0, 0.85);
                config.cursor_intensity = 2.0;
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.4, 1.0, 0.5);
                config.glint_intensity = 2.0;
                config.glint_brightness = -1.5;
                config.glint_contrast = 3.0;
                config.base_brightness = -0.3;
                config.base_contrast = 1.5;
                config.high_pass_threshold = 0.0;
                config.num_columns = 60;
                config.cycle_speed = 0.015;
                config.bloom_strength = 0.7;
                config.fall_speed = 0.3;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.97, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.97, 0.6, 0.5, 1.0),
                ]);
                config.volumetric = true;
                config.forward_speed = 0.1;
                config.raindrop_length = 0.4;
                config.density = 0.75;
            }
            "bugs" => {
                config.set_font(FontKind::Resurrections);
                config.glint_texture = Some(TextureKind::Sand);
                config.base_texture = Some(TextureKind::Metal);
                config.glyph_edge_crop = 0.1;
                config.cursor_color = hsl_to_rgb(0.619, 1.0, 0.65);
                config.cursor_intensity = 2.0;
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.625, 1.0, 0.6);
                config.glint_intensity = 3.0;
                config.glint_brightness = -1.0;
                config.glint_contrast = 3.0;
                config.base_brightness = -0.3;
                config.base_contrast = 1.5;
                config.high_pass_threshold = 0.0;
                config.num_columns = 60;
                config.cycle_speed = 0.01;
                config.bloom_strength = 0.7;
                config.fall_speed = 0.3;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.12, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.14, 0.6, 0.5, 1.0),
                ]);
                config.volumetric = true;
                config.forward_speed = 0.4;
                config.raindrop_length = 0.3;
                config.density = 0.75;
            }
            "palimpsest" => {
                config.set_font(FontKind::HuberfishA);
                config.isolate_cursor = false;
                config.bloom_strength = 0.2;
                config.num_columns = 40;
                config.raindrop_length = 1.2;
                config.cycle_frame_skip = 3;
                config.fall_speed = 0.5;
                config.slant = std::f32::consts::PI * -0.0625;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.15, 0.25, 0.9, 0.0),
                    hsl_palette_entry(0.6, 0.8, 0.1, 0.4),
                ]);
            }
            "twilight" => {
                config.set_font(FontKind::HuberfishD);
                config.cursor_color = hsl_to_rgb(0.167, 1.0, 0.8);
                config.cursor_intensity = 1.5;
                config.bloom_strength = 0.1;
                config.num_columns = 50;
                config.raindrop_length = 0.9;
                config.fall_speed = 0.1;
                config.high_pass_threshold = 0.0;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.6, 1.0, 0.05, 0.0),
                    hsl_palette_entry(0.6, 0.8, 0.1, 0.1),
                    hsl_palette_entry(0.88, 0.8, 0.5, 0.5),
                    hsl_palette_entry(0.15, 1.0, 0.6, 0.8),
                ]);
            }
            "holoplay" => {
                config.set_font(FontKind::Resurrections);
                config.glint_texture = Some(TextureKind::Metal);
                config.glyph_edge_crop = 0.1;
                config.cursor_color = hsl_to_rgb(0.292, 1.0, 0.8);
                config.cursor_intensity = 2.0;
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.131, 1.0, 0.6);
                config.glint_intensity = 3.0;
                config.glint_brightness = -0.5;
                config.glint_contrast = 1.5;
                config.base_brightness = -0.4;
                config.base_contrast = 1.5;
                config.high_pass_threshold = 0.0;
                config.cycle_speed = 0.01;
                config.fall_speed = 0.3;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.37, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.37, 0.6, 0.5, 1.0),
                ]);
                config.raindrop_length = 0.3;
                config.num_columns = 20;
                config.dither_magnitude = 0.0;
                config.bloom_strength = 0.0;
                config.volumetric = true;
                config.forward_speed = 0.0;
                config.density = 3.0;
            }
            "3d" => {
                config.volumetric = true;
                config.fall_speed = 0.5;
                config.cycle_speed = 0.03;
                config.base_brightness = -0.9;
                config.base_contrast = 1.5;
                config.raindrop_length = 0.3;
            }
            _ => {}
        }

        if config.bloom_size <= 0.0 {
            config.bloom_strength = 0.0;
        }

        config
    }

    fn set_font(&mut self, font: FontKind) {
        let asset = font.asset();
        self.font = font;
        self.glyph_sequence_length = asset.glyph_sequence_length;
        self.glyph_texture_grid_size = asset.glyph_texture_grid_size;
    }

    fn apply_resurrections(&mut self) {
        self.set_font(FontKind::Resurrections);
        self.glyph_edge_crop = 0.1;
        self.cursor_color = hsl_to_rgb(0.292, 1.0, 0.8);
        self.cursor_intensity = 2.0;
        self.base_brightness = -0.7;
        self.base_contrast = 1.17;
        self.high_pass_threshold = 0.0;
        self.num_columns = 70;
        self.cycle_speed = 0.03;
        self.bloom_strength = 0.7;
        self.fall_speed = 0.3;
        self.palette = PaletteSpec::new(&[
            hsl_palette_entry(0.375, 0.9, 0.0, 0.0),
            hsl_palette_entry(0.375, 1.0, 0.6, 0.92),
            hsl_palette_entry(0.375, 1.0, 1.0, 1.0),
        ]);
    }

    fn grid_size(self) -> [u32; 2] {
        let density = if self.volumetric { self.density } else { 1.0 };
        [
            (self.num_columns as f32 * density).floor() as u32,
            self.num_columns,
        ]
    }

    fn to_rain_uniform(self) -> RainConfigUniform {
        let grid_size = self.grid_size();
        let glyph_transform =
            Mat2::from_diagonal(Vec2::new(if self.glyph_flip { -1.0 } else { 1.0 }, 1.0))
                * Mat2::from_angle(self.glyph_rotation_degrees.to_radians());
        let slant_scale = 1.0 / ((2.0 * self.slant).sin().abs() * (2.0_f32.sqrt() - 1.0) + 1.0);
        RainConfigUniform {
            animation_speed: self.animation_speed,
            glyph_sequence_length: self.glyph_sequence_length,
            glyph_texture_grid_size: self.glyph_texture_grid_size,
            glyph_height_to_width: self.glyph_height_to_width,
            glyph_transform,
            grid_size: Vec2::new(grid_size[0] as f32, grid_size[1] as f32),
            show_debug_view: 0,
            brightness_threshold: self.brightness_threshold,
            brightness_override: self.brightness_override,
            brightness_decay: self.brightness_decay,
            cursor_brightness: 1.0,
            cycle_speed: self.cycle_speed,
            cycle_frame_skip: self.cycle_frame_skip,
            fall_speed: self.fall_speed,
            has_thunder: self.has_thunder as i32,
            raindrop_length: self.raindrop_length,
            ripple_scale: self.ripple_scale,
            ripple_speed: self.ripple_speed,
            ripple_thickness: self.ripple_thickness,
            ripple_type: self.ripple_type.map_or(-1, RippleType::shader_value),
            msdf_px_range: 4.0,
            forward_speed: self.forward_speed,
            base_brightness: self.base_brightness,
            base_contrast: self.base_contrast,
            glint_brightness: self.glint_brightness,
            glint_contrast: self.glint_contrast,
            has_base_texture: self.base_texture.is_some() as i32,
            has_glint_texture: self.glint_texture.is_some() as i32,
            glyph_vertical_spacing: self.glyph_vertical_spacing,
            glyph_edge_crop: self.glyph_edge_crop,
            is_polar: self.is_polar as i32,
            density: self.density,
            slant_scale,
            slant_vec: Vec2::new(self.slant.cos(), self.slant.sin()),
            volumetric: self.volumetric as i32,
            isolate_cursor: self.isolate_cursor as i32,
            isolate_glint: self.isolate_glint as i32,
            loops: self.loops as i32,
            skip_intro: self.skip_intro as i32,
            high_pass_threshold: self.high_pass_threshold,
        }
    }

    fn to_palette_uniform(self) -> PaletteConfigUniform {
        PaletteConfigUniform {
            dither_magnitude: self.dither_magnitude,
            background_color: self.background_color,
            cursor_color: self.cursor_color,
            glint_color: self.glint_color,
            cursor_intensity: self.cursor_intensity,
            glint_intensity: self.glint_intensity,
        }
    }
}

#[derive(Clone, Copy, ShaderType)]
struct RainConfigUniform {
    animation_speed: f32,
    glyph_sequence_length: f32,
    glyph_texture_grid_size: IVec2,
    glyph_height_to_width: f32,
    glyph_transform: Mat2,
    grid_size: Vec2,
    show_debug_view: i32,
    brightness_threshold: f32,
    brightness_override: f32,
    brightness_decay: f32,
    cursor_brightness: f32,
    cycle_speed: f32,
    cycle_frame_skip: i32,
    fall_speed: f32,
    has_thunder: i32,
    raindrop_length: f32,
    ripple_scale: f32,
    ripple_speed: f32,
    ripple_thickness: f32,
    ripple_type: i32,
    msdf_px_range: f32,
    forward_speed: f32,
    base_brightness: f32,
    base_contrast: f32,
    glint_brightness: f32,
    glint_contrast: f32,
    has_base_texture: i32,
    has_glint_texture: i32,
    glyph_vertical_spacing: f32,
    glyph_edge_crop: f32,
    is_polar: i32,
    density: f32,
    slant_scale: f32,
    slant_vec: Vec2,
    volumetric: i32,
    isolate_cursor: i32,
    isolate_glint: i32,
    loops: i32,
    skip_intro: i32,
    high_pass_threshold: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct TimeUniform {
    seconds: f32,
    frames: i32,
}

#[derive(Clone, Copy, ShaderType)]
struct SceneUniform {
    screen_size: Vec2,
    camera: Mat4,
    transform: Mat4,
}

#[derive(Clone, Copy, ShaderType)]
struct BloomBlurConfigUniform {
    bloom_radius: f32,
    direction: Vec2,
}

#[derive(Clone, Copy, ShaderType)]
struct BloomCombineConfigUniform {
    pyramid_height: f32,
    bloom_strength: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct PaletteConfigUniform {
    dither_magnitude: f32,
    background_color: Vec3,
    cursor_color: Vec3,
    glint_color: Vec3,
    cursor_intensity: f32,
    glint_intensity: f32,
}

#[derive(Clone, Copy)]
enum RainStage {
    Intro,
    Compute,
    Render,
}

fn rain_shader(stage: RainStage) -> String {
    let mut source = include_str!("../matrix/shaders/wgsl/rainPass.wgsl").to_owned();
    let replacements: &[(&str, &str)] = match stage {
        RainStage::Intro => &[
            (
                "@group(0) @binding(2) var<storage, read_write> cells_RW",
                "@group(0) @binding(12) var<storage, read_write> cells_RW",
            ),
            (
                "@group(0) @binding(3) var<storage, read_write> introCells_RO",
                "@group(0) @binding(13) var<storage, read_write> introCells_RO",
            ),
            (
                "@group(0) @binding(2) var<uniform> scene",
                "@group(0) @binding(14) var<uniform> scene",
            ),
            (
                "@group(0) @binding(3) var linearSampler",
                "@group(0) @binding(15) var linearSampler",
            ),
            (
                "@group(0) @binding(4) var glyphMSDFTexture",
                "@group(0) @binding(16) var glyphMSDFTexture",
            ),
            (
                "@group(0) @binding(5) var glintMSDFTexture",
                "@group(0) @binding(17) var glintMSDFTexture",
            ),
            (
                "@group(0) @binding(6) var baseTexture",
                "@group(0) @binding(18) var baseTexture",
            ),
            (
                "@group(0) @binding(7) var glintTexture",
                "@group(0) @binding(19) var glintTexture",
            ),
            (
                "@group(0) @binding(8) var<storage, read> cells_RO",
                "@group(0) @binding(20) var<storage, read> cells_RO",
            ),
        ],
        RainStage::Compute => &[
            (
                "@group(0) @binding(2) var<storage, read_write> introCells_RW",
                "@group(0) @binding(12) var<storage, read_write> introCells_RW",
            ),
            (
                "@group(0) @binding(2) var<uniform> scene",
                "@group(0) @binding(14) var<uniform> scene",
            ),
            (
                "@group(0) @binding(3) var linearSampler",
                "@group(0) @binding(15) var linearSampler",
            ),
            (
                "@group(0) @binding(4) var glyphMSDFTexture",
                "@group(0) @binding(16) var glyphMSDFTexture",
            ),
            (
                "@group(0) @binding(5) var glintMSDFTexture",
                "@group(0) @binding(17) var glintMSDFTexture",
            ),
            (
                "@group(0) @binding(6) var baseTexture",
                "@group(0) @binding(18) var baseTexture",
            ),
            (
                "@group(0) @binding(7) var glintTexture",
                "@group(0) @binding(19) var glintTexture",
            ),
            (
                "@group(0) @binding(8) var<storage, read> cells_RO",
                "@group(0) @binding(20) var<storage, read> cells_RO",
            ),
        ],
        RainStage::Render => &[
            (
                "@group(0) @binding(2) var<storage, read_write> introCells_RW",
                "@group(0) @binding(12) var<storage, read_write> introCells_RW",
            ),
            (
                "@group(0) @binding(2) var<storage, read_write> cells_RW",
                "@group(0) @binding(13) var<storage, read_write> cells_RW",
            ),
            (
                "@group(0) @binding(3) var<storage, read_write> introCells_RO",
                "@group(0) @binding(14) var<storage, read_write> introCells_RO",
            ),
        ],
    };
    for (from, to) in replacements {
        source = source.replace(from, to);
    }
    source
}

fn shader_from_wgsl(
    device: &wgpu::Device,
    label: &'static str,
    source: String,
) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
    })
}

fn uniform_bytes<T>(value: &T) -> Vec<u8>
where
    T: ShaderType + encase::private::WriteInto,
{
    let mut buffer = UniformBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

fn uniform_size<T: ShaderType>() -> wgpu::BufferAddress {
    T::min_size().get()
}

fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn texture_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn make_bloom_pyramid(device: &wgpu::Device, size: [u32; 2], label: &str) -> Vec<RenderTarget> {
    (0..BloomPass::PYRAMID_HEIGHT)
        .map(|level| RenderTarget::new_compute(device, bloom_level_size(size, level), label))
        .collect()
}

fn bloom_level_size(size: [u32; 2], level: usize) -> [u32; 2] {
    let scale = 2_u32.pow(level as u32);
    [(size[0] / scale).max(1), (size[1] / scale).max(1)]
}

fn make_palette(entries: &[PaletteEntry]) -> [[f32; 4]; 512] {
    let mut palette = [[0.0; 4]; 512];
    let mut points: Vec<(Vec3, usize)> = entries
        .iter()
        .map(|entry| {
            (
                entry.color,
                (entry.at.clamp(0.0, 1.0) * 511.0).floor() as usize,
            )
        })
        .collect();
    points.sort_by_key(|(_, index)| *index);

    let first = points[0].0;
    let last = points[points.len() - 1].0;
    points.insert(0, (first, 0));
    points.push((last, 511));

    for window in points.windows(2) {
        let (from_color, from_index) = window[0];
        let (to_color, to_index) = window[1];
        let diff = to_index.saturating_sub(from_index);
        if diff == 0 {
            palette[from_index] = [from_color.x, from_color.y, from_color.z, 0.0];
            continue;
        }
        for i in 0..=diff {
            let ratio = i as f32 / diff as f32;
            let color = from_color * (1.0 - ratio) + to_color * ratio;
            palette[from_index + i] = [color.x, color.y, color.z, 0.0];
        }
    }

    palette
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Vec3 {
    let a = saturation * lightness.min(1.0 - lightness);
    let f = |n: f32| {
        let k = (n + hue * 12.0) % 12.0;
        lightness - a * (-1.0_f32).max((k - 3.0).min(9.0 - k).min(1.0))
    };
    Vec3::new(f(0.0), f(8.0), f(4.0))
}

fn additive_blend() -> wgpu::BlendState {
    let component = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: component,
        alpha: component,
    }
}

fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Mat4 {
    Mat4::perspective_rh(fov_y_radians, aspect_ratio, z_near, z_far)
}
